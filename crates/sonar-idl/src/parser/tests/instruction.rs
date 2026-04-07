use serde_json::{Value, json};

use crate::discriminator::sighash;
use crate::models::Idl;

use super::super::{IndexedIdl, find_instruction_by_discriminator, parse_instruction};
use super::hello_anchor_idl;

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
fn indexed_idl_parses_instruction_matches_discriminator_and_reads_u64_arg() {
    let indexed = IndexedIdl::new(hello_anchor_idl());

    let mut data = vec![175, 175, 109, 31, 13, 152, 155, 237];
    data.extend_from_slice(&42u64.to_le_bytes());

    let result = indexed.parse_instruction(&data).unwrap();
    let parsed = result.expect("should match");

    assert_eq!(parsed.name, "initialize");
    assert_eq!(parsed.fields.len(), 1);
    assert_eq!(parsed.fields[0].name, "data");
    assert_eq!(parsed.fields[0].value, json!(42u64));
}

#[test]
fn indexed_idl_normalizes_current_format_instruction_discriminator() {
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

    let indexed = IndexedIdl::new(idl);
    let data = sighash("global", "do_something").to_vec();

    let result = indexed.parse_instruction(&data).unwrap();
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
    let string = b"hello";
    data.extend_from_slice(&(string.len() as u32).to_le_bytes());
    data.extend_from_slice(string);

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
