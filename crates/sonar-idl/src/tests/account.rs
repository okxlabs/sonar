use crate::discriminator::sighash;
use crate::indexed::IndexedIdl;
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
fn indexed_idl_uses_top_level_accounts_discriminator() {
    // Shank / mpl-core shape: a 1-byte discriminator lives on the top-level
    // `accounts[]` entry but is *also* the struct's first field (a tag enum).
    // sonar must match `AssetV1` from a bare leading `01`, decode the byte
    // as `key` (don't skip it), and not register unrelated `types[]` entries
    // like `Royalties` as accounts.
    let indexed: IndexedIdl = serde_json::from_str(
        r#"{
            "address": "11111111111111111111111111111111",
            "metadata": { "name": "core_like", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [],
            "accounts": [
                { "name": "AssetV1", "discriminator": [1] }
            ],
            "types": [
                {
                    "name": "AssetV1",
                    "type": {
                        "kind": "struct",
                        "fields": [
                            { "name": "key", "type": "u8" },
                            { "name": "owner", "type": "u8" }
                        ]
                    }
                },
                {
                    "name": "Royalties",
                    "type": {
                        "kind": "struct",
                        "fields": [{ "name": "basis_points", "type": "u16" }]
                    }
                }
            ]
        }"#,
    )
    .unwrap();

    let data = vec![1u8, 42u8];
    let (name, value) = indexed.parse_account_data(&data).unwrap().expect("AssetV1 match");
    assert_eq!(name, "AssetV1");
    assert_eq!(
        value,
        IdlValue::Struct(
            vec![("key".into(), IdlValue::U8(1)), ("owner".into(), IdlValue::U8(42)),]
        )
    );

    // Royalties only lives in `types[]`, so it must not be findable as an account.
    let royalties_disc = sighash("account", "Royalties");
    let mut data = royalties_disc.to_vec();
    data.extend_from_slice(&7u16.to_le_bytes());
    assert!(indexed.parse_account_data(&data).unwrap().is_none());
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
