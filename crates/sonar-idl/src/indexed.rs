use std::collections::HashMap;
use std::ops::Deref;

use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::decode::{
    parse_idl_fields_as_parsed_fields, parse_instruction_args, parse_type_definition,
    raw_unparsed_value,
};
use crate::discriminator::sighash;
use crate::idl::*;

const EMIT_CPI_DISCRIMINATOR: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
const CPI_EVENT_ACCOUNT_NAME: &str = "event_authority";

// ── Output types ──

/// A fully parsed IDL instruction with resolved argument fields.
#[derive(Debug, Clone, Serialize)]
pub struct IdlParsedInstruction {
    pub name: String,
    pub fields: IdlInstructionFields,
    pub account_names: Vec<String>,
}

/// A single parsed field from IDL binary data.
#[derive(Debug, Clone, Serialize)]
pub struct IdlParsedField {
    pub name: String,
    pub value: Value,
}

/// Field decode state for a matched IDL instruction.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IdlInstructionFields {
    Parsed(Vec<IdlParsedField>),
    Unparsed(String),
}

impl IdlInstructionFields {
    pub fn raw_args_hex(&self) -> Option<&str> {
        match self {
            Self::Unparsed(raw_args_hex) => Some(raw_args_hex),
            _ => None,
        }
    }
}

impl Deref for IdlInstructionFields {
    type Target = [IdlParsedField];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Parsed(fields) => fields.as_slice(),
            Self::Unparsed(_) => &[],
        }
    }
}

// ── Indexed IDL ──

#[derive(Debug, Clone)]
pub struct IndexedIdl {
    idl: Idl,
    /// Instruction discriminators sorted by length (longest first) for
    /// correct longest-prefix matching. Supports arbitrary-length
    /// discriminators.
    instruction_discriminators: Vec<(Vec<u8>, usize)>,
    /// Account type discriminators sorted by length (longest first) for
    /// correct longest-prefix matching. Supports arbitrary-length
    /// discriminators.
    account_discriminators: Vec<(Vec<u8>, usize)>,
    /// Event discriminators sorted by length (longest first) for
    /// correct longest-prefix matching. Supports arbitrary-length
    /// discriminators. Falls back to sighash("event", name) when not specified.
    event_discriminators: Vec<(Vec<u8>, usize)>,
    types_by_name: HashMap<String, usize>,
}

impl IndexedIdl {
    pub fn from_json_with_program_address(
        idl_json: &str,
        program_address: &str,
    ) -> serde_json::Result<Self> {
        let raw: RawAnchorIdl = serde_json::from_str(idl_json)?;
        let idl = raw.into_idl(program_address);
        Ok(Self::from_normalized_idl(idl))
    }

