/// Borsh binary data → JSON deserializer.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use solana_pubkey::Pubkey;

use super::borsh_type::BorshType;

/// Decode Borsh-encoded bytes into a JSON value according to the given type descriptor.
/// Returns the decoded value and the number of bytes consumed.
pub fn decode_borsh(ty: &BorshType, data: &[u8], offset: &mut usize) -> Result<Value> {
    match ty {
        BorshType::U8 => {
            let v = read_bytes(data, offset, 1, "u8")?;
            Ok(Value::Number(v[0].into()))
        }
        BorshType::U16 => {
            let v = read_bytes(data, offset, 2, "u16")?;
            Ok(Value::Number(u16::from_le_bytes(v.try_into().unwrap()).into()))
        }
        BorshType::U32 => {
            let v = read_bytes(data, offset, 4, "u32")?;
            Ok(Value::Number(u32::from_le_bytes(v.try_into().unwrap()).into()))
        }
        BorshType::U64 => {
            let v = read_bytes(data, offset, 8, "u64")?;
            Ok(Value::Number(u64::from_le_bytes(v.try_into().unwrap()).into()))
        }
        BorshType::U128 => {
            let v = read_bytes(data, offset, 16, "u128")?;
            let n = u128::from_le_bytes(v.try_into().unwrap());
            Ok(Value::String(n.to_string()))
        }
        BorshType::I8 => {
            let v = read_bytes(data, offset, 1, "i8")?;
            Ok(Value::Number((v[0] as i8).into()))
        }
        BorshType::I16 => {
            let v = read_bytes(data, offset, 2, "i16")?;
            Ok(Value::Number(i16::from_le_bytes(v.try_into().unwrap()).into()))
        }
        BorshType::I32 => {
            let v = read_bytes(data, offset, 4, "i32")?;
            Ok(Value::Number(i32::from_le_bytes(v.try_into().unwrap()).into()))
        }
        BorshType::I64 => {
            let v = read_bytes(data, offset, 8, "i64")?;
            Ok(Value::Number(i64::from_le_bytes(v.try_into().unwrap()).into()))
        }
        BorshType::I128 => {
            let v = read_bytes(data, offset, 16, "i128")?;
            let n = i128::from_le_bytes(v.try_into().unwrap());
            Ok(Value::String(n.to_string()))
        }
        BorshType::Bool => {
            let v = read_bytes(data, offset, 1, "bool")?;
            match v[0] {
                0 => Ok(Value::Bool(false)),
                1 => Ok(Value::Bool(true)),
                other => bail!("invalid bool value {other} at byte offset {}", *offset - 1),
            }
        }
        BorshType::String => {
            let len_bytes = read_bytes(data, offset, 4, "string length")?;
            let len = u32::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
            let str_bytes = read_bytes(data, offset, len, "string data")?;
            let s = std::str::from_utf8(str_bytes)
                .context(format!("invalid UTF-8 in string at byte offset {}", *offset - len))?;
            Ok(Value::String(s.to_string()))
        }
        BorshType::Pubkey => {
            let v = read_bytes(data, offset, 32, "pubkey")?;
            let key = Pubkey::new_from_array(v.try_into().unwrap());
            Ok(Value::String(key.to_string()))
        }
        BorshType::Vec(inner) => {
            let len_bytes = read_bytes(data, offset, 4, "vec length")?;
            let len = u32::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
            let mut items = Vec::with_capacity(len);
            for i in 0..len {
                let val = decode_borsh(inner, data, offset)
                    .with_context(|| format!("in vec element [{i}]"))?;
                items.push(val);
            }
            Ok(Value::Array(items))
        }
        BorshType::Option(inner) => {
            let tag = read_bytes(data, offset, 1, "option tag")?;
            match tag[0] {
                0 => Ok(Value::Null),
                1 => decode_borsh(inner, data, offset).context("in option value"),
                other => bail!("invalid option tag {other} at byte offset {}", *offset - 1),
            }
        }
        BorshType::Array(inner, n) => {
            let mut items = Vec::with_capacity(*n);
            for i in 0..*n {
                let val = decode_borsh(inner, data, offset)
                    .with_context(|| format!("in array element [{i}]"))?;
                items.push(val);
            }
            Ok(Value::Array(items))
        }
        BorshType::Tuple(types) => {
            let mut items = Vec::with_capacity(types.len());
            for (i, ty) in types.iter().enumerate() {
                let val = decode_borsh(ty, data, offset)
                    .with_context(|| format!("in tuple element [{i}]"))?;
                items.push(val);
            }
            Ok(Value::Array(items))
        }
    }
}

