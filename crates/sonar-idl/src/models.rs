use serde::de::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::discriminator::{sighash, to_snake_case};

/// Enum to handle both legacy (flat) and new (nested metadata) IDL formats.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RawAnchorIdl {
    Current(Idl),
    Legacy(LegacyIdl),
}

impl RawAnchorIdl {
    pub fn convert(self, program_address: &str) -> Idl {
        match self {
            RawAnchorIdl::Current(idl) => idl,
            RawAnchorIdl::Legacy(legacy) => legacy.into_idl(program_address),
        }
    }
}

/// Legacy IDL structure (pre-0.30).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyIdl {
    pub version: String,
    pub name: String,
    pub instructions: Vec<IdlInstruction>,
    #[serde(default)]
    pub accounts: Option<Vec<IdlTypeDefinition>>,
    #[serde(default)]
    pub types: Option<Vec<IdlTypeDefinition>>,
    #[serde(default)]
    pub events: Option<Vec<IdlEvent>>,
    #[serde(default)]
    pub errors: Option<Vec<JsonValue>>,
    #[serde(default)]
    pub metadata: Option<IdlMetadata>,
}

impl LegacyIdl {
    fn into_idl(self, address: &str) -> Idl {
        let mut types = self.types.unwrap_or_default();
        if let Some(accounts) = self.accounts {
            types.extend(accounts);
        }

        let instructions: Vec<IdlInstruction> = self
            .instructions
            .into_iter()
            .map(|mut inst| {
                if inst.discriminator.is_none() {
                    let snake_name = to_snake_case(&inst.name);
                    inst.discriminator = Some(sighash("global", &snake_name).to_vec());
                }
                inst
            })
            .collect();

        let events: Option<Vec<IdlEvent>> = self.events.map(|events| {
            events
                .into_iter()
                .map(|mut event| {
                    if event.discriminator.is_none() {
                        event.discriminator = Some(sighash("event", &event.name).to_vec());
                    }
                    event
                })
                .collect()
        });

        let metadata = self.metadata.unwrap_or_else(|| IdlMetadata {
            name: self.name.clone(),
            version: self.version.clone(),
            spec: "0.1.0".to_string(),
            description: None,
        });

        Idl {
            address: address.to_string(),
            metadata,
            instructions,
            types: if types.is_empty() { None } else { Some(types) },
            events,
        }
    }
}

/// Complete IDL structure including types for full type resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Idl {
    pub address: String,
    pub metadata: IdlMetadata,
    pub instructions: Vec<IdlInstruction>,
    pub types: Option<Vec<IdlTypeDefinition>>,
    #[serde(default)]
    pub events: Option<Vec<IdlEvent>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlMetadata {
    pub name: String,
    pub version: String,
    pub spec: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlInstruction {
    pub name: String,
    #[serde(default)]
    pub discriminator: Option<Vec<u8>>,
    pub accounts: Vec<IdlAccountItem>,
    pub args: Vec<IdlArg>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlEvent {
    pub name: String,
    #[serde(default)]
    pub discriminator: Option<Vec<u8>>,
    #[serde(default)]
    pub fields: Option<Vec<IdlField>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IdlAccountItem {
    Account(IdlAccount),
    Accounts(IdlAccounts),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlAccount {
    pub name: String,
    #[serde(default, alias = "isMut")]
    pub writable: bool,
    #[serde(default, alias = "isSigner")]
    pub signer: bool,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlAccounts {
    pub name: String,
    pub accounts: Vec<IdlAccountItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlArg {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: IdlType,
}

/// IDL types — mirrors Anchor IDL type system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum IdlType {
    Simple(String),
    Vec { vec: Box<IdlType> },
    Option { option: Box<IdlType> },
    Array { array: [JsonValue; 2] },
    Defined { defined: DefinedType },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum DefinedType {
    Simple(String),
    Complex {
        name: String,
        #[serde(default)]
        generics: Option<Vec<JsonValue>>,
    },
}

impl DefinedType {
    pub fn name(&self) -> &str {
        match self {
            DefinedType::Simple(name) => name,
            DefinedType::Complex { name, .. } => name,
        }
    }
}

/// Custom type definitions (structs and enums).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlTypeDefinition {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: IdlTypeDefinitionBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlTypeDefinitionBody {
    pub kind: String,
    #[serde(default, deserialize_with = "deserialize_optional_idl_fields")]
    pub fields: Option<IdlFields>,
    #[serde(default)]
    pub variants: Option<Vec<IdlEnumVariant>>,
}

/// Fields for IDL types — named (regular structs) or tuple (tuple structs).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IdlFields {
    Named(Vec<IdlField>),
    Tuple(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlField {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: IdlType,
}

fn deserialize_optional_idl_fields<'de, D>(deserializer: D) -> Result<Option<IdlFields>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        Some(val) => deserialize_idl_fields_(val).map_err(D::Error::custom),
        None => Ok(None),
    }
}

fn deserialize_idl_fields_(value: serde_json::Value) -> Result<Option<IdlFields>, String> {
    match value {
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                Ok(Some(IdlFields::Tuple(Vec::new())))
            } else if arr[0].get("name").is_some() {
                match serde_json::from_value::<Vec<IdlField>>(serde_json::Value::Array(arr)) {
                    Ok(fields) => Ok(Some(IdlFields::Named(fields))),
                    Err(e) => Err(format!("Failed to parse named fields: {}", e)),
                }
            } else {
                match serde_json::from_value::<Vec<String>>(serde_json::Value::Array(arr)) {
                    Ok(types) => Ok(Some(IdlFields::Tuple(types))),
                    Err(e) => Err(format!("Failed to parse tuple fields: {}", e)),
                }
            }
        }
        _ => Err("Fields must be an array".to_string()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlEnumVariant {
    pub name: String,
    #[serde(default)]
    pub fields: Option<JsonValue>,
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let raw: RawAnchorIdl = serde_json::from_str(CURRENT_IDL_JSON).unwrap();
        let idl = raw.convert("BYFW1vhC1ohxwRbYoLbAWs86STa25i9sD5uEusVjTYNd");

        assert_eq!(idl.metadata.name, "hello_anchor");
        assert_eq!(idl.instructions.len(), 1);
        assert_eq!(idl.instructions[0].name, "initialize");
        assert_eq!(
            idl.instructions[0].discriminator,
            Some(vec![175, 175, 109, 31, 13, 152, 155, 237])
        );
        assert_eq!(idl.instructions[0].args.len(), 1);
        assert_eq!(idl.instructions[0].args[0].name, "data");
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
            IdlAccountItem::Account(a) => a,
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
                IdlType::Defined { defined: DefinedType::Simple("MyStruct".into()) },
            ),
        ];

        for (json_str, expected) in &types {
            let parsed: IdlType = serde_json::from_str(json_str).unwrap();
            assert_eq!(&parsed, expected, "failed for {}", json_str);
        }
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
}
