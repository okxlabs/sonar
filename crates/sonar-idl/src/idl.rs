use heck::ToSnakeCase;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as JsonValue;

use crate::discriminator::sighash;

// ── Primitive types ──

/// IDL type system — mirrors the Anchor IDL type representation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub(crate) enum IdlType {
    Simple(String),
    Vec { vec: Box<IdlType> },
    Option { option: Box<IdlType> },
    Array { array: IdlArrayType },
    Defined { defined: DefinedType },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(from = "(IdlType, usize)", into = "(IdlType, usize)")]
pub(crate) struct IdlArrayType {
    pub element_type: Box<IdlType>,
    pub length: usize,
}

impl From<(IdlType, usize)> for IdlArrayType {
    fn from((element_type, length): (IdlType, usize)) -> Self {
        Self { element_type: Box::new(element_type), length }
    }
}

impl From<IdlArrayType> for (IdlType, usize) {
    fn from(value: IdlArrayType) -> Self {
        (*value.element_type, value.length)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub(crate) enum DefinedType {
    Simple(String),
    Complex {
        name: String,
        #[serde(default)]
        generics: Option<Vec<JsonValue>>,
    },
}

impl DefinedType {
    pub fn name(&self) -> &str {
        match self {
            DefinedType::Simple(name) => name,
            DefinedType::Complex { name, .. } => name,
        }
    }
}

// ── Field types ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct IdlField {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: IdlType,
}

/// Fields for IDL types — named (regular structs) or tuple (tuple structs).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub(crate) enum IdlFields {
    Named(Vec<IdlField>),
    Tuple(Vec<IdlType>),
}

// ── Serde helpers ──

fn deserialize_idl_fields(value: serde_json::Value) -> Result<Option<IdlFields>, String> {
    match value {
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                Ok(Some(IdlFields::Tuple(Vec::new())))
            } else if arr[0].get("name").is_some() {
                match serde_json::from_value::<Vec<IdlField>>(serde_json::Value::Array(arr)) {
                    Ok(fields) => Ok(Some(IdlFields::Named(fields))),
                    Err(err) => Err(format!("Failed to parse named fields: {}", err)),
                }
            } else {
                match serde_json::from_value::<Vec<IdlType>>(serde_json::Value::Array(arr)) {
                    Ok(types) => Ok(Some(IdlFields::Tuple(types))),
                    Err(err) => Err(format!("Failed to parse tuple fields: {}", err)),
                }
            }
        }
        _ => Err("Fields must be an array".to_string()),
    }
}

pub(crate) fn deserialize_optional_idl_fields<'de, D>(
    deserializer: D,
) -> Result<Option<IdlFields>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        Some(value) => deserialize_idl_fields(value).map_err(D::Error::custom),
        None => Ok(None),
    }
}

// ── Type definitions ──

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IdlTypeDefinitionKind {
    Struct,
    Enum,
    Other(String),
}

impl Serialize for IdlTypeDefinitionKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match self {
            IdlTypeDefinitionKind::Struct => "struct",
            IdlTypeDefinitionKind::Enum => "enum",
            IdlTypeDefinitionKind::Other(value) => value,
        })
    }
}

impl<'de> Deserialize<'de> for IdlTypeDefinitionKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "struct" => IdlTypeDefinitionKind::Struct,
            "enum" => IdlTypeDefinitionKind::Enum,
            _ => IdlTypeDefinitionKind::Other(value),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct IdlEnumVariant {
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_optional_idl_fields")]
    pub fields: Option<IdlFields>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct IdlTypeDefinitionBody {
    pub kind: IdlTypeDefinitionKind,
    #[serde(default, deserialize_with = "deserialize_optional_idl_fields")]
    pub fields: Option<IdlFields>,
    #[serde(default)]
    pub variants: Option<Vec<IdlEnumVariant>>,
}

/// Custom type definitions (structs and enums).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IdlTypeDefinition {
    pub name: String,
    #[serde(default)]
    pub discriminator: Option<Vec<u8>>,
    #[serde(rename = "type")]
    pub type_: IdlTypeDefinitionBody,
}

// ── Instruction / account types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IdlArg {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: IdlType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IdlAccount {
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
pub(crate) struct IdlAccounts {
    pub name: String,
    pub accounts: Vec<IdlAccountItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum IdlAccountItem {
    Account(IdlAccount),
    Accounts(IdlAccounts),
}

// ── Event / metadata ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IdlEvent {
    pub name: String,
    #[serde(default)]
    pub discriminator: Option<Vec<u8>>,
    #[serde(default, deserialize_with = "deserialize_optional_idl_fields")]
    pub fields: Option<IdlFields>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IdlMetadata {
    pub name: String,
    pub version: String,
    pub spec: String,
    #[serde(default)]
    pub description: Option<String>,
}

// ── Top-level IDL ──

/// Complete IDL structure including types for full type resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Idl {
    pub address: String,
    pub metadata: IdlMetadata,
    pub instructions: Vec<IdlInstruction>,
    pub types: Option<Vec<IdlTypeDefinition>>,
    #[serde(default)]
    pub events: Option<Vec<IdlEvent>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IdlInstruction {
    pub name: String,
    #[serde(default)]
    pub discriminator: Option<Vec<u8>>,
    pub accounts: Vec<IdlAccountItem>,
    pub args: Vec<IdlArg>,
}

impl Idl {
    fn normalize(mut self, fallback_address: &str) -> Self {
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

// ── Deserialization wrappers (current + legacy IDL formats) ──

/// Enum to handle both legacy (flat) and new (nested metadata) IDL formats.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum RawAnchorIdl {
    Current(Idl),
    Legacy(LegacyIdl),
}

impl RawAnchorIdl {
    pub(crate) fn into_idl(self, fallback_address: &str) -> Idl {
        match self {
            RawAnchorIdl::Current(idl) => idl.normalize(fallback_address),
            RawAnchorIdl::Legacy(legacy) => {
                legacy.into_idl(fallback_address).normalize(fallback_address)
            }
        }
    }
}

/// Legacy IDL structure (pre-0.30).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LegacyIdl {
    version: String,
    name: String,
    instructions: Vec<IdlInstruction>,
    #[serde(default)]
    accounts: Option<Vec<IdlTypeDefinition>>,
    #[serde(default)]
    types: Option<Vec<IdlTypeDefinition>>,
    #[serde(default)]
    events: Option<Vec<IdlEvent>>,
    #[serde(default)]
    errors: Option<Vec<JsonValue>>,
    #[serde(default)]
    metadata: Option<IdlMetadata>,
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
