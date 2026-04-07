use serde::Deserialize;
use serde::de::Error;

use super::{IdlField, IdlFields, IdlType};

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
