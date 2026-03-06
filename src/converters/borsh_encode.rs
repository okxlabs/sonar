/// JSON → Borsh binary data serializer.

use anyhow::{bail, Context, Result};
use serde_json::Value;

use super::borsh_type::BorshType;

/// Encode a JSON value into Borsh binary format according to the given type descriptor.
pub fn encode_borsh(ty: &BorshType, value: &Value) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    encode_into(ty, value, &mut buf)?;
    Ok(buf)
}

fn encode_into(ty: &BorshType, value: &Value, buf: &mut Vec<u8>) -> Result<()> {
    match ty {
        BorshType::U8 => {
            let n = as_u64(value, "u8")?;
            if n > u8::MAX as u64 {
                bail!("value {n} out of range for u8 (0..255)");
            }
            buf.push(n as u8);
        }
        BorshType::U16 => {
            let n = as_u64(value, "u16")?;
            if n > u16::MAX as u64 {
                bail!("value {n} out of range for u16");
            }
            buf.extend_from_slice(&(n as u16).to_le_bytes());
        }
        BorshType::U32 => {
            let n = as_u64(value, "u32")?;
            if n > u32::MAX as u64 {
                bail!("value {n} out of range for u32");
            }
            buf.extend_from_slice(&(n as u32).to_le_bytes());
        }
        BorshType::U64 => {
            let n = as_u64(value, "u64")?;
            buf.extend_from_slice(&n.to_le_bytes());
        }
        BorshType::U128 => {
            let n = as_u128(value, "u128")?;
            buf.extend_from_slice(&n.to_le_bytes());
        }
        BorshType::I8 => {
            let n = as_i64(value, "i8")?;
            if n < i8::MIN as i64 || n > i8::MAX as i64 {
                bail!("value {n} out of range for i8");
            }
            buf.push(n as u8);
        }
        BorshType::I16 => {
            let n = as_i64(value, "i16")?;
            if n < i16::MIN as i64 || n > i16::MAX as i64 {
                bail!("value {n} out of range for i16");
            }
            buf.extend_from_slice(&(n as i16).to_le_bytes());
        }
        BorshType::I32 => {
            let n = as_i64(value, "i32")?;
            if n < i32::MIN as i64 || n > i32::MAX as i64 {
                bail!("value {n} out of range for i32");
            }
            buf.extend_from_slice(&(n as i32).to_le_bytes());
        }
        BorshType::I64 => {
            let n = as_i64(value, "i64")?;
            buf.extend_from_slice(&n.to_le_bytes());
        }
        BorshType::I128 => {
            let n = as_i128(value, "i128")?;
            buf.extend_from_slice(&n.to_le_bytes());
        }
        BorshType::Bool => {
            let b = value
                .as_bool()
                .ok_or_else(|| anyhow::anyhow!("expected boolean, got {}", value_type_name(value)))?;
            buf.push(if b { 1 } else { 0 });
        }
        BorshType::String => {
            let s = value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("expected string, got {}", value_type_name(value)))?;
            let bytes = s.as_bytes();
            buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        BorshType::Pubkey => {
            let s = value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("expected base58 pubkey string, got {}", value_type_name(value)))?;
            let decoded = bs58::decode(s)
                .into_vec()
                .with_context(|| format!("invalid base58 pubkey: {s}"))?;
            if decoded.len() != 32 {
                bail!("pubkey must be exactly 32 bytes, got {}", decoded.len());
            }
            buf.extend_from_slice(&decoded);
        }
        BorshType::Vec(inner) => {
            let arr = value
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("expected array for vec, got {}", value_type_name(value)))?;
            buf.extend_from_slice(&(arr.len() as u32).to_le_bytes());
            for (i, item) in arr.iter().enumerate() {
                encode_into(inner, item, buf)
                    .with_context(|| format!("in vec element [{i}]"))?;
            }
        }
        BorshType::Option(inner) => {
            if value.is_null() {
                buf.push(0);
            } else {
                buf.push(1);
                encode_into(inner, value, buf).context("in option value")?;
            }
        }
        BorshType::Array(inner, n) => {
            let arr = value
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("expected array for [T;{n}], got {}", value_type_name(value)))?;
            if arr.len() != *n {
                bail!(
                    "expected array of length {n} for [{inner};{n}], got length {}",
                    arr.len()
                );
            }
            for (i, item) in arr.iter().enumerate() {
                encode_into(inner, item, buf)
                    .with_context(|| format!("in array element [{i}]"))?;
            }
        }
        BorshType::Tuple(types) => {
            let arr = value
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("expected array for tuple, got {}", value_type_name(value)))?;
            if arr.len() != types.len() {
                bail!(
                    "expected array of length {} for tuple, got length {}",
                    types.len(),
                    arr.len()
                );
            }
            for (i, (ty, val)) in types.iter().zip(arr.iter()).enumerate() {
                encode_into(ty, val, buf)
                    .with_context(|| format!("in tuple element [{i}]"))?;
            }
        }
        BorshType::Unit => { /* nothing to encode */ }
        BorshType::Enum(_variants) => {
            anyhow::bail!("enum encoding not yet implemented");
        }
        BorshType::Result(_ok, _err) => {
            anyhow::bail!("result encoding not yet implemented");
        }
    }
    Ok(())
}

fn as_u64(value: &Value, type_name: &str) -> Result<u64> {
    if let Some(n) = value.as_u64() {
        return Ok(n);
    }
    // Also accept string representations for u64
    if let Some(s) = value.as_str() {
        return s
            .parse::<u64>()
            .with_context(|| format!("cannot parse '{s}' as {type_name}"));
    }
    bail!(
        "expected number for {type_name}, got {}",
        value_type_name(value)
    )
}

