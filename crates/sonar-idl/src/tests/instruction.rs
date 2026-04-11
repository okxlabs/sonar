use crate::discriminator::sighash;
use crate::idl::*;
use crate::indexed::{IdlInstructionFields, IndexedIdl};
use crate::value::IdlValue;

use super::hello_anchor_indexed_idl;

#[test]
fn indexed_idl_deserializes_current_json_and_parses_instruction() {
    let indexed: IndexedIdl = serde_json::from_str(
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

    let parsed = indexed
        .parse_instruction(&sighash("global", "do_something"))
        .unwrap()
        .expect("instruction should parse");

    assert_eq!(parsed.name, "doSomething");
}

#[test]
fn indexed_idl_deserializes_legacy_json_and_parses_instruction() {
    let indexed: IndexedIdl = serde_json::from_str(
        r#"{
            "version": "0.1.0",
            "name": "legacy_program",
            "instructions": [{
                "name": "doSomething",
                "accounts": [],
                "args": []
            }]
        }"#,
    )
    .unwrap();

    let parsed = indexed
        .parse_instruction(&sighash("global", "do_something"))
        .unwrap()
        .expect("instruction should parse");

    assert_eq!(parsed.name, "doSomething");
}

#[test]
fn indexed_idl_from_json_with_program_address_preserves_legacy_address_fallback() {
    let indexed = IndexedIdl::from_json_with_program_address(
        r#"{
            "version": "0.1.0",
            "name": "legacy_program",
            "instructions": [{
                "name": "doSomething",
                "accounts": [],
                "args": []
            }]
        }"#,
        "11111111111111111111111111111111",
    )
    .unwrap();

    assert_eq!(indexed.address_for_tests(), "11111111111111111111111111111111");
}

#[test]
fn indexed_idl_parse_instruction_exposes_flat_account_names() {
    let indexed = IndexedIdl::from_normalized_idl(Idl {
        address: "11111111111111111111111111111111".to_string(),
        metadata: IdlMetadata {
            name: "current_program".to_string(),
            version: "0.1.0".to_string(),
            spec: "0.1.0".to_string(),
            description: None,
        },
        instructions: vec![IdlInstruction {
            name: "initialize".to_string(),
            discriminator: Some(sighash("global", "initialize").to_vec()),
            accounts: vec![
                IdlAccountItem::Account(IdlAccount {
                    name: "payer".to_string(),
                    writable: true,
                    signer: true,
                    optional: false,
                    address: None,
                }),
                IdlAccountItem::Accounts(IdlAccounts {
                    name: "authority_group".to_string(),
                    accounts: vec![
                        IdlAccountItem::Account(IdlAccount {
                            name: "authority".to_string(),
                            writable: false,
                            signer: true,
                            optional: false,
                            address: None,
                        }),
                        IdlAccountItem::Account(IdlAccount {
                            name: "vault".to_string(),
                            writable: true,
                            signer: false,
                            optional: false,
                            address: None,
                        }),
                    ],
                }),
            ],
            args: vec![],
        }],
        types: None,
        events: None,
    });

    let parsed = indexed
        .parse_instruction(&sighash("global", "initialize"))
        .unwrap()
        .expect("instruction should parse");

    assert_eq!(parsed.account_names, vec!["payer", "authority_group: []"]);
}

#[test]
fn indexed_idl_parse_instruction_matches_discriminator_and_reads_u64_arg() {
    let indexed = hello_anchor_indexed_idl();

    let mut data = vec![175, 175, 109, 31, 13, 152, 155, 237];
    data.extend_from_slice(&42u64.to_le_bytes());

    let result = indexed.parse_instruction(&data).unwrap();
    let parsed = result.expect("should match");

    assert_eq!(parsed.name, "initialize");
    assert_eq!(parsed.fields.len(), 1);
    assert_eq!(parsed.fields[0].name, "data");
    assert_eq!(parsed.fields[0].value, IdlValue::Uint(42));
}

