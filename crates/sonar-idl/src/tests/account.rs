use crate::discriminator::sighash;
use crate::value::IdlValue;

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
    assert_eq!(value, IdlValue::Struct(vec![("data".into(), IdlValue::U64(99))]));
}

#[test]
fn indexed_idl_parse_account_data_returns_none_for_unknown_discriminator() {
    let indexed = hello_anchor_indexed_idl();
    let data = [0u8; 16];

    let result = indexed.parse_account_data(&data).unwrap();
    assert!(result.is_none());
}

#[test]
fn indexed_idl_parse_account_data_returns_none_for_unmatched_short_data() {
    let indexed = hello_anchor_indexed_idl();

    let result = indexed.parse_account_data(&[0u8; 4]);
    assert!(result.unwrap().is_none());
}

#[test]
fn indexed_idl_parse_account_data_errors_when_matched_fields_are_truncated() {
    let indexed = hello_anchor_indexed_idl();

    // NewAccount has an 8-byte discriminator followed by a u64 field.
    // Send just the discriminator with no field data.
    let disc = sighash("account", "NewAccount");
    let result = indexed.parse_account_data(&disc);
    assert!(result.is_err());
}
