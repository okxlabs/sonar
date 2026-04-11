use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

/// Domain-native value type for decoded IDL data.
///
/// Unlike `serde_json::Value`, this preserves the full range of Solana numeric
/// types (u128/i128) without forcing stringification for values that exceed
/// JSON's native integer range.
#[derive(Debug, Clone, PartialEq)]
pub enum IdlValue {
    /// Unsigned integer (u8, u16, u32, u64, u128).
    Uint(u128),
    /// Signed integer (i8, i16, i32, i64, i128).
    Int(i128),
    /// Boolean.
    Bool(bool),
    /// UTF-8 string (pubkeys, seeds, string fields).
    String(String),
    /// Byte array (`bytes` type in Anchor IDL).
    Bytes(Vec<u8>),
    /// Ordered named fields (struct, enum variant payload).
    Struct(Vec<(String, IdlValue)>),
    /// Ordered values (vec, fixed-size array, tuple).
    Array(Vec<IdlValue>),
    /// None / null (Option::None).
    Null,
}

impl Serialize for IdlValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Uint(n) => match u64::try_from(*n) {
                Ok(v) => serializer.serialize_u64(v),
                Err(_) => serializer.serialize_str(&n.to_string()),
            },
            Self::Int(n) => match i64::try_from(*n) {
                Ok(v) => serializer.serialize_i64(v),
                Err(_) => serializer.serialize_str(&n.to_string()),
            },
            Self::Bool(b) => serializer.serialize_bool(*b),
            Self::String(s) => serializer.serialize_str(s),
            Self::Bytes(bytes) => {
                let mut seq = serializer.serialize_seq(Some(bytes.len()))?;
                for b in bytes {
                    seq.serialize_element(b)?;
                }
                seq.end()
            }
            Self::Struct(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (name, value) in fields {
                    map.serialize_entry(name, value)?;
                }
                map.end()
            }
            Self::Array(values) => {
                let mut seq = serializer.serialize_seq(Some(values.len()))?;
                for v in values {
                    seq.serialize_element(v)?;
                }
                seq.end()
            }
            Self::Null => serializer.serialize_none(),
        }
    }
}

impl IdlValue {
    /// Convert to `serde_json::Value` for interop with code that still uses JSON.
    pub fn to_json_value(&self) -> serde_json::Value {
        match self {
            Self::Uint(n) => match u64::try_from(*n) {
                Ok(v) => serde_json::Value::Number(v.into()),
                Err(_) => serde_json::Value::String(n.to_string()),
            },
            Self::Int(n) => match i64::try_from(*n) {
                Ok(v) => serde_json::Value::Number(v.into()),
                Err(_) => serde_json::Value::String(n.to_string()),
            },
            Self::Bool(b) => serde_json::Value::Bool(*b),
            Self::String(s) => serde_json::Value::String(s.clone()),
            Self::Bytes(bytes) => serde_json::Value::Array(
                bytes.iter().map(|b| serde_json::Value::Number((*b as u64).into())).collect(),
            ),
            Self::Struct(fields) => {
                let map: serde_json::Map<std::string::String, serde_json::Value> =
                    fields.iter().map(|(k, v)| (k.clone(), v.to_json_value())).collect();
                serde_json::Value::Object(map)
            }
            Self::Array(values) => {
                serde_json::Value::Array(values.iter().map(|v| v.to_json_value()).collect())
            }
            Self::Null => serde_json::Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uint_within_u64_serializes_as_number() {
        let v = IdlValue::Uint(42745410133);
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, "42745410133");
    }

    #[test]
    fn uint_exceeding_u64_serializes_as_string() {
        let v = IdlValue::Uint(u128::MAX);
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, format!("\"{}\"", u128::MAX));
    }

    #[test]
    fn int_within_i64_serializes_as_number() {
        let v = IdlValue::Int(-100);
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, "-100");
    }

    #[test]
    fn struct_serializes_as_ordered_object() {
        let v = IdlValue::Struct(vec![
            ("amount".into(), IdlValue::Uint(1000)),
            ("owner".into(), IdlValue::String("abc".into())),
        ]);
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, r#"{"amount":1000,"owner":"abc"}"#);
    }

    #[test]
    fn to_json_value_roundtrips() {
        let v = IdlValue::Struct(vec![
            ("x".into(), IdlValue::Uint(42)),
            ("y".into(), IdlValue::Null),
            ("z".into(), IdlValue::Array(vec![IdlValue::Bool(true)])),
        ]);
        let jv = v.to_json_value();
        assert_eq!(jv["x"], 42);
        assert!(jv["y"].is_null());
        assert_eq!(jv["z"][0], true);
    }
}