fn read_bytes<'a>(
    data: &'a [u8],
    offset: &mut usize,
    count: usize,
    context: &str,
) -> Result<&'a [u8]> {
    if *offset + count > data.len() {
        bail!(
            "not enough bytes for {context}: need {count} at offset {}, but only {} bytes remain",
            *offset,
            data.len() - *offset
        );
    }
    let slice = &data[*offset..*offset + count];
    *offset += count;
    Ok(slice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn decode(ty: &str, hex: &str) -> Value {
        let ty = super::super::borsh_type::parse_borsh_type(ty).unwrap();
        let data = hex::decode(hex).unwrap();
        let mut offset = 0;
        let val = decode_borsh(&ty, &data, &mut offset).unwrap();
        assert_eq!(offset, data.len(), "not all bytes consumed");
        val
    }

    #[test]
    fn decode_u8() {
        assert_eq!(decode("u8", "2a"), json!(42));
    }

    #[test]
    fn decode_u16() {
        assert_eq!(decode("u16", "e803"), json!(1000));
    }

    #[test]
    fn decode_u32() {
        assert_eq!(decode("u32", "01000000"), json!(1));
    }

    #[test]
    fn decode_u64() {
        assert_eq!(decode("u64", "0100000000000000"), json!(1));
    }

    #[test]
    fn decode_u128() {
        assert_eq!(decode("u128", "01000000000000000000000000000000"), json!("1"));
    }

    #[test]
    fn decode_i8() {
        assert_eq!(decode("i8", "ff"), json!(-1));
    }

    #[test]
    fn decode_i64() {
        assert_eq!(decode("i64", "ffffffffffffffff"), json!(-1));
    }

    #[test]
    fn decode_i128() {
        assert_eq!(
            decode("i128", "ffffffffffffffffffffffffffffffff"),
            json!("-1")
        );
    }

    #[test]
    fn decode_bool_true() {
        assert_eq!(decode("bool", "01"), json!(true));
    }

    #[test]
    fn decode_bool_false() {
        assert_eq!(decode("bool", "00"), json!(false));
    }

    #[test]
    fn decode_string() {
        // "hello" = len(5) + b"hello"
        assert_eq!(decode("string", "0500000068656c6c6f"), json!("hello"));
    }

    #[test]
    fn decode_pubkey() {
        let zeros = "0".repeat(64);
        let val = decode("pubkey", &zeros);
        assert_eq!(val, json!("11111111111111111111111111111111"));
    }

    #[test]
    fn decode_vec_u32() {
        // vec of 2 u32s: [1, 2]
        assert_eq!(
            decode("vec<u32>", "020000000100000002000000"),
            json!([1, 2])
        );
    }

    #[test]
    fn decode_option_some() {
        assert_eq!(decode("option<u64>", "010100000000000000"), json!(1));
    }

    #[test]
    fn decode_option_none() {
        assert_eq!(decode("option<u64>", "00"), json!(null));
    }

    #[test]
    fn decode_array() {
        assert_eq!(decode("[u8;4]", "01020304"), json!([1, 2, 3, 4]));
    }

    #[test]
    fn decode_tuple() {
        assert_eq!(
            decode("(u64,bool)", "010000000000000001"),
            json!([1, true])
        );
    }

    #[test]
    fn decode_insufficient_bytes() {
        let ty = super::super::borsh_type::parse_borsh_type("u64").unwrap();
        let data = hex::decode("01000000").unwrap();
        let mut offset = 0;
        assert!(decode_borsh(&ty, &data, &mut offset).is_err());
    }

    #[test]
    fn decode_invalid_bool() {
        let ty = super::super::borsh_type::parse_borsh_type("bool").unwrap();
        let data = vec![0x02];
        let mut offset = 0;
        assert!(decode_borsh(&ty, &data, &mut offset).is_err());
    }
}
