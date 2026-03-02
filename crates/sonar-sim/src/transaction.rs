use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bs58::decode::Error as Base58Error;
use serde::Serialize;
use solana_message::VersionedMessage;
use solana_pubkey::Pubkey;
use solana_transaction::versioned::{TransactionVersion, VersionedTransaction};

use crate::error::{Result, SonarSimError};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RawTransactionEncoding {
    Base58,
    Base64,
}

impl fmt::Display for RawTransactionEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Base58 => f.write_str("base58"),
            Self::Base64 => f.write_str("base64"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParsedTransaction {
    pub encoding: RawTransactionEncoding,
    pub version: TransactionVersion,
    pub transaction: VersionedTransaction,
    pub account_plan: MessageAccountPlan,
}

#[derive(Debug, Clone)]
pub struct MessageAccountPlan {
    pub static_accounts: Vec<Pubkey>,
    pub address_lookups: Vec<AddressLookupPlan>,
}

impl MessageAccountPlan {
    pub fn from_transaction(tx: &VersionedTransaction) -> Self {
        let static_accounts = tx.message.static_account_keys().to_vec();
        let address_lookups = build_address_lookup_plan(&tx.message);
        Self { static_accounts, address_lookups }
    }
}

#[derive(Debug, Clone)]
pub struct AddressLookupPlan {
    pub account_key: Pubkey,
    pub writable_indexes: Vec<u8>,
    pub readonly_indexes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct LookupLocation {
    pub table_account: Pubkey,
    pub table_index: u8,
    pub writable: bool,
}

pub fn parse_raw_transaction(raw: &str) -> Result<ParsedTransaction> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(SonarSimError::TransactionParse {
            reason: "Raw transaction string is empty".into(),
        });
    }

    let mut errors = Vec::new();

    for encoding in [RawTransactionEncoding::Base64, RawTransactionEncoding::Base58] {
        match decode_bytes(trimmed, encoding) {
            Ok(bytes) => match bincode::deserialize::<VersionedTransaction>(&bytes) {
                Ok(transaction) => {
                    let version = transaction.version();
                    let account_plan = MessageAccountPlan::from_transaction(&transaction);
                    return Ok(ParsedTransaction { encoding, version, transaction, account_plan });
                }
                Err(err) => errors.push(format!(
                    "{} deserialization failed: {err}",
                    match encoding {
                        RawTransactionEncoding::Base58 => "Base58",
                        RawTransactionEncoding::Base64 => "Base64",
                    }
                )),
            },
            Err(err) => errors.push(err.to_string()),
        }
    }

    let merged = errors.join("; ");
    Err(SonarSimError::TransactionParse {
        reason: format!("Failed to parse raw transaction: {merged}"),
    })
}

