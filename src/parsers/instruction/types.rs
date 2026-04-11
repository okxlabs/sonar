use std::ops::Deref;

use serde::Serialize;
use solana_pubkey::Pubkey;

/// Represents a parsed instruction with human-readable data and account names
#[derive(Debug, Clone, Serialize)]
pub struct ParsedInstruction {
    /// The instruction name (e.g., "Transfer", "CreateAccount")
    pub name: String,
    /// Parsed field state preserving either structured fields or raw hex fallback
    pub fields: ParsedInstructionFields,
    /// Human-readable names for each account in the instruction
    pub account_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum ParsedInstructionFields {
    Parsed(Vec<ParsedField>),
    RawHex(String),
}

impl ParsedInstructionFields {
    /// Returns the parsed fields if decoding succeeded, or `None` for the raw hex variant.
    ///
    /// Prefer this over `Deref` when you need to distinguish "no fields" from "failed to decode".
    pub fn parsed_fields(&self) -> Option<&[ParsedField]> {
        match self {
            Self::Parsed(fields) => Some(fields),
            Self::RawHex(_) => None,
        }
    }
}

impl From<Vec<ParsedField>> for ParsedInstructionFields {
    fn from(fields: Vec<ParsedField>) -> Self {
        Self::Parsed(fields)
    }
}

/// **Caution:** Returns an empty slice for `RawHex`, which is indistinguishable from a
/// zero-field instruction. Use [`ParsedInstructionFields::parsed_fields`] when you need to
/// differentiate between "no fields" and "failed to decode".
impl Deref for ParsedInstructionFields {
    type Target = [ParsedField];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Parsed(fields) => fields.as_slice(),
            Self::RawHex(_) => &[],
        }
    }
}

impl<'a> IntoIterator for &'a ParsedInstructionFields {
    type Item = &'a ParsedField;
    type IntoIter = std::slice::Iter<'a, ParsedField>;

    fn into_iter(self) -> Self::IntoIter {
        self.deref().iter()
    }
}

impl Serialize for ParsedInstructionFields {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Parsed(fields) => fields.serialize(serializer),
            Self::RawHex(raw_hex) => serializer.serialize_str(raw_hex),
        }
    }
}

/// Ordered parsed field entry
#[derive(Debug, Clone, Serialize)]
pub struct ParsedField {
    pub name: String,
    pub value: ParsedFieldValue,
}

impl ParsedField {
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self { name: name.into(), value: ParsedFieldValue::Text(value.into()) }
    }

    pub fn number(name: impl Into<String>, value: u64) -> Self {
        Self { name: name.into(), value: ParsedFieldValue::U64(value) }
    }

    pub fn signed_number(name: impl Into<String>, value: i64) -> Self {
        Self { name: name.into(), value: ParsedFieldValue::I64(value) }
    }

    pub fn boolean(name: impl Into<String>, value: bool) -> Self {
        Self { name: name.into(), value: ParsedFieldValue::Bool(value) }
    }

    pub fn pubkey(name: impl Into<String>, value: Pubkey) -> Self {
        Self { name: name.into(), value: ParsedFieldValue::Pubkey(value) }
    }
}

impl<N, V> From<(N, V)> for ParsedField
where
    N: Into<String>,
    V: Into<String>,
{
    fn from((name, value): (N, V)) -> Self {
        ParsedField::text(name, value)
    }
}

/// Domain-native parsed field value mirroring the IDL/Solana type system.
///
/// Each variant preserves the exact type and bit width from the source,
/// giving consumers full context for serialization and display decisions.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedFieldValue {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    Bool(bool),
    Pubkey(Pubkey),
    Text(String),
    Bytes(Vec<u8>),
    /// Ordered named fields (struct, nested object).
    Struct(Vec<(String, ParsedFieldValue)>),
    /// Ordered values (vec, array).
    Array(Vec<ParsedFieldValue>),
    Null,
}

/// Serialize for `--json` output.
///
/// Delegates to [`to_json_value`](Self::to_json_value) for most variants.
/// Overrides u128/i128 to always emit as strings for stable JSON schema.
impl Serialize for ParsedFieldValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            // u128/i128: always string in JSON for schema stability
            Self::U128(n) => serializer.serialize_str(&n.to_string()),
            Self::I128(n) => serializer.serialize_str(&n.to_string()),
            // Everything else: delegate to the JSON value representation
            _ => self.to_json_value().serialize(serializer),
        }
    }
}

impl ParsedFieldValue {
    /// Convert to `serde_json::Value` for terminal pretty-printing.
    ///
    /// All integer types emit as JSON numbers (u128/i128 as numbers when
    /// they fit in u64/i64, strings when they overflow).
    pub fn to_json_value(&self) -> serde_json::Value {
        use serde_json::Value;
        match self {
            Self::U8(n) => Value::Number((*n as u64).into()),
            Self::U16(n) => Value::Number((*n as u64).into()),
            Self::U32(n) => Value::Number((*n as u64).into()),
            Self::U64(n) => Value::Number((*n).into()),
            Self::U128(n) => match u64::try_from(*n) {
                Ok(v) => Value::Number(v.into()),
                Err(_) => Value::String(n.to_string()),
            },
            Self::I8(n) => Value::Number((*n as i64).into()),
            Self::I16(n) => Value::Number((*n as i64).into()),
            Self::I32(n) => Value::Number((*n as i64).into()),
            Self::I64(n) => Value::Number((*n).into()),
            Self::I128(n) => match i64::try_from(*n) {
                Ok(v) => Value::Number(v.into()),
                Err(_) => Value::String(n.to_string()),
            },
            Self::Bool(b) => Value::Bool(*b),
            Self::Pubkey(p) => Value::String(p.to_string()),
            Self::Text(s) => Value::String(s.clone()),
            Self::Bytes(bytes) => {
                Value::Array(bytes.iter().map(|b| Value::Number((*b as u64).into())).collect())
            }
            Self::Struct(fields) => {
                let map: serde_json::Map<String, Value> =
                    fields.iter().map(|(k, v)| (k.clone(), v.to_json_value())).collect();
                Value::Object(map)
            }
            Self::Array(values) => {
                Value::Array(values.iter().map(|v| v.to_json_value()).collect())
            }
            Self::Null => Value::Null,
        }
    }
}

impl From<String> for ParsedFieldValue {
    fn from(value: String) -> Self {
        ParsedFieldValue::Text(value)
    }
}

impl From<&str> for ParsedFieldValue {
    fn from(value: &str) -> Self {
        ParsedFieldValue::Text(value.to_string())
    }
}

impl PartialEq<&str> for ParsedFieldValue {
    fn eq(&self, other: &&str) -> bool {
        match self {
            Self::Text(s) => s == *other,
            Self::U8(n) => n.to_string() == *other,
            Self::U16(n) => n.to_string() == *other,
            Self::U32(n) => n.to_string() == *other,
            Self::U64(n) => n.to_string() == *other,
            Self::U128(n) => n.to_string() == *other,
            Self::I8(n) => n.to_string() == *other,
            Self::I16(n) => n.to_string() == *other,
            Self::I32(n) => n.to_string() == *other,
            Self::I64(n) => n.to_string() == *other,
            Self::I128(n) => n.to_string() == *other,
            Self::Bool(b) => b.to_string() == *other,
            Self::Pubkey(p) => p.to_string() == *other,
            _ => false,
        }
    }
}

impl PartialEq<String> for ParsedFieldValue {
    fn eq(&self, other: &String) -> bool {
        self == &other.as_str()
    }
}
