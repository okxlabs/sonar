use std::str::FromStr;

use solana_pubkey::Pubkey;

use crate::decode::{parse_array_type, parse_option_type, parse_simple_type, parse_vec_type};
use crate::idl::{DefinedType, IdlArrayType, IdlType};
use crate::indexed::IndexedIdl;
use crate::value::IdlValue;

use super::hello_anchor_indexed_idl;

#[test]
fn parse_simple_u8() {
    let data = [42u8];
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "u8").unwrap();
    assert_eq!(val, IdlValue::Uint(42));
    assert_eq!(offset, 1);
}

#[test]
fn parse_simple_i8() {
    let data = [(-5i8) as u8];
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "i8").unwrap();
    assert_eq!(val, IdlValue::Int(-5));
    assert_eq!(offset, 1);
}

#[test]
fn parse_simple_u16() {
    let data = 1000u16.to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "u16").unwrap();
    assert_eq!(val, IdlValue::Uint(1000));
    assert_eq!(offset, 2);
}

#[test]
fn parse_simple_i16() {
    let data = (-300i16).to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "i16").unwrap();
    assert_eq!(val, IdlValue::Int(-300));
    assert_eq!(offset, 2);
}

#[test]
fn parse_simple_u32() {
    let data = 70000u32.to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "u32").unwrap();
    assert_eq!(val, IdlValue::Uint(70000));
    assert_eq!(offset, 4);
}

#[test]
fn parse_simple_i32() {
    let data = (-70000i32).to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "i32").unwrap();
    assert_eq!(val, IdlValue::Int(-70000));
    assert_eq!(offset, 4);
}

#[test]
fn parse_simple_u64() {
    let data = u64::MAX.to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "u64").unwrap();
    assert_eq!(val, IdlValue::Uint(u64::MAX as u128));
    assert_eq!(offset, 8);
}

#[test]
fn parse_simple_i64() {
    let data = i64::MIN.to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "i64").unwrap();
    assert_eq!(val, IdlValue::Int(i64::MIN as i128));
    assert_eq!(offset, 8);
}

#[test]
fn parse_simple_u128() {
    let value_in: u128 = 340_282_366_920_938_463;
    let data = value_in.to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "u128").unwrap();
    assert_eq!(val, IdlValue::Uint(value_in));
    assert_eq!(offset, 16);
}

#[test]
fn parse_simple_i128() {
    let value_in: i128 = -170_141_183_460_469;
    let data = value_in.to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "i128").unwrap();
    assert_eq!(val, IdlValue::Int(value_in));
    assert_eq!(offset, 16);
}

#[test]
fn parse_simple_bool_true() {
    let data = [1u8];
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "bool").unwrap();
    assert_eq!(val, IdlValue::Bool(true));
    assert_eq!(offset, 1);
}

#[test]
fn parse_simple_bool_false() {
    let data = [0u8];
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "bool").unwrap();
    assert_eq!(val, IdlValue::Bool(false));
    assert_eq!(offset, 1);
}

#[test]
fn parse_simple_pubkey() {
    let pubkey = Pubkey::from_str("11111111111111111111111111111111").unwrap();
    let data = pubkey.to_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "pubkey").unwrap();
    assert_eq!(val, IdlValue::String(pubkey.to_string()));
    assert_eq!(offset, 32);
}

#[test]
fn parse_simple_string() {
    let string = b"hello";
    let mut data = (string.len() as u32).to_le_bytes().to_vec();
    data.extend_from_slice(string);
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "string").unwrap();
    assert_eq!(val, IdlValue::String("hello".into()));
    assert_eq!(offset, 9);
}

#[test]
fn parse_simple_bytes() {
    let payload = vec![0xAA, 0xBB, 0xCC];
    let mut data = (payload.len() as u32).to_le_bytes().to_vec();
    data.extend_from_slice(&payload);
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "bytes").unwrap();
    assert_eq!(val, IdlValue::Bytes(vec![0xAA, 0xBB, 0xCC]));
    assert_eq!(offset, 7);
}

#[test]
fn parse_simple_type_truncated_u32() {
    let data = [0u8; 2];
    let mut offset = 0;
    let err = parse_simple_type(&data, &mut offset, "u32").unwrap_err();
    assert!(err.to_string().contains("Insufficient data"));
}

#[test]
fn parse_simple_type_truncated_string_length() {
    let data = [0u8; 2];
    let mut offset = 0;
    let err = parse_simple_type(&data, &mut offset, "string").unwrap_err();
    assert!(err.to_string().contains("Insufficient data"));
}

#[test]
fn parse_simple_type_truncated_string_body() {
    let mut data = 10u32.to_le_bytes().to_vec();
    data.extend_from_slice(&[0, 0]);
    let mut offset = 0;
    let err = parse_simple_type(&data, &mut offset, "string").unwrap_err();
    assert!(err.to_string().contains("Insufficient data"));
}

