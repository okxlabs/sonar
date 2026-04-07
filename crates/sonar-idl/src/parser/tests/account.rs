use serde_json::json;

use crate::discriminator::sighash;

use super::hello_anchor_indexed_idl;

#[test]
fn indexed_idl_parses_account_data_matches_struct_by_discriminator() {
    let indexed = hello_anchor_indexed_idl();

    let disc = sighash("account", "NewAccount");
    let mut data = disc.to_vec();
    data.extend_from_slice(&99u64.to_le_bytes());

    let result = indexed.parse_account_data(&data).unwrap();
    let (type_name, value) = result.expect("should match NewAccount");

    assert_eq!(type_name, "NewAccount");
    assert_eq!(value, json!({ "data": 99u64 }));
}

#[test]
fn indexed_idl_parse_account_data_returns_none_for_unknown_discriminator() {
    let indexed = hello_anchor_indexed_idl();
    let data = [0u8; 16];

    let result = indexed.parse_account_data(&data).unwrap();
    assert!(result.is_none());
}

#[test]
fn indexed_idl_parse_account_data_rejects_short_data() {
    let indexed = hello_anchor_indexed_idl();

    let result = indexed.parse_account_data(&[0u8; 4]);
    assert!(result.is_err());
}
