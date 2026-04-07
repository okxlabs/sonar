use super::*;
use crate::discriminator::sighash;
use serde_json::{Value, json};
use solana_pubkey::Pubkey;
use std::str::FromStr;

fn hello_anchor_idl() -> Idl {
    serde_json::from_str(
        r#"{
            "address": "BYFW1vhC1ohxwRbYoLbAWs86STa25i9sD5uEusVjTYNd",
            "metadata": { "name": "hello_anchor", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "initialize",
                "discriminator": [175, 175, 109, 31, 13, 152, 155, 237],
                "accounts": [
                    { "name": "new_account", "writable": true, "signer": true },
                    { "name": "signer", "writable": true, "signer": true },
                    { "name": "system_program", "address": "11111111111111111111111111111111" }
                ],
                "args": [{ "name": "data", "type": "u64" }]
            }],
            "types": [{
                "name": "NewAccount",
                "type": { "kind": "struct", "fields": [{ "name": "data", "type": "u64" }] }
            }]
        }"#,
    )
    .unwrap()
}

#[test]
fn parse_instruction_matches_discriminator_and_reads_u64_arg() {
    let idl = hello_anchor_idl();

    let mut data = vec![175, 175, 109, 31, 13, 152, 155, 237];
    data.extend_from_slice(&42u64.to_le_bytes());

    let result = parse_instruction(&idl, &data).unwrap();
    let parsed = result.expect("should match");

    assert_eq!(parsed.name, "initialize");
    assert_eq!(parsed.fields.len(), 1);
    assert_eq!(parsed.fields[0].name, "data");
    assert_eq!(parsed.fields[0].value, json!(42u64));
}

#[test]
fn resolved_idl_parses_instruction_matches_discriminator_and_reads_u64_arg() {
    let resolved = ResolvedIdl::new(hello_anchor_idl());

    let mut data = vec![175, 175, 109, 31, 13, 152, 155, 237];
    data.extend_from_slice(&42u64.to_le_bytes());

    let result = resolved.parse_instruction(&data).unwrap();
    let parsed = result.expect("should match");

    assert_eq!(parsed.name, "initialize");
    assert_eq!(parsed.fields.len(), 1);
    assert_eq!(parsed.fields[0].name, "data");
    assert_eq!(parsed.fields[0].value, json!(42u64));
}

#[test]
fn resolved_idl_normalizes_current_format_instruction_discriminator() {
    let idl: Idl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "current_program", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "doSomething",
                "accounts": [],
                "args": []
            }]
        }"#,
    )
    .unwrap();

    let resolved = ResolvedIdl::new(idl);
    let data = sighash("global", "do_something").to_vec();

    let result = resolved.parse_instruction(&data).unwrap();
    let parsed = result.expect("should match");

    assert_eq!(parsed.name, "doSomething");
    assert!(parsed.fields.is_empty());
}

#[test]
fn parse_instruction_returns_none_for_unknown_discriminator() {
    let idl = hello_anchor_idl();
    let data = vec![0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];

    let result = parse_instruction(&idl, &data).unwrap();
    assert!(result.is_none());
}

#[test]
fn parse_account_data_matches_struct_by_discriminator() {
    let idl = hello_anchor_idl();

    let disc = sighash("account", "NewAccount");
    let mut data = disc.to_vec();
    data.extend_from_slice(&99u64.to_le_bytes());

    let result = parse_account_data(&idl, &data).unwrap();
    let (type_name, value) = result.expect("should match NewAccount");

    assert_eq!(type_name, "NewAccount");
    assert_eq!(value, json!({ "data": 99u64 }));
}

#[test]
fn resolved_idl_parse_account_data_matches_struct_by_discriminator() {
    let resolved = ResolvedIdl::new(hello_anchor_idl());

    let disc = sighash("account", "NewAccount");
    let mut data = disc.to_vec();
    data.extend_from_slice(&99u64.to_le_bytes());

    let result = resolved.parse_account_data(&data).unwrap();
    let (type_name, value) = result.expect("should match NewAccount");

    assert_eq!(type_name, "NewAccount");
    assert_eq!(value, json!({ "data": 99u64 }));
}