#[test]
fn parse_simple_type_unknown_falls_back_to_raw() {
    let data = [1, 2, 3, 4];
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "unknown_type").unwrap();
    if let IdlValue::Struct(entries) = &val {
        let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"context"));
        assert!(keys.contains(&"type_hint"));
        assert!(keys.contains(&"raw_hex"));
    } else {
        panic!("expected Struct for unknown type, got {:?}", val);
    }
}

#[test]
fn parse_simple_type_unknown_preserves_fallback_key_order_in_serde_json_value() {
    let data = [1, 2, 3, 4];
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "unknown_type").unwrap();

    assert_eq!(
        serde_json::to_string(&val).unwrap(),
        r#"{"context":"simple_type","type_hint":"unknown_type","raw_hex":"01020304"}"#
    );
}

#[test]
fn parse_vec_type_u32_elements() {
    let mut data = 3u32.to_le_bytes().to_vec();
    data.extend_from_slice(&10u32.to_le_bytes());
    data.extend_from_slice(&20u32.to_le_bytes());
    data.extend_from_slice(&30u32.to_le_bytes());
    let mut offset = 0;
    let indexed = hello_anchor_indexed_idl();
    let element_type = IdlType::Simple("u32".into());
    let val = parse_vec_type(&data, &mut offset, &element_type, &indexed).unwrap();
    assert_eq!(val, IdlValue::Array(vec![IdlValue::Uint(10), IdlValue::Uint(20), IdlValue::Uint(30)]));
    assert_eq!(offset, 16);
}

#[test]
fn parse_vec_type_empty() {
    let data = 0u32.to_le_bytes();
    let mut offset = 0;
    let indexed = hello_anchor_indexed_idl();
    let element_type = IdlType::Simple("u8".into());
    let val = parse_vec_type(&data, &mut offset, &element_type, &indexed).unwrap();
    assert_eq!(val, IdlValue::Array(vec![]));
    assert_eq!(offset, 4);
}

#[test]
fn parse_vec_type_truncated_length() {
    let data = [0u8; 2];
    let mut offset = 0;
    let indexed = hello_anchor_indexed_idl();
    let element_type = IdlType::Simple("u8".into());
    let err = parse_vec_type(&data, &mut offset, &element_type, &indexed).unwrap_err();
    assert!(err.to_string().contains("Insufficient data"));
}

#[test]
fn parse_vec_type_errors_when_declared_elements_are_missing() {
    let mut data = 2u32.to_le_bytes().to_vec();
    data.extend_from_slice(&10u32.to_le_bytes());
    let mut offset = 0;
    let indexed = hello_anchor_indexed_idl();
    let element_type = IdlType::Simple("u32".into());

    let err = parse_vec_type(&data, &mut offset, &element_type, &indexed).unwrap_err();
    assert!(err.to_string().contains("Insufficient data"));
}

#[test]
fn parse_vec_type_stops_when_element_parser_makes_no_progress() {
    let data = 3u32.to_le_bytes();
    let mut offset = 0;
    let indexed = hello_anchor_indexed_idl();
    let element_type = IdlType::Defined { defined: DefinedType::Simple("MissingType".into()) };

    let val = parse_vec_type(&data, &mut offset, &element_type, &indexed).unwrap();

    assert_eq!(val, IdlValue::Array(vec![]));
    assert_eq!(offset, 4);
}

#[test]
fn parse_option_type_some() {
    let mut data = vec![1u8];
    data.extend_from_slice(&500u16.to_le_bytes());
    let mut offset = 0;
    let indexed = hello_anchor_indexed_idl();
    let inner = IdlType::Simple("u16".into());
    let val = parse_option_type(&data, &mut offset, &inner, &indexed).unwrap();
    assert_eq!(val, IdlValue::Uint(500));
    assert_eq!(offset, 3);
}

#[test]
fn parse_option_type_none() {
    let data = vec![0u8];
    let mut offset = 0;
    let indexed = hello_anchor_indexed_idl();
    let inner = IdlType::Simple("u16".into());
    let val = parse_option_type(&data, &mut offset, &inner, &indexed).unwrap();
    assert_eq!(val, IdlValue::Null);
    assert_eq!(offset, 1);
}

#[test]
fn parse_option_type_truncated_discriminant() {
    let data: [u8; 0] = [];
    let mut offset = 0;
    let indexed = hello_anchor_indexed_idl();
    let inner = IdlType::Simple("u8".into());
    let err = parse_option_type(&data, &mut offset, &inner, &indexed).unwrap_err();
    assert!(err.to_string().contains("Insufficient data"));
}

#[test]
fn parse_array_type_fixed_3_u8() {
    let data = vec![10, 20, 30];
    let mut offset = 0;
    let indexed = hello_anchor_indexed_idl();
    let array_def =
        IdlArrayType { element_type: Box::new(IdlType::Simple("u8".into())), length: 3 };
    let val = parse_array_type(&data, &mut offset, &array_def, &indexed).unwrap();
    assert_eq!(val, IdlValue::Array(vec![IdlValue::Uint(10), IdlValue::Uint(20), IdlValue::Uint(30)]));
    assert_eq!(offset, 3);
}

