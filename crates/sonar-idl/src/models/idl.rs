use heck::ToSnakeCase;
use serde::{Deserialize, Serialize};

use crate::discriminator::sighash;

use super::{IdlFields, IdlType, deserialize_optional_idl_fields};

/// Complete IDL structure including types for full type resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Idl {
    pub address: String,
    pub metadata: IdlMetadata,
    pub instructions: Vec<IdlInstruction>,
    pub types: Option<Vec<super::IdlTypeDefinition>>,
    #[serde(default)]
    pub events: Option<Vec<IdlEvent>>,
}

impl Idl {
    pub(crate) fn normalize(mut self, fallback_address: &str) -> Self {
        if self.address.is_empty() {
            self.address = fallback_address.to_string();
        }

        self.instructions = self.instructions.into_iter().map(normalize_instruction).collect();
        self.events = self.events.map(|events| events.into_iter().map(normalize_event).collect());

        self
    }
}

fn normalize_instruction(mut instruction: IdlInstruction) -> IdlInstruction {
    if instruction.discriminator.is_none() {
        let snake_name = instruction.name.to_snake_case();
        instruction.discriminator = Some(sighash("global", &snake_name).to_vec());
    }

    instruction
}

fn normalize_event(mut event: IdlEvent) -> IdlEvent {
    if event.discriminator.is_none() {
        event.discriminator = Some(sighash("event", &event.name).to_vec());
    }

    event
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlMetadata {
    pub name: String,
    pub version: String,
    pub spec: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlInstruction {
    pub name: String,
    #[serde(default)]
    pub discriminator: Option<Vec<u8>>,
    pub accounts: Vec<IdlAccountItem>,
    pub args: Vec<IdlArg>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlEvent {
    pub name: String,
    #[serde(default)]
    pub discriminator: Option<Vec<u8>>,
    #[serde(default, deserialize_with = "deserialize_optional_idl_fields")]
    pub fields: Option<IdlFields>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IdlAccountItem {
    Account(IdlAccount),
    Accounts(IdlAccounts),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlAccount {
    pub name: String,
    #[serde(default, alias = "isMut")]
    pub writable: bool,
    #[serde(default, alias = "isSigner")]
    pub signer: bool,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlAccounts {
    pub name: String,
    pub accounts: Vec<IdlAccountItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlArg {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: IdlType,
}