#[test]
fn parse_account_data_returns_none_for_unknown_discriminator() {
    let idl = hello_anchor_idl();
    let data = [0u8; 16];

    let result = parse_account_data(&idl, &data).unwrap();
    assert!(result.is_none());
}

#[test]
fn parse_account_data_rejects_short_data() {
    let idl = hello_anchor_idl();

    let result = parse_account_data(&idl, &[0u8; 4]);
    assert!(result.is_err());
}

#[test]
fn is_cpi_event_data_detects_emit_cpi() {
    let mut data = vec![0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
    data.extend_from_slice(&[0; 8]);
    assert!(is_cpi_event_data(&data));
}

#[test]
fn is_cpi_event_data_rejects_short() {
    assert!(!is_cpi_event_data(&[0xe4, 0x45, 0xa5]));
}

#[test]
fn is_cpi_event_data_rejects_wrong_prefix() {
    assert!(!is_cpi_event_data(&[0; 16]));
}

#[test]
fn parse_cpi_event_data_returns_none_for_unknown_event_discriminator() {
    let idl = hello_anchor_idl();
    let mut data = vec![0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
    data.extend_from_slice(&[0; 8]);

    let result = parse_cpi_event_data(&idl, &data).unwrap();
    assert!(result.is_none());
}

#[test]
fn parse_cpi_event_data_parses_event_fields() {
    let event_disc = sighash("event", "TransferDone");

    let idl: Idl = serde_json::from_str(&format!(
        r#"{{
            "address": "BYFW1vhC1ohxwRbYoLbAWs86STa25i9sD5uEusVjTYNd",
            "metadata": {{ "name": "ev", "version": "0.1.0", "spec": "0.1.0" }},
            "instructions": [],
            "events": [{{
                "name": "TransferDone",
                "discriminator": {:?},
                "fields": [{{ "name": "amount", "type": "u64" }}]
            }}]
        }}"#,
        event_disc.to_vec()
    ))
    .unwrap();

    let mut data = vec![0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
    data.extend_from_slice(&event_disc);
    data.extend_from_slice(&500u64.to_le_bytes());

    let result = parse_cpi_event_data(&idl, &data).unwrap();
    let parsed = result.expect("should parse event");

    assert_eq!(parsed.name, "TransferDone");
    assert_eq!(parsed.fields.len(), 1);
    assert_eq!(parsed.fields[0].name, "amount");
    assert_eq!(parsed.fields[0].value, json!(500u64));
}

#[test]
fn parse_cpi_event_data_parses_tuple_event_fields() {
    let event_disc = sighash("event", "PairEvent");

    let idl: Idl = serde_json::from_str(&format!(
        r#"{{
            "address": "11111111111111111111111111111111",
            "metadata": {{ "name": "ev", "version": "0.1.0", "spec": "0.1.0" }},
            "instructions": [],
            "events": [{{
                "name": "PairEvent",
                "discriminator": {:?},
                "fields": ["u32", {{"option":"u16"}}]
            }}]
        }}"#,
        event_disc.to_vec()
    ))
    .unwrap();

    let mut data = vec![0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
    data.extend_from_slice(&event_disc);
    data.extend_from_slice(&9u32.to_le_bytes());
    data.push(1);
    data.extend_from_slice(&7u16.to_le_bytes());

    let result = parse_cpi_event_data(&idl, &data).unwrap();
    let parsed = result.expect("should parse tuple event");

    assert_eq!(parsed.name, "PairEvent");
    assert_eq!(parsed.fields.len(), 2);
    assert_eq!(parsed.fields[0].name, "field_0");
    assert_eq!(parsed.fields[0].value, json!(9u64));
    assert_eq!(parsed.fields[1].name, "field_1");
    assert_eq!(parsed.fields[1].value, json!(7u64));
}

#[test]
fn parse_instruction_multiple_primitive_args() {
    let idl: Idl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "multi",
                "discriminator": [1,2,3,4,5,6,7,8],
                "accounts": [],
                "args": [
                    { "name": "a", "type": "u8" },
                    { "name": "b", "type": "bool" },
                    { "name": "c", "type": "i16" },
                    { "name": "d", "type": "string" }
                ]
            }],
            "types": []
        }"#,
    )
    .unwrap();

    let mut data = vec![1, 2, 3, 4, 5, 6, 7, 8];
    data.push(42);
    data.push(1);
    data.extend_from_slice(&(-5i16).to_le_bytes());
    let s = b"hello";
    data.extend_from_slice(&(s.len() as u32).to_le_bytes());
    data.extend_from_slice(s);

    let parsed = parse_instruction(&idl, &data).unwrap().unwrap();
    assert_eq!(parsed.fields[0].value, json!(42u64));
    assert_eq!(parsed.fields[1].value, json!(true));
    assert_eq!(parsed.fields[2].value, json!(-5i64));
    assert_eq!(parsed.fields[3].value, json!("hello"));
}