#[test]
fn parse_instruction_with_defined_tuple_struct_arg_supports_nested_types() {
    let indexed: IndexedIdl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "wrap",
                "discriminator": [6,6,6,6,6,6,6,6],
                "accounts": [],
                "args": [{ "name": "payload", "type": { "defined": "Wrapper" } }]
            }],
            "types": [
                {
                    "name": "Inner",
                    "type": {
                        "kind": "struct",
                        "fields": [{ "name": "amount", "type": "u16" }]
                    }
                },
                {
                    "name": "Wrapper",
                    "type": {
                        "kind": "struct",
                        "fields": [
                            { "option": "u32" },
                            { "defined": "Inner" }
                        ]
                    }
                }
            ]
        }"#,
    )
    .unwrap();

    let mut data = vec![6, 6, 6, 6, 6, 6, 6, 6, 1];
    data.extend_from_slice(&777u32.to_le_bytes());
    data.extend_from_slice(&42u16.to_le_bytes());

    let parsed = indexed.parse_instruction(&data).unwrap().unwrap();

    assert_eq!(parsed.fields.len(), 1);
    assert_eq!(parsed.fields[0].name, "payload");
    assert_eq!(
        parsed.fields[0].value,
        IdlValue::Array(vec![
            IdlValue::Uint(777),
            IdlValue::Struct(vec![("amount".into(), IdlValue::Uint(42))]),
        ])
    );
}

#[test]
fn parse_enum_with_struct_variant() {
    let indexed: IndexedIdl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "cmd",
                "discriminator": [9,9,9,9,9,9,9,9],
                "accounts": [],
                "args": [{ "name": "op", "type": { "defined": "Op" } }]
            }],
            "types": [{
                "name": "Op",
                "type": {
                    "kind": "enum",
                    "variants": [
                        { "name": "Noop" },
                        {
                            "name": "Transfer",
                            "fields": [
                                { "name": "amount", "type": "u64" },
                                { "name": "fee", "type": "u16" }
                            ]
                        }
                    ]
                }
            }]
        }"#,
    )
    .unwrap();

    let mut data = vec![9, 9, 9, 9, 9, 9, 9, 9];
    data.push(1);
    data.extend_from_slice(&5000u64.to_le_bytes());
    data.extend_from_slice(&100u16.to_le_bytes());

    let parsed = indexed.parse_instruction(&data).unwrap().unwrap();
    let val = &parsed.fields[0].value;
    assert_eq!(
        *val,
        IdlValue::Struct(vec![(
            "Transfer".into(),
            IdlValue::Struct(vec![
                ("amount".into(), IdlValue::Uint(5000)),
                ("fee".into(), IdlValue::Uint(100)),
            ]),
        )])
    );
}

#[test]
fn parse_enum_with_tuple_variant() {
    let indexed: IndexedIdl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "cmd",
                "discriminator": [8,8,8,8,8,8,8,8],
                "accounts": [],
                "args": [{ "name": "op", "type": { "defined": "Op" } }]
            }],
            "types": [{
                "name": "Op",
                "type": {
                    "kind": "enum",
                    "variants": [
                        { "name": "Noop" },
                        { "name": "SetPair", "fields": ["u32", "u32"] }
                    ]
                }
            }]
        }"#,
    )
    .unwrap();

    let mut data = vec![8, 8, 8, 8, 8, 8, 8, 8];
    data.push(1);
    data.extend_from_slice(&111u32.to_le_bytes());
    data.extend_from_slice(&222u32.to_le_bytes());

    let parsed = indexed.parse_instruction(&data).unwrap().unwrap();
    let val = &parsed.fields[0].value;
    assert_eq!(
        *val,
        IdlValue::Struct(vec![(
            "SetPair".into(),
            IdlValue::Array(vec![IdlValue::Uint(111), IdlValue::Uint(222)]),
        )])
    );
}

#[test]
fn parse_enum_out_of_range_variant_index_falls_through() {
    let indexed: IndexedIdl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "cmd",
                "discriminator": [7,7,7,7,7,7,7,7],
                "accounts": [],
                "args": [{ "name": "val", "type": { "defined": "Small" } }]
            }],
            "types": [{
                "name": "Small",
                "type": {
                    "kind": "enum",
                    "variants": [{ "name": "Only" }]
                }
            }]
        }"#,
    )
    .unwrap();

    let mut data = vec![7, 7, 7, 7, 7, 7, 7, 7];
    data.push(99);

    let parsed = indexed.parse_instruction(&data).unwrap().unwrap();
    if let IdlValue::Struct(entries) = &parsed.fields[0].value {
        let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"raw_hex"));
    } else {
        panic!("expected raw fallback for out-of-range variant");
    }
}
