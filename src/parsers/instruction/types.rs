use std::ops::Deref;

use serde::Serialize;
use solana_pubkey::Pubkey;
use sonar_idl::IdlValue;

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
    pub value: IdlValue,
}

impl ParsedField {
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self { name: name.into(), value: IdlValue::String(value.into()) }
    }

    pub fn number(name: impl Into<String>, value: u64) -> Self {
        Self { name: name.into(), value: IdlValue::U64(value) }
    }

    pub fn signed_number(name: impl Into<String>, value: i64) -> Self {
        Self { name: name.into(), value: IdlValue::I64(value) }
    }

    pub fn boolean(name: impl Into<String>, value: bool) -> Self {
        Self { name: name.into(), value: IdlValue::Bool(value) }
    }

    pub fn pubkey(name: impl Into<String>, value: Pubkey) -> Self {
        Self { name: name.into(), value: IdlValue::Pubkey(value) }
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
