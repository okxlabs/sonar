use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use serde_json::Number as JsonNumber;

/// A JSON-like value that preserves insertion order for objects.
///
/// Unlike `serde_json::Value`, object entries are stored as a `Vec<(String, _)>`
/// so field order from IDL parsing is deterministic.
#[derive(Debug, Clone, PartialEq)]
pub enum OrderedJsonValue {
    Null,
    Bool(bool),
    Number(JsonNumber),
    String(String),
    Array(Vec<OrderedJsonValue>),
    Object(Vec<(String, OrderedJsonValue)>),
}

impl Serialize for OrderedJsonValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            OrderedJsonValue::Null => serializer.serialize_unit(),
            OrderedJsonValue::Bool(value) => serializer.serialize_bool(*value),
            OrderedJsonValue::Number(num) => {
                if let Some(value) = num.as_i64() {
                    serializer.serialize_i64(value)
                } else if let Some(value) = num.as_u64() {
                    serializer.serialize_u64(value)
                } else if let Some(value) = num.as_f64() {
                    serializer.serialize_f64(value)
                } else {
                    serializer.serialize_str(&num.to_string())
                }
            }
            OrderedJsonValue::String(value) => serializer.serialize_str(value),
            OrderedJsonValue::Array(values) => {
                let mut seq = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    seq.serialize_element(value)?;
                }
                seq.end()
            }
            OrderedJsonValue::Object(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}
