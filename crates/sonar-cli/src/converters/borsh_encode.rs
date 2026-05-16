/// JSON → Borsh binary data serializer.
use anyhow::{Context, Result, bail};
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
            let b = value.as_bool().ok_or_else(|| {
                anyhow::anyhow!("expected boolean, got {}", value_type_name(value))
            })?;
            buf.push(if b { 1 } else { 0 });
        }
        BorshType::String => {
            let s = value.as_str().ok_or_else(|| {
                anyhow::anyhow!("expected string, got {}", value_type_name(value))
            })?;
            let bytes = s.as_bytes();
            buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        BorshType::Pubkey => {
            let s = value.as_str().ok_or_else(|| {
                anyhow::anyhow!("expected base58 pubkey string, got {}", value_type_name(value))
            })?;
            let decoded = bs58::decode(s)
                .into_vec()
                .with_context(|| format!("invalid base58 pubkey: {s}"))?;
            if decoded.len() != 32 {
                bail!("pubkey must be exactly 32 bytes, got {}", decoded.len());
            }
            buf.extend_from_slice(&decoded);
        }
        BorshType::Vec(inner) => {
            let arr = value.as_array().ok_or_else(|| {
                anyhow::anyhow!("expected array for vec, got {}", value_type_name(value))
            })?;
            buf.extend_from_slice(&(arr.len() as u32).to_le_bytes());
            for (i, item) in arr.iter().enumerate() {
                encode_into(inner, item, buf).with_context(|| format!("in vec element [{i}]"))?;
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
            let arr = value.as_array().ok_or_else(|| {
                anyhow::anyhow!("expected array for [T;{n}], got {}", value_type_name(value))
            })?;
            if arr.len() != *n {
                bail!("expected array of length {n} for [{inner};{n}], got length {}", arr.len());
            }
            for (i, item) in arr.iter().enumerate() {
                encode_into(inner, item, buf).with_context(|| format!("in array element [{i}]"))?;
            }
        }
        BorshType::Tuple(types) => {
            let arr = value.as_array().ok_or_else(|| {
                anyhow::anyhow!("expected array for tuple, got {}", value_type_name(value))
            })?;
            if arr.len() != types.len() {
                bail!(
                    "expected array of length {} for tuple, got length {}",
                    types.len(),
                    arr.len()
                );
            }
            for (i, (ty, val)) in types.iter().zip(arr.iter()).enumerate() {
                encode_into(ty, val, buf).with_context(|| format!("in tuple element [{i}]"))?;
            }
        }
        BorshType::Unit => {
            if !value.is_null() {
                bail!("expected null for unit type, got {}", value_type_name(value));
            }
            // Unit encodes to zero bytes
        }
        BorshType::Enum(variants) => {
            let obj = value.as_object().ok_or_else(|| {
                anyhow::anyhow!(
                    "expected object with \"variant\" field for enum, got {}",
                    value_type_name(value)
                )
            })?;
            let idx = obj.get("variant").and_then(|v| v.as_u64()).ok_or_else(|| {
                anyhow::anyhow!("enum object must have a numeric \"variant\" field")
            })? as usize;
            if idx >= variants.len() {
                bail!("enum variant index {idx} out of range (0..{})", variants.len());
            }
            if idx > u8::MAX as usize {
                bail!("enum variant index {idx} exceeds u8 max (255)");
            }
            buf.push(idx as u8);
            let variant_ty = &variants[idx];
            if !matches!(variant_ty, BorshType::Unit) {
                let payload = obj.get("value").ok_or_else(|| {
                    anyhow::anyhow!("enum variant {idx} requires a \"value\" field")
                })?;
                encode_into(variant_ty, payload, buf)
                    .with_context(|| format!("in enum variant {idx}"))?;
            }
        }
        BorshType::HashSet(inner) => {
            let arr = value.as_array().ok_or_else(|| {
                anyhow::anyhow!("expected array for hashset, got {}", value_type_name(value))
            })?;
            if !supports_total_order(inner) {
                bail!("hashset element type must support total ordering, got `{inner}`");
            }
            // Sort by canonical value ordering (Ord on T), then serialize
            let mut indexed: Vec<(usize, &Value)> = arr.iter().enumerate().collect();
            indexed.sort_by(|(_, a), (_, b)| cmp_values(inner, a, b));
            for pair in indexed.windows(2) {
                if cmp_values(inner, pair[0].1, pair[1].1).is_eq() {
                    bail!(
                        "hashset contains duplicate element at input indexes {} and {}",
                        pair[0].0,
                        pair[1].0
                    );
                }
            }
            buf.extend_from_slice(&(arr.len() as u32).to_le_bytes());
            for (i, item) in indexed {
                encode_into(inner, item, buf)
                    .with_context(|| format!("in hashset element [{i}]"))?;
            }
        }
        BorshType::HashMap(key_ty, val_ty) => {
            let arr = value.as_array().ok_or_else(|| {
                anyhow::anyhow!(
                    "expected array of [key,value] pairs for hashmap, got {}",
                    value_type_name(value)
                )
            })?;
            if !supports_total_order(key_ty) {
                bail!("hashmap key type must support total ordering, got `{key_ty}`");
            }
            // Collect and validate entries, then sort by canonical key ordering (Ord on K)
            let mut indexed: Vec<(usize, &Value)> = Vec::with_capacity(arr.len());
            for (i, pair) in arr.iter().enumerate() {
                let pair_arr = pair.as_array().ok_or_else(|| {
                    anyhow::anyhow!("hashmap entry [{i}] must be a [key, value] array")
                })?;
                if pair_arr.len() != 2 {
                    bail!(
                        "hashmap entry [{i}] must have exactly 2 elements, got {}",
                        pair_arr.len()
                    );
                }
                indexed.push((i, pair));
            }
            indexed.sort_by(|(_, a), (_, b)| {
                let a_key = &a.as_array().unwrap()[0];
                let b_key = &b.as_array().unwrap()[0];
                cmp_values(key_ty, a_key, b_key)
            });
            for pair in indexed.windows(2) {
                let a_key = &pair[0].1.as_array().unwrap()[0];
                let b_key = &pair[1].1.as_array().unwrap()[0];
                if cmp_values(key_ty, a_key, b_key).is_eq() {
                    bail!(
                        "hashmap contains duplicate key at input entries {} and {}",
                        pair[0].0,
                        pair[1].0
                    );
                }
            }
            buf.extend_from_slice(&(arr.len() as u32).to_le_bytes());
            for (i, pair) in indexed {
                let pair_arr = pair.as_array().unwrap();
                encode_into(key_ty, &pair_arr[0], buf)
                    .with_context(|| format!("in hashmap key [{i}]"))?;
                encode_into(val_ty, &pair_arr[1], buf)
                    .with_context(|| format!("in hashmap value [{i}]"))?;
            }
        }
        BorshType::Struct(fields) => {
            let obj = value.as_object().ok_or_else(|| {
                anyhow::anyhow!("expected object for struct, got {}", value_type_name(value))
            })?;
            for (name, ty) in fields {
                let field_val = obj
                    .get(name.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing struct field \"{name}\""))?;
                encode_into(ty, field_val, buf)
                    .with_context(|| format!("in struct field \"{name}\""))?;
            }
        }
        BorshType::Result(ok_ty, err_ty) => {
            let obj = value.as_object().ok_or_else(|| {
                anyhow::anyhow!(
                    "expected object with \"ok\" or \"err\" field for result, got {}",
                    value_type_name(value)
                )
            })?;
            if let Some(ok_val) = obj.get("ok") {
                buf.push(0);
                encode_into(ok_ty, ok_val, buf).context("in result ok")?;
            } else if let Some(err_val) = obj.get("err") {
                buf.push(1);
                encode_into(err_ty, err_val, buf).context("in result err")?;
            } else {
                bail!("result object must have either \"ok\" or \"err\" field");
            }
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
        return s.parse::<u64>().with_context(|| format!("cannot parse '{s}' as {type_name}"));
    }
    bail!("expected number for {type_name}, got {}", value_type_name(value))
}

