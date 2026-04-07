use sonar_idl::{
    Discriminator, Idl, IdlFields, IdlType, IdlTypeDefinitionKind, RawAnchorIdl,
};

const CURRENT_IDL_JSON: &str = r#"{
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
}"#;

const LEGACY_IDL_JSON: &str = r#"{
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
}"#;

#[test]
fn parse_current_idl_format() {
    let idl =
        Idl::parse(CURRENT_IDL_JSON.as_bytes(), "BYFW1vhC1ohxwRbYoLbAWs86STa25i9sD5uEusVjTYNd")
            .unwrap();

    assert_eq!(idl.idl().metadata.name, "hello_anchor");
    assert_eq!(idl.idl().instructions.len(), 1);
    assert_eq!(idl.idl().instructions[0].name, "initialize");
    assert_eq!(
        idl.idl().instructions[0].discriminator,
        Some(vec![175, 175, 109, 31, 13, 152, 155, 237])
    );
    assert_eq!(idl.idl().instructions[0].args.len(), 1);
    assert_eq!(idl.idl().instructions[0].args[0].name, "data");
}

#[test]
fn current_idl_instruction_gets_auto_discriminator_on_convert() {
    let raw: RawAnchorIdl = serde_json::from_str(
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

    let idl = raw.convert("11111111111111111111111111111111");
    let instruction = &idl.instructions[0];

    assert!(instruction.discriminator.is_some());
    assert_eq!(instruction.discriminator.as_ref().unwrap().len(), 8);
}

#[test]
fn current_idl_event_gets_auto_discriminator_on_convert() {
    let raw: RawAnchorIdl = serde_json::from_str(
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

    let idl = raw.convert("11111111111111111111111111111111");
    let event = &idl.events.as_ref().unwrap()[0];

    assert!(event.discriminator.is_some());
    assert_eq!(event.discriminator.as_ref().unwrap().len(), 8);
}

#[test]
fn current_idl_event_fields_support_tuple_types() {
    let raw: RawAnchorIdl = serde_json::from_str(
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

    let idl = raw.convert("11111111111111111111111111111111");
    let event = &idl.events.as_ref().unwrap()[0];

    assert_eq!(
        event.fields,
        Some(IdlFields::Tuple(vec![
            IdlType::Simple("u32".into()),
            IdlType::Option { option: Box::new(IdlType::Simple("u16".into())) },
        ]))
    );
}

#[test]
fn parse_legacy_idl_and_convert() {
    let raw: RawAnchorIdl = serde_json::from_str(LEGACY_IDL_JSON).unwrap();
    let idl = raw.convert("11111111111111111111111111111111");

    assert_eq!(idl.metadata.name, "legacy_program");
    assert_eq!(idl.address, "11111111111111111111111111111111");

    assert_eq!(idl.instructions.len(), 1);
    let inst = &idl.instructions[0];
    assert_eq!(inst.name, "doSomething");
    assert!(inst.discriminator.is_some(), "legacy instruction should get auto-discriminator");
    assert_eq!(inst.discriminator.as_ref().unwrap().len(), 8);

    let types = idl.types.as_ref().expect("types should be populated from legacy accounts");
    assert!(types.iter().any(|t| t.name == "MyState"));
}

#[test]
fn legacy_idl_accounts_merge_into_types() {
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
    let raw: RawAnchorIdl = serde_json::from_str(json).unwrap();
    let idl = raw.convert("11111111111111111111111111111111");

    let types = idl.types.unwrap();
    assert_eq!(types.len(), 2);
    assert!(types.iter().any(|t| t.name == "TypeB"));
    assert!(types.iter().any(|t| t.name == "AcctA"));
}

#[test]
fn legacy_accounts_use_is_mut_and_is_signer_aliases() {
    let json = r#"{
        "version": "0.1.0",
        "name": "alias_test",
        "instructions": [{
            "name": "init",
            "accounts": [
                { "name": "payer", "isMut": true, "isSigner": true }
            ],
            "args": []
        }]
    }"#;
    let raw: RawAnchorIdl = serde_json::from_str(json).unwrap();
    let idl = raw.convert("11111111111111111111111111111111");

    let acct = match &idl.instructions[0].accounts[0] {
        sonar_idl::IdlAccountItem::Account(a) => a,
        _ => panic!("expected Account"),
    };
    assert!(acct.writable);
    assert!(acct.signer);
}

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
            IdlType::Defined { defined: sonar_idl::DefinedType::Simple("MyStruct".into()) },
        ),
        (
            r#"{"array":[{"defined":"MyStruct"},3]}"#,
            IdlType::Array {
                array: sonar_idl::IdlArrayType {
                    element_type: Box::new(IdlType::Defined {
                        defined: sonar_idl::DefinedType::Simple("MyStruct".into()),
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
    let struct_def: sonar_idl::IdlTypeDefinition = serde_json::from_str(
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

    let enum_def: sonar_idl::IdlTypeDefinition = serde_json::from_str(
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
fn legacy_event_gets_auto_discriminator() {
    let json = r#"{
        "version": "0.1.0",
        "name": "event_test",
        "instructions": [],
        "events": [
            { "name": "TransferEvent", "fields": [{ "name": "amount", "type": "u64" }] }
        ]
    }"#;
    let raw: RawAnchorIdl = serde_json::from_str(json).unwrap();
    let idl = raw.convert("11111111111111111111111111111111");

    let events = idl.events.as_ref().unwrap();
    assert_eq!(events.len(), 1);
    assert!(events[0].discriminator.is_some());
    assert_eq!(events[0].discriminator.as_ref().unwrap().len(), 8);
}

#[test]
fn discriminator_from_bytes() {
    let bytes = [1, 2, 3, 4, 5, 6, 7, 8];
    let disc = Discriminator::from_bytes(&bytes).unwrap();
    assert_eq!(disc.as_bytes(), &bytes);

    assert!(Discriminator::from_bytes(&[1, 2, 3]).is_none());
    assert!(Discriminator::from_bytes(&[1; 9]).is_none());
}
