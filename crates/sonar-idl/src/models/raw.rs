use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::{Idl, IdlEvent, IdlInstruction, IdlMetadata, IdlTypeDefinition};

/// Enum to handle both legacy (flat) and new (nested metadata) IDL formats.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RawAnchorIdl {
    Current(Idl),
    Legacy(LegacyIdl),
}

impl RawAnchorIdl {
    pub fn convert(self, program_address: &str) -> Idl {
        match self {
            RawAnchorIdl::Current(idl) => idl.normalize(program_address),
            RawAnchorIdl::Legacy(legacy) => {
                legacy.into_idl(program_address).normalize(program_address)
            }
        }
    }
}

/// Legacy IDL structure (pre-0.30).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyIdl {
    pub version: String,
    pub name: String,
    pub instructions: Vec<IdlInstruction>,
    #[serde(default)]
    pub accounts: Option<Vec<IdlTypeDefinition>>,
    #[serde(default)]
    pub types: Option<Vec<IdlTypeDefinition>>,
    #[serde(default)]
    pub events: Option<Vec<IdlEvent>>,
    #[serde(default)]
    pub errors: Option<Vec<JsonValue>>,
    #[serde(default)]
    pub metadata: Option<IdlMetadata>,
}

impl LegacyIdl {
    fn into_idl(self, address: &str) -> Idl {
        let mut types = self.types.unwrap_or_default();
        if let Some(accounts) = self.accounts {
            types.extend(accounts);
        }

        let metadata = self.metadata.unwrap_or_else(|| IdlMetadata {
            name: self.name.clone(),
            version: self.version.clone(),
            spec: "0.1.0".to_string(),
            description: None,
        });

        Idl {
            address: address.to_string(),
            metadata,
            instructions: self.instructions,
            types: if types.is_empty() { None } else { Some(types) },
            events: self.events,
        }
    }
}