#[test]
fn parse_instruction_errors_when_a_required_arg_is_missing() {
    let idl: Idl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "pair",
                "discriminator": [4,5,6,7,8,9,10,11],
                "accounts": [],
                "args": [
                    { "name": "left", "type": "u32" },
                    { "name": "right", "type": "u32" }
                ]
            }],
            "types": []
        }"#,
    )
    .unwrap();

    let mut data = vec![4, 5, 6, 7, 8, 9, 10, 11];
    data.extend_from_slice(&123u32.to_le_bytes());

    let err = parse_instruction(&idl, &data).unwrap_err();
    assert!(err.to_string().contains("Insufficient data"));
}

#[test]
fn parse_instruction_with_defined_struct_arg() {
    let idl: Idl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "create",
                "discriminator": [10,20,30,40,50,60,70,80],
                "accounts": [],
                "args": [{ "name": "params", "type": { "defined": "Params" } }]
            }],
            "types": [{
                "name": "Params",
                "type": {
                    "kind": "struct",
                    "fields": [
                        { "name": "x", "type": "u32" },
                        { "name": "y", "type": "u32" }
                    ]
                }
            }]
        }"#,
    )
    .unwrap();

    let mut data = vec![10, 20, 30, 40, 50, 60, 70, 80];
    data.extend_from_slice(&100u32.to_le_bytes());
    data.extend_from_slice(&200u32.to_le_bytes());

    let parsed = parse_instruction(&idl, &data).unwrap().unwrap();
    assert_eq!(parsed.fields[0].name, "params");
    assert_eq!(parsed.fields[0].value, json!({ "x": 100u64, "y": 200u64 }));
}

#[test]
fn parse_instruction_errors_when_a_defined_struct_field_is_missing() {
    let idl: Idl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "create",
                "discriminator": [10,20,30,40,50,60,70,80],
                "accounts": [],
                "args": [{ "name": "params", "type": { "defined": "Params" } }]
            }],
            "types": [{
                "name": "Params",
                "type": {
                    "kind": "struct",
                    "fields": [
                        { "name": "x", "type": "u32" },
                        { "name": "y", "type": "u32" }
                    ]
                }
            }]
        }"#,
    )
    .unwrap();

    let mut data = vec![10, 20, 30, 40, 50, 60, 70, 80];
    data.extend_from_slice(&100u32.to_le_bytes());

    let err = parse_instruction(&idl, &data).unwrap_err();
    assert!(err.to_string().contains("Insufficient data"));
}

#[test]
fn parse_instruction_preserves_named_field_order_in_serde_json_value() {
    let idl: Idl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "create",
                "discriminator": [10,20,30,40,50,60,70,80],
                "accounts": [],
                "args": [{ "name": "params", "type": { "defined": "Params" } }]
            }],
            "types": [{
                "name": "Params",
                "type": {
                    "kind": "struct",
                    "fields": [
                        { "name": "zeta", "type": "u32" },
                        { "name": "alpha", "type": "u32" }
                    ]
                }
            }]
        }"#,
    )
    .unwrap();

    let mut data = vec![10, 20, 30, 40, 50, 60, 70, 80];
    data.extend_from_slice(&100u32.to_le_bytes());
    data.extend_from_slice(&200u32.to_le_bytes());

    let parsed = parse_instruction(&idl, &data).unwrap().unwrap();
    let json = serde_json::to_value(&parsed.fields[0].value).unwrap();

    assert_eq!(serde_json::to_string(&json).unwrap(), r#"{"zeta":100,"alpha":200}"#);
}

