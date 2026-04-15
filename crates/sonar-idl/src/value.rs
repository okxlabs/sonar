use solana_pubkey::Pubkey;

/// Domain-native value type for decoded Solana instruction/account data.
///
/// Each variant mirrors the corresponding IDL/Solana type exactly,
/// preserving bit width, signedness, and semantic identity (e.g.
/// `Pubkey` vs `String`).
///
/// Used by both `sonar-idl` (Anchor IDL decoder) and the main crate's
/// built-in parsers as the single value representation.
#[derive(Debug, Clone, PartialEq)]
pub enum IdlValue {
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
    String(String),
    Bytes(Vec<u8>),
    /// Ordered named fields (struct, enum variant payload).
    Struct(Vec<(String, IdlValue)>),
    /// Ordered values (vec, fixed-size array, tuple).
    Array(Vec<IdlValue>),
    /// Fieldless (unit) enum variant — carries only a name, no payload.
    EnumUnit(String),
    /// None / null (Option::None).
    Null,
}

impl IdlValue {
    /// Convert to `serde_json::Value`.
    ///
    /// All integer types (including u128/i128) emit as JSON numbers via
    /// `serde_json`'s `arbitrary_precision` feature.
    pub fn to_json_value(&self) -> serde_json::Value {
        use serde_json::{Number, Value};
        match self {
            Self::U8(n) => Value::Number((*n as u64).into()),
            Self::U16(n) => Value::Number((*n as u64).into()),
            Self::U32(n) => Value::Number((*n as u64).into()),
            Self::U64(n) => Value::Number((*n).into()),
            Self::U128(n) => Value::Number(Number::from_u128(*n).unwrap()),
            Self::I8(n) => Value::Number((*n as i64).into()),
            Self::I16(n) => Value::Number((*n as i64).into()),
            Self::I32(n) => Value::Number((*n as i64).into()),
            Self::I64(n) => Value::Number((*n).into()),
            Self::I128(n) => Value::Number(Number::from_i128(*n).unwrap()),
            Self::Bool(b) => Value::Bool(*b),
            Self::Pubkey(p) => Value::String(p.to_string()),
            Self::String(s) => Value::String(s.clone()),
            Self::Bytes(bytes) => {
                Value::Array(bytes.iter().map(|b| Value::Number((*b as u64).into())).collect())
            }
            Self::Struct(fields) => {
                let map: serde_json::Map<std::string::String, Value> =
                    fields.iter().map(|(k, v)| (k.clone(), v.to_json_value())).collect();
                Value::Object(map)
            }
            Self::Array(values) => Value::Array(values.iter().map(|v| v.to_json_value()).collect()),
            Self::EnumUnit(name) => {
                let mut map = serde_json::Map::with_capacity(1);
                map.insert(name.clone(), Value::Null);
                Value::Object(map)
            }
            Self::Null => Value::Null,
        }
    }

    /// Convert to `serde_json::Value` optimised for human-readable display.
    ///
    /// [`EnumUnit`] variants are flattened to a plain string so they render
    /// as `name` instead of `name: null`.  All other values are identical
    /// to [`to_json_value`].
    pub fn to_display_json_value(&self) -> serde_json::Value {
        use serde_json::Value;
        match self {
            Self::EnumUnit(name) => Value::String(name.clone()),
            Self::Struct(fields) => {
                let map: serde_json::Map<std::string::String, Value> =
                    fields.iter().map(|(k, v)| (k.clone(), v.to_display_json_value())).collect();
                Value::Object(map)
            }
            Self::Array(values) => {
                Value::Array(values.iter().map(|v| v.to_display_json_value()).collect())
            }
            _ => self.to_json_value(),
        }
    }
}

impl From<std::string::String> for IdlValue {
    fn from(value: std::string::String) -> Self {
        IdlValue::String(value)
    }
}

impl From<&str> for IdlValue {
    fn from(value: &str) -> Self {
        IdlValue::String(value.to_string())
    }
}

impl PartialEq<&str> for IdlValue {
    fn eq(&self, other: &&str) -> bool {
        match self {
            Self::String(s) => s == *other,
            Self::Pubkey(p) => p.to_string() == *other,
            Self::Bool(b) => *other == if *b { "true" } else { "false" },
            Self::U8(n) => other.parse::<u8>().ok() == Some(*n),
            Self::U16(n) => other.parse::<u16>().ok() == Some(*n),
            Self::U32(n) => other.parse::<u32>().ok() == Some(*n),
            Self::U64(n) => other.parse::<u64>().ok() == Some(*n),
            Self::U128(n) => other.parse::<u128>().ok() == Some(*n),
            Self::I8(n) => other.parse::<i8>().ok() == Some(*n),
            Self::I16(n) => other.parse::<i16>().ok() == Some(*n),
            Self::I32(n) => other.parse::<i32>().ok() == Some(*n),
            Self::I64(n) => other.parse::<i64>().ok() == Some(*n),
            Self::I128(n) => other.parse::<i128>().ok() == Some(*n),
            _ => false,
        }
    }
}

impl PartialEq<std::string::String> for IdlValue {
    fn eq(&self, other: &std::string::String) -> bool {
        self == &other.as_str()
    }
}