    pub(crate) fn from_normalized_idl(idl: Idl) -> Self {
        let mut instruction_discriminators: Vec<(Vec<u8>, usize)> = idl
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(idx, instr)| Some((instr.discriminator.clone()?, idx)))
            .collect();
        // Sort longest first so matching prefers more-specific discriminators.
        instruction_discriminators.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        let mut event_discriminators: Vec<(Vec<u8>, usize)> = Vec::new();
        if let Some(events) = &idl.events {
            for (idx, event) in events.iter().enumerate() {
                let disc = event
                    .discriminator
                    .clone()
                    .unwrap_or_else(|| sighash("event", &event.name).to_vec());
                if !disc.is_empty() {
                    event_discriminators.push((disc, idx));
                }
            }
        }
        event_discriminators.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        let mut account_discriminators: Vec<(Vec<u8>, usize)> = Vec::new();
        let mut types_by_name = HashMap::new();
        if let Some(types) = &idl.types {
            for (idx, type_def) in types.iter().enumerate() {
                types_by_name.insert(type_def.name.clone(), idx);
                if type_def.type_.kind == IdlTypeDefinitionKind::Struct {
                    let disc = type_def
                        .discriminator
                        .clone()
                        .unwrap_or_else(|| sighash("account", &type_def.name).to_vec());
                    if !disc.is_empty() {
                        account_discriminators.push((disc, idx));
                    }
                }
            }
        }
        account_discriminators.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        Self {
            idl,
            instruction_discriminators,
            account_discriminators,
            event_discriminators,
            types_by_name,
        }
    }

    pub fn parse_instruction(&self, data: &[u8]) -> Result<Option<IdlParsedInstruction>> {
        let Some(idl_instruction) = self.find_instruction_by_discriminator(data) else {
            return Ok(None);
        };

        let mut offset = idl_instruction.discriminator.as_ref().map_or(0, |d| d.len());
        let args_offset = offset;
        let fields = if idl_instruction.args.is_empty() {
            IdlInstructionFields::Parsed(Vec::new())
        } else {
            match parse_instruction_args(data, &mut offset, &idl_instruction.args, self) {
                Ok(fields) => IdlInstructionFields::Parsed(fields),
                Err(_) => IdlInstructionFields::Unparsed(hex::encode(
                    data.get(args_offset..).unwrap_or_default(),
                )),
            }
        };
        let account_names = flatten_account_names(&idl_instruction.accounts);

        Ok(Some(IdlParsedInstruction { name: idl_instruction.name.clone(), fields, account_names }))
    }

    pub fn parse_account_data(&self, account_data: &[u8]) -> Result<Option<(String, Value)>> {
        let Some((type_def, disc_len)) = self.find_account_type_by_discriminator(account_data)
        else {
            return Ok(None);
        };

        let mut offset = disc_len;
        let parsed_value = parse_type_definition(account_data, &mut offset, type_def, self)?;

        Ok(Some((type_def.name.clone(), parsed_value)))
    }

    pub fn parse_cpi_event_data(&self, data: &[u8]) -> Result<Option<IdlParsedInstruction>> {
        if data.len() < 8 {
            return Ok(None);
        }

        if data[..8] != EMIT_CPI_DISCRIMINATOR {
            return Ok(None);
        }

        let Some((event_def, disc_len)) = self.find_event_by_discriminator(&data[8..]) else {
            return Ok(None);
        };

        let type_fields = if let Some(fields) = &event_def.fields {
            Some(fields.clone())
        } else {
            self.find_type_definition(&event_def.name)
                .and_then(|type_def| type_def.type_.fields.clone())
        };

        let mut offset = 8 + disc_len;
        let fields = match type_fields.as_ref() {
            Some(fields) => {
                match parse_idl_fields_as_parsed_fields(data, &mut offset, fields, self) {
                    Ok(fields) => IdlInstructionFields::Parsed(fields),
                    Err(_) => IdlInstructionFields::Unparsed(hex::encode(
                        data.get(8 + disc_len..).unwrap_or_default(),
                    )),
                }
            }
            None => {
                let mut raw_fields = Vec::new();
                if offset < data.len() {
                    let raw_data = &data[offset..];
                    raw_fields.push(IdlParsedField {
                        name: "raw_data".into(),
                        value: raw_unparsed_value("event_data", &event_def.name, raw_data),
                    });
                }
                IdlInstructionFields::Parsed(raw_fields)
            }
        };

        Ok(Some(IdlParsedInstruction {
            name: event_def.name.clone(),
            fields,
            account_names: vec![CPI_EVENT_ACCOUNT_NAME.to_string()],
        }))
    }

    pub(crate) fn find_instruction_by_discriminator(&self, data: &[u8]) -> Option<&IdlInstruction> {
        for (disc, idx) in &self.instruction_discriminators {
            if data.len() >= disc.len() && data[..disc.len()] == disc[..] {
                return self.idl.instructions.get(*idx);
            }
        }
        None
    }

    fn find_event_by_discriminator(&self, data: &[u8]) -> Option<(&IdlEvent, usize)> {
        for (disc, idx) in &self.event_discriminators {
            if data.len() >= disc.len() && &data[..disc.len()] == disc.as_slice() {
                return self
                    .idl
                    .events
                    .as_ref()
                    .and_then(|events| events.get(*idx).map(|e| (e, disc.len())));
            }
        }
        None
    }

    fn find_account_type_by_discriminator(
        &self,
        data: &[u8],
    ) -> Option<(&IdlTypeDefinition, usize)> {
        for (disc, idx) in &self.account_discriminators {
            if data.len() >= disc.len() && &data[..disc.len()] == disc.as_slice() {
                return self
                    .idl
                    .types
                    .as_ref()
                    .and_then(|types| types.get(*idx).map(|td| (td, disc.len())));
            }
        }
        None
    }

    pub(crate) fn find_type_definition(&self, name: &str) -> Option<&IdlTypeDefinition> {
        let idx = self.types_by_name.get(name)?;
        self.idl.types.as_ref()?.get(*idx)
    }

    #[cfg(test)]
    pub(crate) fn address_for_tests(&self) -> &str {
        &self.idl.address
    }
}

impl<'de> Deserialize<'de> for IndexedIdl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawAnchorIdl::deserialize(deserializer)?;
        let idl = raw.into_idl("");
        Ok(IndexedIdl::from_normalized_idl(idl))
    }
}

// ── Public helpers ──

/// Check if raw instruction data represents an Anchor CPI event.
pub fn is_cpi_event_data(data: &[u8]) -> bool {
    data.len() >= 8 && data[..8] == EMIT_CPI_DISCRIMINATOR
}

// ── Internal helpers ──

fn flatten_account_names(accounts: &[IdlAccountItem]) -> Vec<String> {
    let mut names = Vec::new();
    for item in accounts {
        match item {
            IdlAccountItem::Account(account) => names.push(account.name.clone()),
            IdlAccountItem::Accounts(group) => names.push(format!("{}: []", group.name)),
        }
    }
    names
}