fn build_address_lookup_plan(message: &VersionedMessage) -> Vec<AddressLookupPlan> {
    message
        .address_table_lookups()
        .map(|lookups| {
            lookups
                .iter()
                .map(|lookup| AddressLookupPlan {
                    account_key: lookup.account_key,
                    writable_indexes: lookup.writable_indexes.clone(),
                    readonly_indexes: lookup.readonly_indexes.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Builds lookup table account position mapping table.
///
/// According to Solana v0 message ordering rules, all writable lookup
/// addresses from all tables must come first, followed by all readonly
/// lookup addresses from all tables. This function preserves that order.
pub fn build_lookup_locations(plan: &[AddressLookupPlan]) -> Vec<LookupLocation> {
    let mut locations = Vec::new();

    for entry in plan {
        for &idx in &entry.writable_indexes {
            locations.push(LookupLocation {
                table_account: entry.account_key,
                table_index: idx,
                writable: true,
            });
        }
    }

    for entry in plan {
        for &idx in &entry.readonly_indexes {
            locations.push(LookupLocation {
                table_account: entry.account_key,
                table_index: idx,
                writable: false,
            });
        }
    }

    locations
}

fn decode_bytes(input: &str, encoding: RawTransactionEncoding) -> Result<Vec<u8>> {
    match encoding {
        RawTransactionEncoding::Base58 => {
            bs58::decode(input).into_vec().map_err(|err| map_base58_error(input, err))
        }
        RawTransactionEncoding::Base64 => BASE64_STANDARD.decode(input.as_bytes()).map_err(|err| {
            SonarSimError::TransactionParse { reason: format!("Base64 decode failed: {err}") }
        }),
    }
}

fn map_base58_error(input: &str, err: Base58Error) -> SonarSimError {
    let base_message = match err {
        Base58Error::InvalidCharacter { character, index } => {
            format!(
                "Base58 decode failed: position {index} contains invalid character `{character}`"
            )
        }
        other => format!("Base58 decode failed: {other}"),
    };

    if input.contains(['+', '/', '=']) {
        SonarSimError::TransactionParse {
            reason: format!(
                "{base_message}. Base64 characteristic characters detected, you may need to try Base64 encoding"
            ),
        }
    } else {
        SonarSimError::TransactionParse { reason: base_message }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use solana_hash::Hash;
    use solana_keypair::Keypair;
    use solana_message::Message;
    use solana_pubkey::Pubkey;
    use solana_signer::Signer;
    use solana_system_interface::instruction as system_instruction;
    use solana_transaction::Transaction;

    fn sample_transaction() -> (VersionedTransaction, Pubkey) {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();
        let blockhash = Hash::new_unique();
        let instruction = system_instruction::transfer(&payer.pubkey(), &recipient, 42);
        let message = Message::new(&[instruction], Some(&payer.pubkey()));
        let transaction = Transaction::new(&[&payer], message, blockhash);
        (VersionedTransaction::from(transaction), payer.pubkey())
    }

    #[test]
    fn parse_base64_transaction() {
        let (versioned, payer) = sample_transaction();
        let bytes = bincode::serialize(&versioned).unwrap();
        let base64 = BASE64_STANDARD.encode(&bytes);

        let parsed = parse_raw_transaction(&base64).expect("parse base64");
        assert_eq!(parsed.encoding, RawTransactionEncoding::Base64);
        assert_eq!(parsed.account_plan.static_accounts.len(), 3);
        assert_eq!(parsed.account_plan.static_accounts[0], payer);
    }

    #[test]
    fn parse_base58_transaction() {
        let (versioned, _) = sample_transaction();
        let bytes = bincode::serialize(&versioned).unwrap();
        let base58 = bs58::encode(&bytes).into_string();

        let parsed = parse_raw_transaction(&base58).expect("parse base58");
        assert_eq!(parsed.encoding, RawTransactionEncoding::Base58);
    }

    #[test]
    fn raw_transaction_encoding_display() {
        assert_eq!(RawTransactionEncoding::Base58.to_string(), "base58");
        assert_eq!(RawTransactionEncoding::Base64.to_string(), "base64");
    }

    #[test]
    fn test_build_lookup_locations_ordering() {
        let table1 = Pubkey::new_unique();
        let table2 = Pubkey::new_unique();

        let plan = vec![
            AddressLookupPlan {
                account_key: table1,
                writable_indexes: vec![0, 1],
                readonly_indexes: vec![2, 3],
            },
            AddressLookupPlan {
                account_key: table2,
                writable_indexes: vec![5, 6],
                readonly_indexes: vec![7],
            },
        ];

        let locations = build_lookup_locations(&plan);

        assert_eq!(locations.len(), 7);

        assert_eq!(locations[0].table_account, table1);
        assert_eq!(locations[0].table_index, 0);
        assert!(locations[0].writable);

        assert_eq!(locations[1].table_account, table1);
        assert_eq!(locations[1].table_index, 1);
        assert!(locations[1].writable);

        assert_eq!(locations[2].table_account, table2);
        assert_eq!(locations[2].table_index, 5);
        assert!(locations[2].writable);

        assert_eq!(locations[3].table_account, table2);
        assert_eq!(locations[3].table_index, 6);
        assert!(locations[3].writable);

        assert_eq!(locations[4].table_account, table1);
        assert_eq!(locations[4].table_index, 2);
        assert!(!locations[4].writable);

        assert_eq!(locations[5].table_account, table1);
        assert_eq!(locations[5].table_index, 3);
        assert!(!locations[5].writable);

        assert_eq!(locations[6].table_account, table2);
        assert_eq!(locations[6].table_index, 7);
        assert!(!locations[6].writable);
    }

    #[test]
    fn test_build_lookup_locations_empty() {
        let locations = build_lookup_locations(&[]);
        assert_eq!(locations.len(), 0);
    }

    #[test]
    fn test_build_lookup_locations_single_table() {
        let table = Pubkey::new_unique();
        let plan = vec![AddressLookupPlan {
            account_key: table,
            writable_indexes: vec![10],
            readonly_indexes: vec![20, 21],
        }];

        let locations = build_lookup_locations(&plan);

        assert_eq!(locations.len(), 3);
        assert_eq!(locations[0].table_index, 10);
        assert!(locations[0].writable);
        assert_eq!(locations[1].table_index, 20);
        assert!(!locations[1].writable);
        assert_eq!(locations[2].table_index, 21);
        assert!(!locations[2].writable);
    }
}
