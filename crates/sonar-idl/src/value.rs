/// Domain-native value type for decoded IDL data.
///
/// Unlike `serde_json::Value`, this preserves the full range of Solana numeric
/// types (u128/i128) without forcing stringification for values that exceed
/// JSON's native integer range.
///
/// Serialization is not implemented on purpose — consumers decide how to
/// render `IdlValue` (JSON, terminal, etc.) via [`to_json_value`](Self::to_json_value)
/// or direct pattern matching.
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

impl IdlValue {
    /// Convert to `serde_json::Value` for interop with code that needs JSON.
    ///
    /// - `Uint` / `Int` emit as JSON numbers when the value fits in u64/i64,
    ///   falling back to a string for values that exceed JSON's range.
    /// - `Bytes` emits as a JSON array of numbers.
    /// - `Struct` emits as a JSON object preserving field order.
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
                let map: serde_json::Map<String, serde_json::Value> =
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
    fn uint_within_u64_converts_to_json_number() {
        let jv = IdlValue::Uint(42745410133).to_json_value();
        assert_eq!(jv, serde_json::json!(42745410133u64));
    }

    #[test]
    fn uint_exceeding_u64_converts_to_json_string() {
        let jv = IdlValue::Uint(u128::MAX).to_json_value();
        assert_eq!(jv, serde_json::Value::String(u128::MAX.to_string()));
    }

    #[test]
    fn int_within_i64_converts_to_json_number() {
        let jv = IdlValue::Int(-100).to_json_value();
        assert_eq!(jv, serde_json::json!(-100));
    }

    #[test]
    fn struct_converts_to_json_object() {
        let v = IdlValue::Struct(vec![
            ("amount".into(), IdlValue::Uint(1000)),
            ("owner".into(), IdlValue::String("abc".into())),
        ]);
        let jv = v.to_json_value();
        assert_eq!(jv, serde_json::json!({"amount": 1000, "owner": "abc"}));
    }

    #[test]
    fn to_json_value_nested() {
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
