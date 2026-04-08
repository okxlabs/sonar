use serde_json::json;

use crate::discriminator::sighash;
use crate::indexed::{IndexedIdl, is_cpi_event_data};

use super::hello_anchor_indexed_idl;

#[test]
fn is_cpi_event_data_detects_emit_cpi() {
    let mut data = vec![0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
    data.extend_from_slice(&[0; 8]);
    assert!(is_cpi_event_data(&data));

    // Exactly the 8-byte emit prefix with no event discriminator is still valid.
    let exact = vec![0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
    assert!(is_cpi_event_data(&exact));
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
fn indexed_idl_parse_cpi_event_data_returns_none_for_unknown_event_discriminator() {
    let indexed = hello_anchor_indexed_idl();
    let mut data = vec![0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
    data.extend_from_slice(&[0; 8]);

    let result = indexed.parse_cpi_event_data(&data).unwrap();
    assert!(result.is_none());
}

#[test]
fn indexed_idl_parse_cpi_event_data_parses_event_fields() {
    let event_disc = sighash("event", "TransferDone");

    let indexed: IndexedIdl = serde_json::from_str(&format!(
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

    let result = indexed.parse_cpi_event_data(&data).unwrap();
    let parsed = result.expect("should parse event");

    assert_eq!(parsed.name, "TransferDone");
    assert_eq!(parsed.fields.len(), 1);
    assert_eq!(parsed.fields[0].name, "amount");
    assert_eq!(parsed.fields[0].value, json!(500u64));
    assert_eq!(parsed.account_names, vec!["event_authority"]);
}

#[test]
fn indexed_idl_parse_cpi_event_data_parses_tuple_event_fields() {
    let event_disc = sighash("event", "PairEvent");

    let indexed: IndexedIdl = serde_json::from_str(&format!(
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

    let result = indexed.parse_cpi_event_data(&data).unwrap();
    let parsed = result.expect("should parse tuple event");

    assert_eq!(parsed.name, "PairEvent");
    assert_eq!(parsed.fields.len(), 2);
    assert_eq!(parsed.fields[0].name, "field_0");
    assert_eq!(parsed.fields[0].value, json!(9u64));
    assert_eq!(parsed.fields[1].name, "field_1");
    assert_eq!(parsed.fields[1].value, json!(7u64));
    assert_eq!(parsed.account_names, vec!["event_authority"]);
}