#[test]
fn indexed_idl_normalizes_current_format_instruction_discriminator() {
    let indexed: IndexedIdl = serde_json::from_str(
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
    let data = sighash("global", "do_something").to_vec();

    let result = indexed.parse_instruction(&data).unwrap();
    let parsed = result.expect("should match");

    assert_eq!(parsed.name, "doSomething");
    assert!(parsed.fields.is_empty());
}

#[test]
fn indexed_idl_parse_instruction_returns_none_for_unknown_discriminator() {
    let indexed = hello_anchor_indexed_idl();
    let data = vec![0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];

    let result = indexed.parse_instruction(&data).unwrap();
    assert!(result.is_none());
}

#[test]
fn indexed_idl_parse_instruction_multiple_primitive_args() {
    let indexed: IndexedIdl = serde_json::from_str(
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

    let parsed = indexed.parse_instruction(&data).unwrap().unwrap();
    assert_eq!(parsed.fields[0].value, IdlValue::Uint(42));
    assert_eq!(parsed.fields[1].value, IdlValue::Bool(true));
    assert_eq!(parsed.fields[2].value, IdlValue::Int(-5));
    assert_eq!(parsed.fields[3].value, IdlValue::String("hello".into()));
}

#[test]
fn indexed_idl_parse_instruction_marks_fields_unparsed_when_a_required_arg_is_missing() {
    let indexed: IndexedIdl = serde_json::from_str(
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

    let parsed = indexed.parse_instruction(&data).unwrap().unwrap();
    assert_eq!(parsed.name, "pair");
    assert_eq!(parsed.account_names, Vec::<String>::new());
    assert!(matches!(
        parsed.fields,
        IdlInstructionFields::Unparsed(raw_args_hex) if raw_args_hex == "7b000000"
    ));
}

#[test]
fn indexed_idl_parse_instruction_with_defined_struct_arg() {
    let indexed: IndexedIdl = serde_json::from_str(
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

    let parsed = indexed.parse_instruction(&data).unwrap().unwrap();
    assert_eq!(parsed.fields[0].name, "params");
    assert_eq!(
        parsed.fields[0].value,
        IdlValue::Struct(vec![
            ("x".into(), IdlValue::Uint(100)),
            ("y".into(), IdlValue::Uint(200)),
        ])
    );
}

#[test]
fn indexed_idl_parse_instruction_marks_fields_unparsed_when_a_defined_struct_field_is_missing() {
    let indexed: IndexedIdl = serde_json::from_str(
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

    let parsed = indexed.parse_instruction(&data).unwrap().unwrap();
    assert_eq!(parsed.name, "create");
    assert!(matches!(
        parsed.fields,
        IdlInstructionFields::Unparsed(raw_args_hex) if raw_args_hex == "64000000"
    ));
}

#[test]
fn indexed_idl_parse_instruction_preserves_named_field_order() {
    let indexed: IndexedIdl = serde_json::from_str(
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

    let parsed = indexed.parse_instruction(&data).unwrap().unwrap();

    // Verify field order is preserved (zeta before alpha, matching IDL definition)
    assert_eq!(
        parsed.fields[0].value,
        IdlValue::Struct(vec![
            ("zeta".into(), IdlValue::Uint(100)),
            ("alpha".into(), IdlValue::Uint(200)),
        ])
    );
}

#[test]
fn indexed_idl_parse_instruction_with_enum_arg() {
    let indexed: IndexedIdl = serde_json::from_str(
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
    let parsed = indexed.parse_instruction(&data).unwrap().unwrap();
    assert_eq!(
        parsed.fields[0].value,
        IdlValue::Struct(vec![("Start".into(), IdlValue::Null)])
    );

    data[8] = 1;
    let parsed = indexed.parse_instruction(&data).unwrap().unwrap();
    assert_eq!(
        parsed.fields[0].value,
        IdlValue::Struct(vec![("Stop".into(), IdlValue::Null)])
    );
}

#[test]
fn indexed_idl_parse_instruction_with_vec_arg() {
    let indexed: IndexedIdl = serde_json::from_str(
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

    let parsed = indexed.parse_instruction(&data).unwrap().unwrap();
    assert_eq!(
        parsed.fields[0].value,
        IdlValue::Array(vec![IdlValue::Uint(10), IdlValue::Uint(20), IdlValue::Uint(30)])
    );
}

#[test]
fn indexed_idl_parse_instruction_with_option_arg() {
    let indexed: IndexedIdl = serde_json::from_str(
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
    let parsed = indexed.parse_instruction(&data).unwrap().unwrap();
    assert_eq!(parsed.fields[0].value, IdlValue::Uint(777));

    let data_none = vec![3, 3, 3, 3, 3, 3, 3, 3, 0];
    let parsed = indexed.parse_instruction(&data_none).unwrap().unwrap();
    assert_eq!(parsed.fields[0].value, IdlValue::Null);
}

#[test]
fn indexed_idl_find_instruction_prefers_longest_discriminator() {
    let indexed: IndexedIdl = serde_json::from_str(
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
    let found = indexed.find_instruction_by_discriminator(&data).unwrap();
    assert_eq!(found.name, "specific");
}

#[test]
fn indexed_idl_find_instruction_ignores_missing_discriminator() {
    let indexed: IndexedIdl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [
                { "name": "missing", "accounts": [], "args": [] }
            ]
        }"#,
    )
    .unwrap();

    let found = indexed.find_instruction_by_discriminator(&[1, 2, 3, 4, 5, 6, 7, 8]);
    assert!(found.is_none());
}

#[test]
fn indexed_idl_parse_instruction_returns_empty_fields_for_zero_arg_instruction() {
    let indexed: IndexedIdl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "ping",
                "discriminator": [9,9,9,9,9,9,9,9],
                "accounts": [{ "name": "payer", "writable": true, "signer": true }],
                "args": []
            }],
            "types": []
        }"#,
    )
    .unwrap();

    let data = vec![9, 9, 9, 9, 9, 9, 9, 9];

    let parsed = indexed.parse_instruction(&data).unwrap().unwrap();
    assert_eq!(parsed.name, "ping");
    assert!(matches!(parsed.fields, IdlInstructionFields::Parsed(ref fields) if fields.is_empty()));
    assert_eq!(parsed.account_names, vec!["payer"]);
}