fn as_i64(value: &Value, type_name: &str) -> Result<i64> {
    if let Some(n) = value.as_i64() {
        return Ok(n);
    }
    if let Some(s) = value.as_str() {
        return s.parse::<i64>().with_context(|| format!("cannot parse '{s}' as {type_name}"));
    }
    bail!("expected number for {type_name}, got {}", value_type_name(value))
}

fn as_u128(value: &Value, type_name: &str) -> Result<u128> {
    // Try number first (works for small values)
    if let Some(n) = value.as_u64() {
        return Ok(n as u128);
    }
    // String for large values
    if let Some(s) = value.as_str() {
        return s.parse::<u128>().with_context(|| format!("cannot parse '{s}' as {type_name}"));
    }
    bail!("expected number or string for {type_name}, got {}", value_type_name(value))
}

fn as_i128(value: &Value, type_name: &str) -> Result<i128> {
    if let Some(n) = value.as_i64() {
        return Ok(n as i128);
    }
    if let Some(s) = value.as_str() {
        return s.parse::<i128>().with_context(|| format!("cannot parse '{s}' as {type_name}"));
    }
    bail!("expected number or string for {type_name}, got {}", value_type_name(value))
}

fn supports_total_order(ty: &BorshType) -> bool {
    match ty {
        BorshType::U8
        | BorshType::U16
        | BorshType::U32
        | BorshType::U64
        | BorshType::U128
        | BorshType::I8
        | BorshType::I16
        | BorshType::I32
        | BorshType::I64
        | BorshType::I128
        | BorshType::Bool
        | BorshType::String
        | BorshType::Pubkey
        | BorshType::Unit => true,
        BorshType::Vec(inner) | BorshType::Option(inner) | BorshType::Array(inner, _) => {
            supports_total_order(inner)
        }
        BorshType::Tuple(types) | BorshType::Enum(types) => types.iter().all(supports_total_order),
        BorshType::Result(ok, err) => supports_total_order(ok) && supports_total_order(err),
        BorshType::Struct(fields) => {
            fields.iter().all(|(_, field_ty)| supports_total_order(field_ty))
        }
        BorshType::HashSet(_) | BorshType::HashMap(_, _) => false,
    }
}