#[test]
fn parse_instruction_with_enum_arg() {
    let idl: Idl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "act",
                "discriminator": [1,1,1,1,1,1,1,1],
                "accounts": [],
                "args": [{ "name": "action", "type": { "defined": "Action" } }]
            }],
            "types": [{
                "name": "Action",
                "type": {
                    "kind": "enum",
                    "variants": [
                        { "name": "Start" },
                        { "name": "Stop" }
                    ]
                }
            }]
        }"#,
    )
    .unwrap();

    let mut data = vec![1, 1, 1, 1, 1, 1, 1, 1, 0];
    let parsed = parse_instruction(&idl, &data).unwrap().unwrap();
    assert_eq!(parsed.fields[0].value, json!({ "Start": null }));

    data[8] = 1;
    let parsed = parse_instruction(&idl, &data).unwrap().unwrap();
    assert_eq!(parsed.fields[0].value, json!({ "Stop": null }));
}

#[test]
fn parse_instruction_with_vec_arg() {
    let idl: Idl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "bulk",
                "discriminator": [2,2,2,2,2,2,2,2],
                "accounts": [],
                "args": [{ "name": "vals", "type": { "vec": "u16" } }]
            }],
            "types": []
        }"#,
    )
    .unwrap();

    let mut data = vec![2, 2, 2, 2, 2, 2, 2, 2];
    data.extend_from_slice(&3u32.to_le_bytes());
    data.extend_from_slice(&10u16.to_le_bytes());
    data.extend_from_slice(&20u16.to_le_bytes());
    data.extend_from_slice(&30u16.to_le_bytes());

    let parsed = parse_instruction(&idl, &data).unwrap().unwrap();
    assert_eq!(parsed.fields[0].value, json!([10u64, 20u64, 30u64]));
}

#[test]
fn parse_instruction_with_option_arg() {
    let idl: Idl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "opt",
                "discriminator": [3,3,3,3,3,3,3,3],
                "accounts": [],
                "args": [{ "name": "maybe", "type": { "option": "u32" } }]
            }],
            "types": []
        }"#,
    )
    .unwrap();

    let mut data = vec![3, 3, 3, 3, 3, 3, 3, 3, 1];
    data.extend_from_slice(&777u32.to_le_bytes());
    let parsed = parse_instruction(&idl, &data).unwrap().unwrap();
    assert_eq!(parsed.fields[0].value, json!(777u64));

    let data_none = vec![3, 3, 3, 3, 3, 3, 3, 3, 0];
    let parsed = parse_instruction(&idl, &data_none).unwrap().unwrap();
    assert_eq!(parsed.fields[0].value, Value::Null);
}

#[test]
fn find_instruction_prefers_longest_discriminator() {
    let idl: Idl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [
                { "name": "fallback", "accounts": [], "args": [] },
                { "name": "specific", "discriminator": [1], "accounts": [], "args": [] }
            ]
        }"#,
    )
    .unwrap();

    let data = vec![1, 0, 0, 0];
    let found = find_instruction_by_discriminator(&idl, &data).unwrap();
    assert_eq!(found.name, "specific");
}

#[test]
fn find_instruction_ignores_missing_discriminator() {
    let idl: Idl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [
                { "name": "missing", "accounts": [], "args": [] }
            ]
        }"#,
    )
    .unwrap();

    let found = find_instruction_by_discriminator(&idl, &[1, 2, 3, 4]);
    assert!(found.is_none());
}

#[test]
fn parse_simple_u8() {
    let data = [42u8];
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "u8").unwrap();
    assert_eq!(val, json!(42u64));
    assert_eq!(offset, 1);
}

#[test]
fn parse_simple_i8() {
    let data = [(-5i8) as u8];
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "i8").unwrap();
    assert_eq!(val, json!(-5i64));
    assert_eq!(offset, 1);
}

#[test]
fn parse_simple_u16() {
    let data = 1000u16.to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "u16").unwrap();
    assert_eq!(val, json!(1000u64));
    assert_eq!(offset, 2);
}

#[test]
fn parse_simple_i16() {
    let data = (-300i16).to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "i16").unwrap();
    assert_eq!(val, json!(-300i64));
    assert_eq!(offset, 2);
}

#[test]
fn parse_simple_u32() {
    let data = 70000u32.to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "u32").unwrap();
    assert_eq!(val, json!(70000u64));
    assert_eq!(offset, 4);
}

