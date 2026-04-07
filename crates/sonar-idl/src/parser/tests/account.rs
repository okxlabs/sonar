use serde_json::json;

use crate::discriminator::sighash;

use super::super::{ResolvedIdl, parse_account_data};
use super::hello_anchor_idl;

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
