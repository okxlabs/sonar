use crate::idl::*;
use crate::indexed::IndexedIdl;
use crate::value::IdlValue;

#[test]
fn idl_type_serde_roundtrip() {
    let types = [
        (r#""u64""#, IdlType::Simple("u64".into())),
        (r#"{"vec":"u8"}"#, IdlType::Vec { vec: Box::new(IdlType::Simple("u8".into())) }),
        (
            r#"{"option":"bool"}"#,
            IdlType::Option { option: Box::new(IdlType::Simple("bool".into())) },
        ),
        (
            r#"{"defined":"MyStruct"}"#,
            IdlType::Defined { defined: DefinedType::Simple("MyStruct".into()) },
        ),
        (
            r#"{"array":[{"defined":"MyStruct"},3]}"#,
            IdlType::Array {
                array: IdlArrayType {
                    element_type: Box::new(IdlType::Defined {
                        defined: DefinedType::Simple("MyStruct".into()),
                    }),
                    length: 3,
                },
            },
        ),
    ];

    for (json_str, expected) in &types {
        let parsed: IdlType = serde_json::from_str(json_str).unwrap();
        assert_eq!(&parsed, expected, "failed for {}", json_str);
    }
}

#[test]
fn idl_type_definition_kind_serde_roundtrip() {
    let struct_def: IdlTypeDefinition = serde_json::from_str(
        r#"{
            "name": "MyStruct",
            "type": {
                "kind": "struct",
                "fields": [{ "name": "value", "type": "u64" }]
            }
        }"#,
    )
    .unwrap();
    assert_eq!(struct_def.type_.kind, IdlTypeDefinitionKind::Struct);

    let enum_def: IdlTypeDefinition = serde_json::from_str(
        r#"{
            "name": "MyEnum",
            "type": {
                "kind": "enum",
                "variants": [{ "name": "Ready" }]
            }
        }"#,
    )
    .unwrap();
    assert_eq!(enum_def.type_.kind, IdlTypeDefinitionKind::Enum);
}

#[test]
fn parse_current_idl_tuple_fields_with_nested_types() {
    let parsed: Result<Idl, _> = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "tuple_types", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [],
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
    );

    assert!(parsed.is_ok(), "tuple fields should support full IDL types: {:?}", parsed.err());
}

#[test]
fn parse_current_idl_format() {
    let indexed: IndexedIdl = serde_json::from_str(
        r#"{
            "address": "BYFW1vhC1ohxwRbYoLbAWs86STa25i9sD5uEusVjTYNd",
            "metadata": {
                "name": "hello_anchor",
                "version": "0.1.0",
                "spec": "0.1.0",
                "description": "Created with Anchor"
            },
            "instructions": [
                {
                    "name": "initialize",
                    "discriminator": [175, 175, 109, 31, 13, 152, 155, 237],
                    "accounts": [
                        { "name": "new_account", "writable": true, "signer": true },
                        { "name": "signer", "writable": true, "signer": true },
                        { "name": "system_program", "address": "11111111111111111111111111111111" }
                    ],
                    "args": [{ "name": "data", "type": "u64" }]
                }
            ],
            "types": [
                {
                    "name": "NewAccount",
                    "type": {
                        "kind": "struct",
                        "fields": [{ "name": "data", "type": "u64" }]
                    }
                }
            ]
        }"#,
    )
    .unwrap();

    let mut data = vec![175, 175, 109, 31, 13, 152, 155, 237];
    data.extend_from_slice(&42u64.to_le_bytes());

    let parsed = indexed.parse_instruction(&data).unwrap().expect("should parse");

    assert_eq!(parsed.name, "initialize");
    assert_eq!(parsed.fields.len(), 1);
    assert_eq!(parsed.fields[0].name, "data");
    assert_eq!(parsed.fields[0].value, IdlValue::Uint(42));
}

#[test]
fn current_idl_instruction_gets_auto_discriminator() {
    use crate::discriminator::sighash;

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
    let parsed = indexed.parse_instruction(&data).unwrap().expect("should parse");

    assert_eq!(parsed.name, "doSomething");
    assert!(parsed.fields.is_empty());
}