#[test]
fn parse_simple_i32() {
    let data = (-70000i32).to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "i32").unwrap();
    assert_eq!(val, json!(-70000i64));
    assert_eq!(offset, 4);
}

#[test]
fn parse_simple_u64() {
    let data = u64::MAX.to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "u64").unwrap();
    assert_eq!(val, json!(u64::MAX));
    assert_eq!(offset, 8);
}

#[test]
fn parse_simple_i64() {
    let data = i64::MIN.to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "i64").unwrap();
    assert_eq!(val, json!(i64::MIN));
    assert_eq!(offset, 8);
}

#[test]
fn parse_simple_u128() {
    let val_in: u128 = 340_282_366_920_938_463;
    let data = val_in.to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "u128").unwrap();
    assert_eq!(val, json!(val_in.to_string()));
    assert_eq!(offset, 16);
}

#[test]
fn parse_simple_i128() {
    let val_in: i128 = -170_141_183_460_469;
    let data = val_in.to_le_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "i128").unwrap();
    assert_eq!(val, json!(val_in.to_string()));
    assert_eq!(offset, 16);
}

#[test]
fn parse_simple_bool_true() {
    let data = [1u8];
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "bool").unwrap();
    assert_eq!(val, json!(true));
    assert_eq!(offset, 1);
}

#[test]
fn parse_simple_bool_false() {
    let data = [0u8];
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "bool").unwrap();
    assert_eq!(val, json!(false));
    assert_eq!(offset, 1);
}

#[test]
fn parse_simple_pubkey() {
    let pk = Pubkey::from_str("11111111111111111111111111111111").unwrap();
    let data = pk.to_bytes();
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "pubkey").unwrap();
    assert_eq!(val, json!(pk.to_string()));
    assert_eq!(offset, 32);
}

#[test]
fn parse_simple_string() {
    let s = b"hello";
    let mut data = (s.len() as u32).to_le_bytes().to_vec();
    data.extend_from_slice(s);
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "string").unwrap();
    assert_eq!(val, json!("hello"));
    assert_eq!(offset, 9);
}

#[test]
fn parse_simple_bytes() {
    let payload = vec![0xAA, 0xBB, 0xCC];
    let mut data = (payload.len() as u32).to_le_bytes().to_vec();
    data.extend_from_slice(&payload);
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "bytes").unwrap();
    assert_eq!(val, json!([0xAAu64, 0xBBu64, 0xCCu64]));
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
    if let Value::Object(entries) = &val {
        let keys: Vec<&str> = entries.keys().map(|k| k.as_str()).collect();
        assert!(keys.contains(&"context"));
        assert!(keys.contains(&"type_hint"));
        assert!(keys.contains(&"raw_hex"));
    } else {
        panic!("expected Object for unknown type, got {:?}", val);
    }
}

