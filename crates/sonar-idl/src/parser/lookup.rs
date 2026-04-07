use std::collections::{BTreeMap, HashMap};

use crate::discriminator::{NAMESPACE_ACCOUNT, sighash};
use crate::models::*;

use super::{
    IdlParsedInstruction, parse_account_data_with_lookup, parse_cpi_event_data_with_lookup,
    parse_instruction_with_lookup,
};
use crate::error::Result;
use serde_json::Value;

/// A resolved IDL with pre-built lookup indexes for efficient parsing.
#[derive(Debug, Clone)]
pub struct ResolvedIdl {
    idl: Idl,
    instruction_indices_by_length: Vec<(usize, HashMap<Vec<u8>, usize>)>,
    event_indices_by_discriminator: HashMap<[u8; 8], usize>,
    account_type_indices_by_discriminator: HashMap<[u8; 8], usize>,
    type_indices_by_name: HashMap<String, usize>,
}

impl ResolvedIdl {
    /// Create a new `ResolvedIdl` from an `Idl`, building lookup indexes.
    pub fn new(idl: Idl) -> Self {
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
                if let Some(key) = discriminator_key(event.discriminator.as_deref()) {
                    event_indices_by_discriminator.insert(key, idx);
                }
            }
        }

        let mut account_type_indices_by_discriminator = HashMap::new();
        let mut type_indices_by_name = HashMap::new();
        if let Some(types) = &idl.types {
            for (idx, type_def) in types.iter().enumerate() {
                type_indices_by_name.insert(type_def.name.clone(), idx);
                if type_def.type_.kind == IdlTypeDefinitionKind::Struct {
                    account_type_indices_by_discriminator.insert(
                        sighash(NAMESPACE_ACCOUNT, &type_def.name).as_bytes().to_owned(),
                        idx,
                    );
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

    /// Get a reference to the underlying IDL.
    pub fn idl(&self) -> &Idl {
        &self.idl
    }

    /// Parse an instruction from binary data.
    pub fn parse_instruction(&self, data: &[u8]) -> Result<Option<IdlParsedInstruction>> {
        parse_instruction_with_lookup(self, data)
    }

    /// Parse account data.
    pub fn parse_account_data(&self, account_data: &[u8]) -> Result<Option<(String, Value)>> {
        parse_account_data_with_lookup(self, account_data)
    }

    /// Parse CPI event data.
    pub fn parse_cpi_event_data(&self, data: &[u8]) -> Result<Option<IdlParsedInstruction>> {
        parse_cpi_event_data_with_lookup(self, data)
    }

    fn find_instruction_by_discriminator(&self, data: &[u8]) -> Option<&IdlInstruction> {
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

    fn find_event_by_discriminator(&self, discriminator: &[u8]) -> Option<&IdlEvent> {
        let key = discriminator_key(Some(discriminator))?;
        let idx = self.event_indices_by_discriminator.get(&key)?;
        self.idl.events.as_ref()?.get(*idx)
    }

    fn find_account_type_by_discriminator(
        &self,
        discriminator: &[u8],
    ) -> Option<&IdlTypeDefinition> {
        let key = discriminator_key(Some(discriminator))?;
        let idx = self.account_type_indices_by_discriminator.get(&key)?;
        self.idl.types.as_ref()?.get(*idx)
    }

    fn find_type_definition(&self, name: &str) -> Option<&IdlTypeDefinition> {
        let idx = self.type_indices_by_name.get(name)?;
        self.idl.types.as_ref()?.get(*idx)
    }
}

/// Internal trait for IDL lookup operations.
///
/// This trait is implemented by `ResolvedIdl` for efficient lookups.
/// It is intentionally not implemented for `Idl` to encourage use of
/// the indexed `ResolvedIdl` for parsing operations.
pub(super) trait IdlLookup {
    fn find_instruction_by_discriminator(&self, data: &[u8]) -> Option<&IdlInstruction>;
    fn find_event_by_discriminator(&self, discriminator: &[u8]) -> Option<&IdlEvent>;
    fn find_account_type_by_discriminator(
        &self,
        discriminator: &[u8],
    ) -> Option<&IdlTypeDefinition>;
    fn find_type_definition(&self, name: &str) -> Option<&IdlTypeDefinition>;
}

impl IdlLookup for ResolvedIdl {
    fn find_instruction_by_discriminator(&self, data: &[u8]) -> Option<&IdlInstruction> {
        ResolvedIdl::find_instruction_by_discriminator(self, data)
    }

    fn find_event_by_discriminator(&self, discriminator: &[u8]) -> Option<&IdlEvent> {
        ResolvedIdl::find_event_by_discriminator(self, discriminator)
    }

    fn find_account_type_by_discriminator(
        &self,
        discriminator: &[u8],
    ) -> Option<&IdlTypeDefinition> {
        ResolvedIdl::find_account_type_by_discriminator(self, discriminator)
    }

    fn find_type_definition(&self, name: &str) -> Option<&IdlTypeDefinition> {
        ResolvedIdl::find_type_definition(self, name)
    }
}

fn discriminator_key(discriminator: Option<&[u8]>) -> Option<[u8; 8]> {
    let discriminator = discriminator?;
    if discriminator.len() != 8 {
        return None;
    }

    let mut key = [0u8; 8];
    key.copy_from_slice(discriminator);
    Some(key)
}

pub(super) fn scan_instruction_by_discriminator<'a>(
    idl: &'a Idl,
    data: &[u8],
) -> Option<&'a IdlInstruction> {
    idl.instructions
        .iter()
        .filter_map(|inst| {
            let disc = inst.discriminator.as_deref()?;
            let disc_len = disc.len();
            (data.len() >= disc_len && &data[..disc_len] == disc).then_some((disc_len, inst))
        })
        .max_by_key(|(disc_len, _)| *disc_len)
        .map(|(_, inst)| inst)
}

pub(super) fn scan_event_by_discriminator<'a>(
    idl: &'a Idl,
    discriminator: &[u8],
) -> Option<&'a IdlEvent> {
    if let Some(events) = &idl.events {
        events.iter().find(|event| {
            event
                .discriminator
                .as_ref()
                .is_some_and(|d| d.len() == 8 && d.as_slice() == discriminator)
        })
    } else {
        None
    }
}