fn as_i64(value: &Value, type_name: &str) -> Result<i64> {
    if let Some(n) = value.as_i64() {
        return Ok(n);
    }
    if let Some(s) = value.as_str() {
        return s
            .parse::<i64>()
            .with_context(|| format!("cannot parse '{s}' as {type_name}"));
    }
    bail!(
        "expected number for {type_name}, got {}",
        value_type_name(value)
    )
}

fn as_u128(value: &Value, type_name: &str) -> Result<u128> {
    // Try number first (works for small values)
    if let Some(n) = value.as_u64() {
        return Ok(n as u128);
    }
    // String for large values
    if let Some(s) = value.as_str() {
        return s
            .parse::<u128>()
            .with_context(|| format!("cannot parse '{s}' as {type_name}"));
    }
    bail!(
        "expected number or string for {type_name}, got {}",
        value_type_name(value)
    )
}

fn as_i128(value: &Value, type_name: &str) -> Result<i128> {
    if let Some(n) = value.as_i64() {
        return Ok(n as i128);
    }
    if let Some(s) = value.as_str() {
        return s
            .parse::<i128>()
            .with_context(|| format!("cannot parse '{s}' as {type_name}"));
    }
    bail!(
        "expected number or string for {type_name}, got {}",
        value_type_name(value)
    )
}

fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn encode(ty: &str, value: Value) -> Vec<u8> {
        let ty = super::super::borsh_type::parse_borsh_type(ty).unwrap();
        encode_borsh(&ty, &value).unwrap()
    }

    #[test]
    fn encode_u8() {
        assert_eq!(encode("u8", json!(42)), vec![42]);
    }

    #[test]
    fn encode_u16() {
        assert_eq!(encode("u16", json!(1000)), vec![0xe8, 0x03]);
    }

    #[test]
    fn encode_u32() {
        assert_eq!(encode("u32", json!(1)), vec![1, 0, 0, 0]);
    }

    #[test]
    fn encode_u64() {
        assert_eq!(encode("u64", json!(1)), vec![1, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn encode_u128() {
        let expected = {
            let mut v = vec![1u8];
            v.extend_from_slice(&[0u8; 15]);
            v
        };
        assert_eq!(encode("u128", json!("1")), expected);
    }

    #[test]
    fn encode_i8_negative() {
        assert_eq!(encode("i8", json!(-1)), vec![0xff]);
    }

    #[test]
    fn encode_i64_negative() {
        assert_eq!(encode("i64", json!(-1)), vec![0xff; 8]);
    }

    #[test]
    fn encode_i128_negative() {
        assert_eq!(encode("i128", json!("-1")), vec![0xff; 16]);
    }

    #[test]
    fn encode_bool() {
        assert_eq!(encode("bool", json!(true)), vec![1]);
        assert_eq!(encode("bool", json!(false)), vec![0]);
    }

    #[test]
    fn encode_string() {
        let mut expected = vec![5, 0, 0, 0]; // length = 5
        expected.extend_from_slice(b"hello");
        assert_eq!(encode("string", json!("hello")), expected);
    }

    #[test]
    fn encode_pubkey() {
        let bytes = encode("pubkey", json!("11111111111111111111111111111111"));
        assert_eq!(bytes.len(), 32);
        assert!(bytes.iter().all(|&b| b == 0));
    }

    #[test]
    fn encode_vec_u32() {
        let mut expected = vec![2, 0, 0, 0]; // count = 2
        expected.extend_from_slice(&1u32.to_le_bytes());
        expected.extend_from_slice(&2u32.to_le_bytes());
        assert_eq!(encode("vec<u32>", json!([1, 2])), expected);
    }

    #[test]
    fn encode_option_some() {
        let mut expected = vec![1]; // tag = Some
        expected.extend_from_slice(&1u64.to_le_bytes());
        assert_eq!(encode("option<u64>", json!(1)), expected);
    }

    #[test]
    fn encode_option_none() {
        assert_eq!(encode("option<u64>", json!(null)), vec![0]);
    }

    #[test]
    fn encode_array() {
        assert_eq!(encode("[u8;4]", json!([1, 2, 3, 4])), vec![1, 2, 3, 4]);
    }

    #[test]
    fn encode_tuple() {
        let mut expected = 1u64.to_le_bytes().to_vec();
        expected.push(1); // true
        assert_eq!(encode("(u64,bool)", json!([1, true])), expected);
    }

    #[test]
    fn encode_array_wrong_length() {
        let ty = super::super::borsh_type::parse_borsh_type("[u8;4]").unwrap();
        assert!(encode_borsh(&ty, &json!([1, 2, 3])).is_err());
    }

    #[test]
    fn encode_tuple_wrong_arity() {
        let ty = super::super::borsh_type::parse_borsh_type("(u64,bool)").unwrap();
        assert!(encode_borsh(&ty, &json!([1])).is_err());
    }

    #[test]
    fn encode_type_mismatch() {
        let ty = super::super::borsh_type::parse_borsh_type("u64").unwrap();
        assert!(encode_borsh(&ty, &json!("not a number")).is_err());
    }

    #[test]
    fn encode_u8_overflow() {
        let ty = super::super::borsh_type::parse_borsh_type("u8").unwrap();
        assert!(encode_borsh(&ty, &json!(256)).is_err());
    }

    #[test]
    fn roundtrip() {
        use super::super::borsh_decode::decode_borsh;

        let ty = super::super::borsh_type::parse_borsh_type("(u64,bool,vec<u32>)").unwrap();
        let original = json!([42, true, [1, 2, 3]]);
        let encoded = encode_borsh(&ty, &original).unwrap();
        let mut offset = 0;
        let decoded = decode_borsh(&ty, &encoded, &mut offset).unwrap();
        assert_eq!(decoded, original);
        assert_eq!(offset, encoded.len());
    }
}
