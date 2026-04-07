use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as JsonValue;

use super::deserialize_optional_idl_fields;

/// IDL types — mirrors Anchor IDL type system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum IdlType {
    Simple(String),
    Vec { vec: Box<IdlType> },
    Option { option: Box<IdlType> },
    Array { array: IdlArrayType },
    Defined { defined: DefinedType },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(from = "(IdlType, usize)", into = "(IdlType, usize)")]
pub struct IdlArrayType {
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
pub enum DefinedType {
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

/// Custom type definitions (structs and enums).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlTypeDefinition {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: IdlTypeDefinitionBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IdlTypeDefinitionBody {
    pub kind: IdlTypeDefinitionKind,
    #[serde(default, deserialize_with = "deserialize_optional_idl_fields")]
    pub fields: Option<IdlFields>,
    #[serde(default)]
    pub variants: Option<Vec<IdlEnumVariant>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdlTypeDefinitionKind {
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

/// Fields for IDL types — named (regular structs) or tuple (tuple structs).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum IdlFields {
    Named(Vec<IdlField>),
    Tuple(Vec<IdlType>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IdlField {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: IdlType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IdlEnumVariant {
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_optional_idl_fields")]
    pub fields: Option<IdlFields>,
}