#[test]
fn current_idl_event_gets_auto_discriminator() {
    use crate::discriminator::sighash;
    use crate::indexed::is_cpi_event_data;

    let indexed: IndexedIdl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "current_program", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [],
            "events": [{
                "name": "TransferEvent",
                "fields": [{ "name": "amount", "type": "u64" }]
            }]
        }"#,
    )
    .unwrap();

    let emit_disc: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
    let mut data = emit_disc.to_vec();
    data.extend_from_slice(&sighash("event", "TransferEvent"));
    data.extend_from_slice(&7u64.to_le_bytes());

    let parsed = indexed.parse_cpi_event_data(&data).unwrap().expect("should parse event");

    assert_eq!(parsed.name, "TransferEvent");
    assert_eq!(parsed.fields.len(), 1);
    assert_eq!(parsed.fields[0].name, "amount");
    assert_eq!(parsed.fields[0].value, IdlValue::Uint(7));
    assert!(is_cpi_event_data(&data));
}

#[test]
fn current_idl_event_fields_support_tuple_types() {
    use crate::discriminator::sighash;

    let indexed: IndexedIdl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "current_program", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [],
            "events": [{
                "name": "PairEvent",
                "fields": ["u32", {"option":"u16"}]
            }]
        }"#,
    )
    .unwrap();

    let emit_disc: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
    let mut data = emit_disc.to_vec();
    data.extend_from_slice(&sighash("event", "PairEvent"));
    data.extend_from_slice(&9u32.to_le_bytes());
    data.push(1);
    data.extend_from_slice(&7u16.to_le_bytes());

    let parsed = indexed.parse_cpi_event_data(&data).unwrap().expect("should parse tuple event");

    assert_eq!(parsed.name, "PairEvent");
    assert_eq!(parsed.fields.len(), 2);
    assert_eq!(parsed.fields[0].name, "field_0");
    assert_eq!(parsed.fields[0].value, IdlValue::Uint(9));
    assert_eq!(parsed.fields[1].name, "field_1");
    assert_eq!(parsed.fields[1].value, IdlValue::Uint(7));
}

#[test]
fn parse_legacy_idl_and_into_indexed_idl() {
    use crate::discriminator::sighash;

    let indexed: IndexedIdl = serde_json::from_str(
        r#"{
            "version": "0.1.0",
            "name": "legacy_program",
            "instructions": [
                {
                    "name": "doSomething",
                    "accounts": [
                        { "name": "authority", "isMut": true, "isSigner": true }
                    ],
                    "args": [
                        { "name": "amount", "type": "u64" }
                    ]
                }
            ],
            "accounts": [
                {
                    "name": "MyState",
                    "type": {
                        "kind": "struct",
                        "fields": [
                            { "name": "value", "type": "u32" }
                        ]
                    }
                }
            ]
        }"#,
    )
    .unwrap();

    let mut data = sighash("global", "do_something").to_vec();
    data.extend_from_slice(&123u64.to_le_bytes());

    let parsed = indexed.parse_instruction(&data).unwrap().expect("should parse");

    assert_eq!(parsed.name, "doSomething");
    assert_eq!(parsed.fields.len(), 1);
    assert_eq!(parsed.fields[0].name, "amount");
    assert_eq!(parsed.fields[0].value, IdlValue::Uint(123));
}

#[test]
fn legacy_idl_accounts_merge_into_types() {
    use crate::discriminator::sighash;

    let json = r#"{
        "version": "0.1.0",
        "name": "merge_test",
        "instructions": [],
        "accounts": [
            { "name": "AcctA", "type": { "kind": "struct", "fields": [] } }
        ],
        "types": [
            { "name": "TypeB", "type": { "kind": "struct", "fields": [] } }
        ]
    }"#;
    let indexed: IndexedIdl = serde_json::from_str(json).unwrap();
    let data = sighash("account", "AcctA").to_vec();

    let (type_name, value) = indexed.parse_account_data(&data).unwrap().expect("should parse");
    assert_eq!(type_name, "AcctA");
    assert_eq!(value, IdlValue::Array(vec![]));
}

#[test]
fn legacy_event_gets_auto_discriminator() {
    use crate::discriminator::sighash;

    let json = r#"{
        "version": "0.1.0",
        "name": "event_test",
        "instructions": [],
        "events": [
            { "name": "TransferEvent", "fields": [{ "name": "amount", "type": "u64" }] }
        ]
    }"#;
    let indexed: IndexedIdl = serde_json::from_str(json).unwrap();

    let emit_disc: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
    let mut data = emit_disc.to_vec();
    data.extend_from_slice(&sighash("event", "TransferEvent"));
    data.extend_from_slice(&7u64.to_le_bytes());

    let parsed = indexed.parse_cpi_event_data(&data).unwrap().expect("should parse event");

    assert_eq!(parsed.name, "TransferEvent");
    assert_eq!(parsed.fields.len(), 1);
    assert_eq!(parsed.fields[0].name, "amount");
    assert_eq!(parsed.fields[0].value, IdlValue::Uint(7));
}