#[test]
fn parse_simple_type_unknown_preserves_fallback_key_order_in_serde_json_value() {
    let data = [1, 2, 3, 4];
    let mut offset = 0;
    let val = parse_simple_type(&data, &mut offset, "unknown_type").unwrap();
    let json = serde_json::to_value(&val).unwrap();

    assert_eq!(
        serde_json::to_string(&json).unwrap(),
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
    let idl = hello_anchor_idl();
    let element_type = IdlType::Simple("u32".into());
    let val = parse_vec_type(&data, &mut offset, &element_type, &idl).unwrap();
    assert_eq!(val, json!([10u64, 20u64, 30u64]));
    assert_eq!(offset, 16);
}

#[test]
fn parse_vec_type_empty() {
    let data = 0u32.to_le_bytes();
    let mut offset = 0;
    let idl = hello_anchor_idl();
    let element_type = IdlType::Simple("u8".into());
    let val = parse_vec_type(&data, &mut offset, &element_type, &idl).unwrap();
    assert_eq!(val, json!([]));
    assert_eq!(offset, 4);
}

#[test]
fn parse_vec_type_truncated_length() {
    let data = [0u8; 2];
    let mut offset = 0;
    let idl = hello_anchor_idl();
    let element_type = IdlType::Simple("u8".into());
    let err = parse_vec_type(&data, &mut offset, &element_type, &idl).unwrap_err();
    assert!(err.to_string().contains("Insufficient data"));
}

#[test]
fn parse_vec_type_errors_when_declared_elements_are_missing() {
    let mut data = 2u32.to_le_bytes().to_vec();
    data.extend_from_slice(&10u32.to_le_bytes());
    let mut offset = 0;
    let idl = hello_anchor_idl();
    let element_type = IdlType::Simple("u32".into());

    let err = parse_vec_type(&data, &mut offset, &element_type, &idl).unwrap_err();
    assert!(err.to_string().contains("Insufficient data"));
}

#[test]
fn parse_vec_type_stops_when_element_parser_makes_no_progress() {
    let data = 3u32.to_le_bytes();
    let mut offset = 0;
    let idl = hello_anchor_idl();
    let element_type = IdlType::Defined { defined: DefinedType::Simple("MissingType".into()) };

    let val = parse_vec_type(&data, &mut offset, &element_type, &idl).unwrap();

    assert_eq!(val, json!([]));
    assert_eq!(offset, 4);
}

#[test]
fn parse_option_type_some() {
    let mut data = vec![1u8];
    data.extend_from_slice(&500u16.to_le_bytes());
    let mut offset = 0;
    let idl = hello_anchor_idl();
    let inner = IdlType::Simple("u16".into());
    let val = parse_option_type(&data, &mut offset, &inner, &idl).unwrap();
    assert_eq!(val, json!(500u64));
    assert_eq!(offset, 3);
}

#[test]
fn parse_option_type_none() {
    let data = vec![0u8];
    let mut offset = 0;
    let idl = hello_anchor_idl();
    let inner = IdlType::Simple("u16".into());
    let val = parse_option_type(&data, &mut offset, &inner, &idl).unwrap();
    assert_eq!(val, Value::Null);
    assert_eq!(offset, 1);
}

#[test]
fn parse_option_type_truncated_discriminant() {
    let data: [u8; 0] = [];
    let mut offset = 0;
    let idl = hello_anchor_idl();
    let inner = IdlType::Simple("u8".into());
    let err = parse_option_type(&data, &mut offset, &inner, &idl).unwrap_err();
    assert!(err.to_string().contains("Insufficient data"));
}

#[test]
fn parse_array_type_fixed_3_u8() {
    let data = vec![10, 20, 30];
    let mut offset = 0;
    let idl = hello_anchor_idl();
    let array_def =
        IdlArrayType { element_type: Box::new(IdlType::Simple("u8".into())), length: 3 };
    let val = parse_array_type(&data, &mut offset, &array_def, &idl).unwrap();
    assert_eq!(val, json!([10u64, 20u64, 30u64]));
    assert_eq!(offset, 3);
}

#[test]
fn parse_instruction_with_defined_tuple_struct_arg_supports_nested_types() {
    let idl: Idl = serde_json::from_str(
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

    let parsed = parse_instruction(&idl, &data).unwrap().unwrap();

    assert_eq!(parsed.fields.len(), 1);
    assert_eq!(parsed.fields[0].name, "payload");
    assert_eq!(parsed.fields[0].value, json!([777u64, { "amount": 42u64 }]));
}

#[test]
fn parse_enum_with_struct_variant() {
    let idl: Idl = serde_json::from_str(
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

    let parsed = parse_instruction(&idl, &data).unwrap().unwrap();
    let val = &parsed.fields[0].value;
    assert_eq!(*val, json!({ "Transfer": { "amount": 5000u64, "fee": 100u64 } }));
}

#[test]
fn parse_enum_with_tuple_variant() {
    let idl: Idl = serde_json::from_str(
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

    let parsed = parse_instruction(&idl, &data).unwrap().unwrap();
    let val = &parsed.fields[0].value;
    assert_eq!(*val, json!({ "SetPair": [111u64, 222u64] }));
}

#[test]
fn parse_enum_out_of_range_variant_index_falls_through() {
    let idl: Idl = serde_json::from_str(
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

    let parsed = parse_instruction(&idl, &data).unwrap().unwrap();
    if let Value::Object(entries) = &parsed.fields[0].value {
        let keys: Vec<&str> = entries.keys().map(|k| k.as_str()).collect();
        assert!(keys.contains(&"raw_hex"));
    } else {
        panic!("expected raw fallback for out-of-range variant");
    }
}
