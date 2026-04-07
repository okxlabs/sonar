use serde::{Deserialize, Serialize};

use crate::discriminator::{NAMESPACE_EVENT, NAMESPACE_GLOBAL, sighash, to_snake_case};

use super::{IdlAccountItem, IdlArg, IdlTypeDefinition};

/// Complete IDL structure including types for full type resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Idl {
    pub address: String,
    pub metadata: IdlMetadata,
    pub instructions: Vec<IdlInstruction>,
    pub types: Option<Vec<IdlTypeDefinition>>,
    #[serde(default)]
    pub events: Option<Vec<IdlEvent>>,
}

impl Idl {
    /// Parse an IDL from JSON bytes.
    ///
    /// This handles both current and legacy IDL formats, normalizes the data,
    /// and returns a `ResolvedIdl` ready for parsing operations.
    pub fn parse(json: &[u8], program_address: &str) -> crate::Result<crate::ResolvedIdl> {
        let raw: super::RawAnchorIdl = serde_json::from_slice(json)?;
        let idl = raw.convert(program_address);
        Ok(crate::ResolvedIdl::new(idl))
    }

    /// Normalize the IDL, ensuring all discriminators are populated.
    pub(crate) fn normalize(mut self, fallback_address: &str) -> Self {
        if self.address.is_empty() {
            self.address = fallback_address.to_string();
        }

        self.instructions = self.instructions.into_iter().map(normalize_instruction).collect();

        self.events = self.events.map(|events| events.into_iter().map(normalize_event).collect());

        self
    }
}

/// IDL metadata (name, version, spec version).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlMetadata {
    pub name: String,
    pub version: String,
    pub spec: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// An instruction definition in the IDL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlInstruction {
    pub name: String,
    #[serde(default)]
    pub discriminator: Option<Vec<u8>>,
    pub accounts: Vec<IdlAccountItem>,
    pub args: Vec<IdlArg>,
}

/// An event definition in the IDL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlEvent {
    pub name: String,
    #[serde(default)]
    pub discriminator: Option<Vec<u8>>,
    #[serde(default, deserialize_with = "super::types::deserialize_optional_idl_fields")]
    pub fields: Option<super::IdlFields>,
}

fn normalize_instruction(mut instruction: IdlInstruction) -> IdlInstruction {
    if instruction.discriminator.is_none() {
        let snake_name = to_snake_case(&instruction.name);
        instruction.discriminator =
            Some(sighash(NAMESPACE_GLOBAL, &snake_name).as_bytes().to_vec());
    }

    instruction
}

fn normalize_event(mut event: IdlEvent) -> IdlEvent {
    if event.discriminator.is_none() {
        event.discriminator = Some(sighash(NAMESPACE_EVENT, &event.name).as_bytes().to_vec());
    }

    event
}
