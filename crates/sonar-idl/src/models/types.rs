use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as JsonValue;

/// An IDL type definition (struct or enum).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlTypeDefinition {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: IdlTypeDefinitionBody,
}

/// The body of an IDL type definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IdlTypeDefinitionBody {
    pub kind: IdlTypeDefinitionKind,
    #[serde(default, deserialize_with = "deserialize_optional_idl_fields")]
    pub fields: Option<IdlFields>,
    #[serde(default)]
    pub variants: Option<Vec<IdlEnumVariant>>,
}

/// The kind of type definition (struct or enum).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdlTypeDefinitionKind {
    Struct,
    Enum,
}

impl Serialize for IdlTypeDefinitionKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match self {
            IdlTypeDefinitionKind::Struct => "struct",
            IdlTypeDefinitionKind::Enum => "enum",
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
            _ => return Err(D::Error::custom(format!("unknown type kind: {}", value))),
        })
    }
}

/// An IDL type (primitive, composite, or user-defined).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum IdlType {
    Simple(String),
    Vec { vec: Box<IdlType> },
    Option { option: Box<IdlType> },
    Array { array: IdlArrayType },
    Defined { defined: DefinedType },
}

/// An array type with element type and fixed length.
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

/// A reference to a user-defined type.
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
    /// Get the name of the defined type.
    pub fn name(&self) -> &str {
        match self {
            DefinedType::Simple(name) => name,
            DefinedType::Complex { name, .. } => name,
        }
    }
}

/// An instruction argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlArg {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: IdlType,
}

/// Fields for IDL types — named (regular structs) or tuple (tuple structs).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum IdlFields {
    Named(Vec<IdlField>),
    Tuple(Vec<IdlType>),
}

/// A named field in a struct.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IdlField {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: IdlType,
}

/// A variant in an enum.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IdlEnumVariant {
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_optional_idl_fields")]
    pub fields: Option<IdlFields>,
}

/// Deserialize optional IDL fields, handling both named and tuple formats.
pub(crate) fn deserialize_optional_idl_fields<'de, D>(
    deserializer: D,
) -> Result<Option<IdlFields>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        Some(val) => deserialize_idl_fields_(val).map_err(D::Error::custom),
        None => Ok(None),
    }
}

fn deserialize_idl_fields_(value: serde_json::Value) -> Result<Option<IdlFields>, String> {
    match value {
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                Ok(Some(IdlFields::Tuple(Vec::new())))
            } else if arr[0].get("name").is_some() {
                match serde_json::from_value::<Vec<IdlField>>(serde_json::Value::Array(arr)) {
                    Ok(fields) => Ok(Some(IdlFields::Named(fields))),
                    Err(e) => Err(format!("Failed to parse named fields: {}", e)),
                }
            } else {
                match serde_json::from_value::<Vec<IdlType>>(serde_json::Value::Array(arr)) {
                    Ok(types) => Ok(Some(IdlFields::Tuple(types))),
                    Err(e) => Err(format!("Failed to parse tuple fields: {}", e)),
                }
            }
        }
        _ => Err("Fields must be an array".to_string()),
    }
}
