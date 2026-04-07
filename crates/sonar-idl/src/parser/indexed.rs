use std::collections::{BTreeMap, HashMap};

use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::discriminator::sighash;
use crate::models::*;

use super::decode::{
    parse_idl_fields_as_parsed_fields, parse_instruction_args, parse_type_definition,
    raw_unparsed_value,
};
use super::{IdlParsedField, IdlParsedInstruction};

pub(super) const EMIT_CPI_DISCRIMINATOR: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];

#[derive(Debug, Clone)]
pub struct IndexedIdl {
    idl: Idl,
    instruction_indices_by_length: Vec<(usize, HashMap<Vec<u8>, usize>)>,
    event_indices_by_discriminator: HashMap<[u8; 8], usize>,
    account_type_indices_by_discriminator: HashMap<[u8; 8], usize>,
    type_indices_by_name: HashMap<String, usize>,
}

impl IndexedIdl {
    pub fn new(idl: Idl) -> Self {
        Self::from_normalized_idl(idl.normalize(""))
    }

    pub(crate) fn from_normalized_idl(idl: Idl) -> Self {
        let mut instruction_indices_by_length = BTreeMap::<usize, HashMap<Vec<u8>, usize>>::new();
        for (idx, instruction) in idl.instructions.iter().enumerate() {
            if let Some(discriminator) = instruction.discriminator.clone() {
                instruction_indices_by_length
                    .entry(discriminator.len())
                    .or_default()
                    .insert(discriminator, idx);
            }
        }

        let mut event_indices_by_discriminator = HashMap::new();
        if let Some(events) = &idl.events {
            for (idx, event) in events.iter().enumerate() {
                if let Some(discriminator) = discriminator_key(event.discriminator.as_deref()) {
                    event_indices_by_discriminator.insert(discriminator, idx);
                }
            }
        }

        let mut account_type_indices_by_discriminator = HashMap::new();
        let mut type_indices_by_name = HashMap::new();
        if let Some(types) = &idl.types {
            for (idx, type_def) in types.iter().enumerate() {
                type_indices_by_name.insert(type_def.name.clone(), idx);
                if type_def.type_.kind == IdlTypeDefinitionKind::Struct {
                    account_type_indices_by_discriminator
                        .insert(sighash("account", &type_def.name), idx);
                }
            }
        }

        Self {
            idl,
            instruction_indices_by_length: instruction_indices_by_length
                .into_iter()
                .rev()
                .collect(),
            event_indices_by_discriminator,
            account_type_indices_by_discriminator,
            type_indices_by_name,
        }
    }

    pub fn idl(&self) -> &Idl {
        &self.idl
    }

    pub fn parse_instruction(&self, data: &[u8]) -> Result<Option<IdlParsedInstruction>> {
        let Some(idl_instruction) = self.find_instruction_by_discriminator(data) else {
            return Ok(None);
        };

        let mut offset = idl_instruction.discriminator.as_ref().map_or(0, |d| d.len());
        let fields = parse_instruction_args(data, &mut offset, &idl_instruction.args, self)?;

        Ok(Some(IdlParsedInstruction {
            name: idl_instruction.name.clone(),
            fields,
            accounts: idl_instruction.accounts.clone(),
        }))
    }

    pub fn parse_account_data(&self, account_data: &[u8]) -> Result<Option<(String, Value)>> {
        if account_data.len() < 8 {
            return Err(anyhow!(
                "Account data too short: {} bytes (expected at least 8 for discriminator)",
                account_data.len()
            ));
        }

        let discriminator = &account_data[..8];

        let Some(type_def) = self.find_account_type_by_discriminator(discriminator) else {
            return Ok(None);
        };

        let mut offset = 8;
        let parsed_value = parse_type_definition(account_data, &mut offset, type_def, self)?;

        Ok(Some((type_def.name.clone(), parsed_value)))
    }

    pub fn parse_cpi_event_data(&self, data: &[u8]) -> Result<Option<IdlParsedInstruction>> {
        if data.len() < 16 {
            return Ok(None);
        }

        if data[..8] != EMIT_CPI_DISCRIMINATOR {
            return Ok(None);
        }

        let event_discriminator = &data[8..16];

        let Some(event_def) = self.find_event_by_discriminator(event_discriminator) else {
            return Ok(None);
        };

        let type_fields = if let Some(fields) = &event_def.fields {
            Some(fields.clone())
        } else {
            self.find_type_definition(&event_def.name)
                .and_then(|type_def| type_def.type_.fields.clone())
        };

        let mut offset = 16;
        let fields = match type_fields.as_ref() {
            Some(fields) => parse_idl_fields_as_parsed_fields(data, &mut offset, fields, self)?,
            None => {
                let mut raw_fields = Vec::new();
                if offset < data.len() {
                    let raw_data = &data[offset..];
                    raw_fields.push(IdlParsedField {
                        name: "raw_data".into(),
                        value: raw_unparsed_value("event_data", &event_def.name, raw_data),
                    });
                }
                raw_fields
            }
        };

        Ok(Some(IdlParsedInstruction {
            name: event_def.name.clone(),
            fields,
            accounts: Vec::new(),
        }))
    }

    pub fn find_instruction_by_discriminator(&self, data: &[u8]) -> Option<&IdlInstruction> {
        for (disc_len, instructions) in &self.instruction_indices_by_length {
            if data.len() < *disc_len {
                continue;
            }
            if let Some(&idx) = instructions.get(&data[..*disc_len]) {
                return self.idl.instructions.get(idx);
            }
        }
        None
    }

    pub fn find_event_by_discriminator(&self, discriminator: &[u8]) -> Option<&IdlEvent> {
        let key = discriminator_key(Some(discriminator))?;
        let idx = self.event_indices_by_discriminator.get(&key)?;
        self.idl.events.as_ref()?.get(*idx)
    }

    pub(super) fn find_account_type_by_discriminator(
        &self,
        discriminator: &[u8],
    ) -> Option<&IdlTypeDefinition> {
        let key = discriminator_key(Some(discriminator))?;
        let idx = self.account_type_indices_by_discriminator.get(&key)?;
        self.idl.types.as_ref()?.get(*idx)
    }

    pub(super) fn find_type_definition(&self, name: &str) -> Option<&IdlTypeDefinition> {
        let idx = self.type_indices_by_name.get(name)?;
        self.idl.types.as_ref()?.get(*idx)
    }
}

pub(super) fn discriminator_key(discriminator: Option<&[u8]>) -> Option<[u8; 8]> {
    let discriminator = discriminator?;
    if discriminator.len() != 8 {
        return None;
    }

    let mut key = [0u8; 8];
    key.copy_from_slice(discriminator);
    Some(key)
}
