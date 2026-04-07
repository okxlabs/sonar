use serde::Serialize;
use serde_json::Value;

mod decode;
mod indexed;

#[cfg(test)]
mod tests;

#[cfg(test)]
use self::decode::{parse_array_type, parse_option_type, parse_simple_type, parse_vec_type};
pub use self::indexed::IndexedIdl;

/// A fully parsed IDL instruction with resolved argument fields.
#[derive(Debug, Clone, Serialize)]
pub struct IdlParsedInstruction {
    pub name: String,
    pub fields: Vec<IdlParsedField>,
    pub account_names: Vec<String>,
}

/// A single parsed field from IDL binary data.
#[derive(Debug, Clone, Serialize)]
pub struct IdlParsedField {
    pub name: String,
    pub value: Value,
}

/// Check if raw instruction data represents an Anchor CPI event.
pub fn is_cpi_event_data(data: &[u8]) -> bool {
    data.len() >= 16 && data[..8] == self::indexed::EMIT_CPI_DISCRIMINATOR
}