/// Compare two JSON values according to the canonical Ord for the given BorshType.
/// This matches the borsh-rs reference implementation which uses Rust's Ord trait.
fn cmp_values(ty: &BorshType, a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    fn extract_u64(v: &Value) -> u64 {
        v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
    }
    fn extract_i64(v: &Value) -> i64 {
        v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
    }
    fn extract_u128(v: &Value) -> u128 {
        v.as_u64()
            .map(|n| n as u128)
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0)
    }
    fn extract_i128(v: &Value) -> i128 {
        v.as_i64()
            .map(|n| n as i128)
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0)
    }

    /// Lexicographic comparison of two JSON arrays by element type.
    fn cmp_array_elements(
        a: &Value,
        b: &Value,
        types: impl Iterator<Item = impl std::borrow::Borrow<BorshType>>,
    ) -> Ordering {
        match (a.as_array(), b.as_array()) {
            (Some(a), Some(b)) => {
                for (i, ty) in types.enumerate() {
                    match (a.get(i), b.get(i)) {
                        (Some(av), Some(bv)) => {
                            let ord = cmp_values(ty.borrow(), av, bv);
                            if ord != Ordering::Equal {
                                return ord;
                            }
                        }
                        (None, Some(_)) => return Ordering::Less,
                        (Some(_), None) => return Ordering::Greater,
                        (None, None) => break,
                    }
                }
                Ordering::Equal
            }
            _ => Ordering::Equal,
        }
    }

    match ty {
        BorshType::U8 | BorshType::U16 | BorshType::U32 | BorshType::U64 => {
            extract_u64(a).cmp(&extract_u64(b))
        }
        BorshType::U128 => extract_u128(a).cmp(&extract_u128(b)),
        BorshType::I8 | BorshType::I16 | BorshType::I32 | BorshType::I64 => {
            extract_i64(a).cmp(&extract_i64(b))
        }
        BorshType::I128 => extract_i128(a).cmp(&extract_i128(b)),
        BorshType::Bool => {
            let av = a.as_bool().unwrap_or(false) as u8;
            let bv = b.as_bool().unwrap_or(false) as u8;
            av.cmp(&bv)
        }
        BorshType::String => {
            let av = a.as_str().unwrap_or("");
            let bv = b.as_str().unwrap_or("");
            av.cmp(bv)
        }
        BorshType::Pubkey => {
            let a_bytes =
                a.as_str().and_then(|s| bs58::decode(s).into_vec().ok()).unwrap_or_default();
            let b_bytes =
                b.as_str().and_then(|s| bs58::decode(s).into_vec().ok()).unwrap_or_default();
            a_bytes.cmp(&b_bytes)
        }
        BorshType::Unit => Ordering::Equal,
        BorshType::Array(inner, n) => {
            let repeated = std::iter::repeat_n(inner.as_ref(), *n);
            cmp_array_elements(a, b, repeated)
        }
        BorshType::Tuple(types) => cmp_array_elements(a, b, types.iter()),
        BorshType::Vec(inner) => {
            // Vec<T> Ord: lexicographic element comparison, then length
            match (a.as_array(), b.as_array()) {
                (Some(a_arr), Some(b_arr)) => {
                    let min_len = a_arr.len().min(b_arr.len());
                    for i in 0..min_len {
                        let ord = cmp_values(inner, &a_arr[i], &b_arr[i]);
                        if ord != Ordering::Equal {
                            return ord;
                        }
                    }
                    a_arr.len().cmp(&b_arr.len())
                }
                _ => Ordering::Equal,
            }
        }
        BorshType::Option(inner) => {
            // Option<T> Ord: None < Some, then compare inner
            match (a.is_null(), b.is_null()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => cmp_values(inner, a, b),
            }
        }
        BorshType::Struct(fields) => {
            // Struct Ord: lexicographic by field values in declaration order
            match (a.as_object(), b.as_object()) {
                (Some(a_obj), Some(b_obj)) => {
                    for (name, ty) in fields {
                        if let (Some(av), Some(bv)) = (a_obj.get(name), b_obj.get(name)) {
                            let ord = cmp_values(ty, av, bv);
                            if ord != Ordering::Equal {
                                return ord;
                            }
                        }
                    }
                    Ordering::Equal
                }
                _ => Ordering::Equal,
            }
        }
        BorshType::Enum(variants) => {
            // Enum Ord: by variant index, then by variant value
            match (a.as_object(), b.as_object()) {
                (Some(a_obj), Some(b_obj)) => {
                    let a_idx = a_obj.get("variant").and_then(|v| v.as_u64()).unwrap_or(0);
                    let b_idx = b_obj.get("variant").and_then(|v| v.as_u64()).unwrap_or(0);
                    match a_idx.cmp(&b_idx) {
                        Ordering::Equal => {
                            let idx = a_idx as usize;
                            if idx < variants.len() && !matches!(&variants[idx], BorshType::Unit) {
                                let av = a_obj.get("value").unwrap_or(&Value::Null);
                                let bv = b_obj.get("value").unwrap_or(&Value::Null);
                                cmp_values(&variants[idx], av, bv)
                            } else {
                                Ordering::Equal
                            }
                        }
                        ord => ord,
                    }
                }
                _ => Ordering::Equal,
            }
        }
        BorshType::Result(ok_ty, err_ty) => {
            // Result Ord: Ok < Err (variant index 0 < 1), then compare inner
            match (a.as_object(), b.as_object()) {
                (Some(a_obj), Some(b_obj)) => {
                    let a_is_ok = a_obj.contains_key("ok");
                    let b_is_ok = b_obj.contains_key("ok");
                    match (a_is_ok, b_is_ok) {
                        (true, true) => {
                            let av = a_obj.get("ok").unwrap_or(&Value::Null);
                            let bv = b_obj.get("ok").unwrap_or(&Value::Null);
                            cmp_values(ok_ty, av, bv)
                        }
                        (false, false) => {
                            let av = a_obj.get("err").unwrap_or(&Value::Null);
                            let bv = b_obj.get("err").unwrap_or(&Value::Null);
                            cmp_values(err_ty, av, bv)
                        }
                        (true, false) => Ordering::Less,
                        (false, true) => Ordering::Greater,
                    }
                }
                _ => Ordering::Equal,
            }
        }
        BorshType::HashSet(_) | BorshType::HashMap(_, _) => {
            // HashSet/HashMap don't implement Ord in Rust, so they shouldn't
            // appear as keys. Fall back to serialized bytes as a best effort.
            let a_bytes = encode_borsh(ty, a).unwrap_or_default();
            let b_bytes = encode_borsh(ty, b).unwrap_or_default();
            a_bytes.cmp(&b_bytes)
        }
    }
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

    #[test]
    fn encode_unit() {
        assert_eq!(encode("()", json!(null)), Vec::<u8>::new());
    }

    #[test]
    fn encode_enum_unit_variant() {
        // enum<(), u64> — variant 0 (unit)
        assert_eq!(encode("enum<(),u64>", json!({"variant": 0})), vec![0x00]);
    }

    #[test]
    fn encode_enum_data_variant() {
        // enum<(), u64> — variant 1 with value 42
        let mut expected = vec![0x01];
        expected.extend_from_slice(&42u64.to_le_bytes());
        assert_eq!(encode("enum<(),u64>", json!({"variant": 1, "value": 42})), expected);
    }

    #[test]
    fn encode_enum_invalid_variant_index() {
        let ty = super::super::borsh_type::parse_borsh_type("enum<(),u64>").unwrap();
        assert!(encode_borsh(&ty, &json!({"variant": 5})).is_err());
    }

    #[test]
    fn encode_enum_missing_variant_field() {
        let ty = super::super::borsh_type::parse_borsh_type("enum<(),u64>").unwrap();
        assert!(encode_borsh(&ty, &json!({"value": 42})).is_err());
    }

    #[test]
    fn encode_result_ok() {
        let mut expected = vec![0x00];
        expected.extend_from_slice(&42u64.to_le_bytes());
        assert_eq!(encode("result<u64,string>", json!({"ok": 42})), expected);
    }

    #[test]
    fn encode_result_err() {
        let mut expected = vec![0x01];
        expected.extend_from_slice(&4u32.to_le_bytes()); // string length
        expected.extend_from_slice(b"fail");
        assert_eq!(encode("result<u64,string>", json!({"err": "fail"})), expected);
    }

    #[test]
    fn encode_result_ambiguous() {
        let ty = super::super::borsh_type::parse_borsh_type("result<u64,string>").unwrap();
        // Neither "ok" nor "err"
        assert!(encode_borsh(&ty, &json!({"foo": 1})).is_err());
    }

    #[test]
    fn roundtrip_enum() {
        use super::super::borsh_decode::decode_borsh;

        let ty = super::super::borsh_type::parse_borsh_type("enum<(),u64,(u32,bool)>").unwrap();
        let original = json!({"variant": 2, "value": [7, true]});
        let encoded = encode_borsh(&ty, &original).unwrap();
        let mut offset = 0;
        let decoded = decode_borsh(&ty, &encoded, &mut offset).unwrap();
        assert_eq!(decoded, original);
        assert_eq!(offset, encoded.len());
    }

    #[test]
    fn encode_hashset() {
        // hashset<u32> with [3, 1, 2] — should be sorted by value order
        let result = encode("hashset<u32>", json!([3, 1, 2]));
        let mut expected = vec![];
        expected.extend_from_slice(&3u32.to_le_bytes()); // count = 3
        expected.extend_from_slice(&1u32.to_le_bytes()); // sorted: 1
        expected.extend_from_slice(&2u32.to_le_bytes()); // 2
        expected.extend_from_slice(&3u32.to_le_bytes()); // 3
        assert_eq!(result, expected);
    }

    #[test]
    fn encode_hashmap() {
        // hashmap<u32,bool> with [[2,false],[1,true]] — should sort by key order
        let result = encode("hashmap<u32,bool>", json!([[2, false], [1, true]]));
        let mut expected = vec![];
        expected.extend_from_slice(&2u32.to_le_bytes()); // count = 2
        expected.extend_from_slice(&1u32.to_le_bytes()); // key 1 (sorted first)
        expected.push(1); // true
        expected.extend_from_slice(&2u32.to_le_bytes()); // key 2
        expected.push(0); // false
        assert_eq!(result, expected);
    }

    #[test]
    fn encode_hashset_value_ordering() {
        // 256 serialized LE = 00010000, 1 serialized LE = 01000000
        // Byte sort would put 256 before 1, but value sort puts 1 before 256
        let result = encode("hashset<u32>", json!([256, 1]));
        let mut expected = vec![];
        expected.extend_from_slice(&2u32.to_le_bytes()); // count = 2
        expected.extend_from_slice(&1u32.to_le_bytes()); // 1 first (value order)
        expected.extend_from_slice(&256u32.to_le_bytes()); // 256 second
        assert_eq!(result, expected);
    }

    #[test]
    fn encode_hashmap_string_key_ordering() {
        // "b" (len=1) vs "aa" (len=2): byte sort would put "b" first (01 < 02),
        // but value sort puts "aa" first (lexicographic)
        let result = encode("hashmap<string,u32>", json!([["b", 2], ["aa", 1]]));
        let mut expected = vec![];
        expected.extend_from_slice(&2u32.to_le_bytes()); // count = 2
        // "aa" first (lexicographic order)
        expected.extend_from_slice(&2u32.to_le_bytes()); // string len
        expected.extend_from_slice(b"aa");
        expected.extend_from_slice(&1u32.to_le_bytes()); // value
        // "b" second
        expected.extend_from_slice(&1u32.to_le_bytes()); // string len
        expected.extend_from_slice(b"b");
        expected.extend_from_slice(&2u32.to_le_bytes()); // value
        assert_eq!(result, expected);
    }

    #[test]
    fn encode_hashset_u64_string_inputs() {
        // u64 values passed as strings should still sort numerically
        let result = encode("hashset<u64>", json!(["256", "1"]));
        let mut expected = vec![];
        expected.extend_from_slice(&2u32.to_le_bytes());
        expected.extend_from_slice(&1u64.to_le_bytes());
        expected.extend_from_slice(&256u64.to_le_bytes());
        assert_eq!(result, expected);
    }

    #[test]
    fn encode_hashset_vec_key_ordering() {
        // vec<u32> elements: [1,2] vs [1,1] — lexicographic element comparison
        let result = encode("hashset<vec<u32>>", json!([[1, 2], [1, 1]]));
        let mut expected = vec![];
        expected.extend_from_slice(&2u32.to_le_bytes()); // count = 2
        // [1,1] first (element-wise: equal first, 1 < 2 second)
        expected.extend_from_slice(&2u32.to_le_bytes()); // vec len
        expected.extend_from_slice(&1u32.to_le_bytes());
        expected.extend_from_slice(&1u32.to_le_bytes());
        // [1,2] second
        expected.extend_from_slice(&2u32.to_le_bytes()); // vec len
        expected.extend_from_slice(&1u32.to_le_bytes());
        expected.extend_from_slice(&2u32.to_le_bytes());
        assert_eq!(result, expected);
    }

    #[test]
    fn encode_hashset_option_ordering() {
        // option<u32>: None < Some(1) < Some(2)
        let result = encode("hashset<option<u32>>", json!([2, null, 1]));
        let mut expected = vec![];
        expected.extend_from_slice(&3u32.to_le_bytes()); // count = 3
        expected.push(0); // None
        expected.push(1); // Some tag
        expected.extend_from_slice(&1u32.to_le_bytes()); // Some(1)
        expected.push(1); // Some tag
        expected.extend_from_slice(&2u32.to_le_bytes()); // Some(2)
        assert_eq!(result, expected);
    }

    #[test]
    fn encode_hashmap_bad_entry() {
        let ty = super::super::borsh_type::parse_borsh_type("hashmap<u32,bool>").unwrap();
        assert!(encode_borsh(&ty, &json!([[1]])).is_err()); // entry with 1 element
    }

    #[test]
    fn encode_hashset_rejects_duplicates() {
        let ty = super::super::borsh_type::parse_borsh_type("hashset<u32>").unwrap();
        let err = encode_borsh(&ty, &json!([2, 1, 2])).unwrap_err();
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn encode_hashmap_rejects_duplicate_keys() {
        let ty = super::super::borsh_type::parse_borsh_type("hashmap<u32,bool>").unwrap();
        let err = encode_borsh(&ty, &json!([[2, false], [1, true], [2, true]])).unwrap_err();
        assert!(err.to_string().contains("duplicate key"));
    }

    #[test]
    fn encode_hashset_rejects_non_ord_element_type() {
        let ty = BorshType::HashSet(Box::new(BorshType::HashSet(Box::new(BorshType::U8))));
        let err = encode_borsh(&ty, &json!([[1], [2]])).unwrap_err();
        assert!(err.to_string().contains("must support total ordering"));
    }

    #[test]
    fn encode_hashmap_rejects_non_ord_key_type() {
        let ty = BorshType::HashMap(
            Box::new(BorshType::HashMap(Box::new(BorshType::U8), Box::new(BorshType::U8))),
            Box::new(BorshType::Bool),
        );
        let err = encode_borsh(&ty, &json!([[[[1, 1]], true]])).unwrap_err();
        assert!(err.to_string().contains("must support total ordering"));
    }

    #[test]
    fn roundtrip_hashset() {
        use super::super::borsh_decode::decode_borsh;

        let ty = super::super::borsh_type::parse_borsh_type("hashset<u32>").unwrap();
        // Input already sorted (roundtrip requires sorted input since encode sorts)
        let original = json!([1, 2, 3]);
        let encoded = encode_borsh(&ty, &original).unwrap();
        let mut offset = 0;
        let decoded = decode_borsh(&ty, &encoded, &mut offset).unwrap();
        assert_eq!(decoded, original);
        assert_eq!(offset, encoded.len());
    }

    #[test]
    fn roundtrip_hashmap() {
        use super::super::borsh_decode::decode_borsh;

        let ty = super::super::borsh_type::parse_borsh_type("hashmap<string,u64>").unwrap();
        // Input unsorted — encoder sorts by canonical key ordering (lexicographic)
        let input = json!([["bob", 200], ["alice", 100]]);
        let encoded = encode_borsh(&ty, &input).unwrap();
        let mut offset = 0;
        let decoded = decode_borsh(&ty, &encoded, &mut offset).unwrap();
        // Decoded order matches sorted key order (alice < bob)
        assert_eq!(decoded, json!([["alice", 100], ["bob", 200]]));
        assert_eq!(offset, encoded.len());
    }

    #[test]
    fn encode_struct() {
        let mut expected = 42u64.to_le_bytes().to_vec();
        expected.push(1); // true
        assert_eq!(
            encode("{amount:u64,active:bool}", json!({"amount": 42, "active": true})),
            expected
        );
    }

    #[test]
    fn encode_struct_missing_field() {
        let ty = super::super::borsh_type::parse_borsh_type("{amount:u64,active:bool}").unwrap();
        assert!(encode_borsh(&ty, &json!({"amount": 42})).is_err());
    }

    #[test]
    fn roundtrip_struct() {
        use super::super::borsh_decode::decode_borsh;

        let ty =
            super::super::borsh_type::parse_borsh_type("{name:string,balance:u64,active:bool}")
                .unwrap();
        let original = json!({"name": "alice", "balance": 1000, "active": true});
        let encoded = encode_borsh(&ty, &original).unwrap();
        let mut offset = 0;
        let decoded = decode_borsh(&ty, &encoded, &mut offset).unwrap();
        assert_eq!(decoded, original);
        assert_eq!(offset, encoded.len());
    }

    #[test]
    fn roundtrip_result() {
        use super::super::borsh_decode::decode_borsh;

        let ty = super::super::borsh_type::parse_borsh_type("result<u64,string>").unwrap();
        let original = json!({"err": "oops"});
        let encoded = encode_borsh(&ty, &original).unwrap();
        let mut offset = 0;
        let decoded = decode_borsh(&ty, &encoded, &mut offset).unwrap();
        assert_eq!(decoded, original);
        assert_eq!(offset, encoded.len());
    }
}
