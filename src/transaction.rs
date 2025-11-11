use std::path::Path;

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use bs58::decode::Error as Base58Error;
use serde::Serialize;
use solana_message::inner_instruction::InnerInstructionsList;
use solana_sdk::{
    message::VersionedMessage,
    pubkey::Pubkey,
    transaction::{TransactionVersion, VersionedTransaction},
};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RawTransactionEncoding {
    Base58,
    Base64,
}

#[derive(Debug, Clone)]
pub struct ParsedTransaction {
    pub encoding: RawTransactionEncoding,
    pub version: TransactionVersion,
    pub transaction: VersionedTransaction,
    pub summary: TransactionSummary,
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
        Self {
            static_accounts,
            address_lookups,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AddressLookupPlan {
    pub account_key: Pubkey,
    pub writable_indexes: Vec<u8>,
    pub readonly_indexes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransactionSummary {
    pub signatures: Vec<String>,
    pub recent_blockhash: String,
    pub static_accounts: Vec<AccountKeySummary>,
    pub instructions: Vec<InstructionSummary>,
    pub inner_instructions: InnerInstructionsList,
    pub address_table_lookups: Vec<AddressLookupSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountKeySummary {
    pub index: usize,
    pub pubkey: String,
    pub signer: bool,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstructionSummary {
    pub index: usize,
    pub program: AccountReferenceSummary,
    pub accounts: Vec<AccountReferenceSummary>,
    pub data: Box<[u8]>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum AccountSourceSummary {
    Static,
    Lookup {
        table_account: String,
        lookup_index: u8,
        writable: bool,
    },
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountReferenceSummary {
    pub index: usize,
    pub pubkey: Option<String>,
    pub signer: bool,
    pub writable: bool,
    pub source: AccountSourceSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct AddressLookupSummary {
    pub account_key: String,
    pub writable_indexes: Vec<u8>,
    pub readonly_indexes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct LookupLocation {
    pub table_account: Pubkey,
    pub table_index: u8,
    pub writable: bool,
}

pub fn read_raw_transaction(inline: Option<String>, tx_file: Option<&Path>) -> Result<String> {
    match (inline, tx_file) {
        (Some(tx), None) => {
            let trimmed = tx.trim();
            if trimmed.is_empty() {
                Err(anyhow!("Raw transaction string cannot be empty"))
            } else {
                Ok(trimmed.to_owned())
            }
        }
        (None, Some(path)) => {
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read transaction file: {}", path.display()))?;
            let trimmed = content.trim();
            if trimmed.is_empty() {
                Err(anyhow!(
                    "File `{}` does not contain valid raw transaction content",
                    path.display()
                ))
            } else {
                Ok(trimmed.to_owned())
            }
        }
        (Some(_), Some(_)) => Err(anyhow!("Please specify only one of --tx or --tx-file")),
        (None, None) => Err(anyhow!(
            "No raw transaction provided, please use --tx or --tx-file"
        )),
    }
}

pub fn is_transaction_signature(s: &str) -> bool {
    let trimmed = s.trim();
    // Solana signatures are typically 87-88 characters in base58
    if trimmed.len() < 87 || trimmed.len() > 88 {
        return false;
    }
    
    // Check if it contains only base58 characters (alphanumeric except 0OIl)
    trimmed.chars().all(|c| c.is_ascii_alphanumeric() && !matches!(c, '0' | 'O' | 'I' | 'l'))
}

pub fn fetch_transaction_from_rpc(rpc_url: &str, signature: &str) -> Result<String> {
    use crate::account_loader::AccountLoader;
    
    let loader = AccountLoader::new(rpc_url.to_string())?;
    let tx = loader.fetch_transaction_by_signature(signature)?;
    
    let serialized = bincode::serialize(&tx)
        .context("Failed to serialize transaction")?;
    
    Ok(BASE64_STANDARD.encode(serialized))
}

pub fn parse_raw_transaction(raw: &str) -> Result<ParsedTransaction> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("Raw transaction string is empty"));
    }

    let mut errors = Vec::new();

    for encoding in [
        RawTransactionEncoding::Base64,
        RawTransactionEncoding::Base58,
    ] {
        match decode_bytes(trimmed, encoding) {
            Ok(bytes) => match bincode::deserialize::<VersionedTransaction>(&bytes) {
                Ok(transaction) => {
                    let version = transaction.version();
                    let account_plan = MessageAccountPlan::from_transaction(&transaction);
                    let summary = TransactionSummary::from_transaction(&transaction, &account_plan, Vec::new());
                    return Ok(ParsedTransaction {
                        encoding,
                        version,
                        transaction,
                        summary,
                        account_plan,
                    });
                }
                Err(err) => errors.push(anyhow!(
                    "{} deserialization failed: {err}",
                    match encoding {
                        RawTransactionEncoding::Base58 => "Base58",
                        RawTransactionEncoding::Base64 => "Base64",
                    }
                )),
            },
            Err(err) => errors.push(err),
        }
    }

    let merged = errors
        .into_iter()
        .map(|err| err.to_string())
        .collect::<Vec<_>>()
        .join("； ");
    Err(anyhow!("Failed to parse raw transaction: {merged}"))
}

pub fn collect_account_plan(tx: &VersionedTransaction) -> MessageAccountPlan {
    MessageAccountPlan::from_transaction(tx)
}

fn decode_bytes(input: &str, encoding: RawTransactionEncoding) -> Result<Vec<u8>> {
    match encoding {
        RawTransactionEncoding::Base58 => bs58::decode(input)
            .into_vec()
            .map_err(|err| map_base58_error(input, err)),
        RawTransactionEncoding::Base64 => BASE64_STANDARD
            .decode(input.as_bytes())
            .map_err(|err| anyhow!("Base64 decode failed: {err}")),
    }
}

fn map_base58_error(input: &str, err: Base58Error) -> anyhow::Error {
    let base_message = match err {
        Base58Error::InvalidCharacter { character, index } => {
            format!(
                "Base58 decode failed: position {index} contains invalid character `{character}`"
            )
        }
        other => format!("Base58 decode failed: {other}"),
    };

    if input.contains(['+', '/', '=']) {
        anyhow!("{base_message}. Base64 characteristic characters detected, you may need to try Base64 encoding")
    } else {
        anyhow!(base_message)
    }
}

impl TransactionSummary {
    pub fn from_transaction(
        tx: &VersionedTransaction,
        plan: &MessageAccountPlan,
        inner_instructions: InnerInstructionsList,
    ) -> Self {
        let message = &tx.message;
        let lookup_locations = build_lookup_locations(&plan.address_lookups);
        let static_accounts = plan
            .static_accounts
            .iter()
            .enumerate()
            .map(|(index, key)| AccountKeySummary {
                index,
                pubkey: key.to_string(),
                signer: message.is_signer(index),
                writable: message.is_maybe_writable(index, None),
            })
            .collect();

        let instructions = message
            .instructions()
            .iter()
            .enumerate()
            .map(|(idx, ix)| InstructionSummary {
                index: idx,
                program: classify_account_reference(
                    message,
                    ix.program_id_index as usize,
                    plan,
                    &lookup_locations,
                ),
                accounts: ix
                    .accounts
                    .iter()
                    .map(|account_index| {
                        classify_account_reference(
                            message,
                            *account_index as usize,
                            plan,
                            &lookup_locations,
                        )
                    })
                    .collect(),
                data: ix.data.clone().into_boxed_slice(),
            })
            .collect();

        let address_table_lookups = plan
            .address_lookups
            .iter()
            .map(|lookup| AddressLookupSummary {
                account_key: lookup.account_key.to_string(),
                writable_indexes: lookup.writable_indexes.clone(),
                readonly_indexes: lookup.readonly_indexes.clone(),
            })
            .collect();

        TransactionSummary {
            signatures: tx.signatures.iter().map(|sig| sig.to_string()).collect(),
            recent_blockhash: message.recent_blockhash().to_string(),
            static_accounts,
            instructions,
            inner_instructions,
            address_table_lookups,
        }
    }
}

/// Classifies account index as static account or lookup table account
///
/// Account index rules in Solana V0 transactions:
/// - Account indexes in Instructions are **global indexes**, ranging [0, total_accounts)
/// - total_accounts = static_accounts.len() + lookup_accounts.len()
///
/// Global index to account mapping rules:
/// 1. Indexes [0, static_accounts.len()) map to static accounts
/// 2. Indexes [static_accounts.len(), total_accounts) map to lookup table accounts
///
/// Lookup table account order:
/// - For each address_table_lookup (in transaction order):
///   a. First all accounts corresponding to writable_indexes of that table
///   b. Then all accounts corresponding to readonly_indexes of that table
///
/// # Parameters
/// * `message` - Transaction message for querying account attributes
/// * `index` - Global account index referenced in instruction
/// * `plan` - Account plan containing static accounts and lookup table info
/// * `lookup_locations` - Position mapping table for lookup table accounts
///
/// # Returns
/// Returns account reference summary containing account source, pubkey, signer, and writable attributes
pub fn classify_account_reference(
    message: &VersionedMessage,
    index: usize,
    plan: &MessageAccountPlan,
    lookup_locations: &[LookupLocation],
) -> AccountReferenceSummary {
    if index < plan.static_accounts.len() {
        AccountReferenceSummary {
            index,
            pubkey: Some(plan.static_accounts[index].to_string()),
            signer: message.is_signer(index),
            writable: message.is_maybe_writable(index, None),
            source: AccountSourceSummary::Static,
        }
    } else {
        let lookup_index = index - plan.static_accounts.len();
        let Some(location) = lookup_locations.get(lookup_index) else {
            return AccountReferenceSummary {
                index,
                pubkey: None,
                signer: false,
                writable: false,
                source: AccountSourceSummary::Unknown,
            };
        };
        AccountReferenceSummary {
            index,
            pubkey: None,
            signer: false,
            writable: location.writable,
            source: AccountSourceSummary::Lookup {
                table_account: location.table_account.to_string(),
                lookup_index: location.table_index,
                writable: location.writable,
            },
        }
    }
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

/// Builds lookup table account position mapping table
///
/// This function builds account index mapping according to Solana V0 transaction spec. In V0 transactions, global account order is:
/// 1. Static accounts (static_account_keys)
/// 2. Accounts from address lookup tables, in the following order:
///    - For each lookup table (in address_table_lookups order):
///      a. All accounts corresponding to writable_indexes of that table
///      b. All accounts corresponding to readonly_indexes of that table
///
/// The index of returned Vec<LookupLocation> corresponds to (global account index - static account count)
///
/// # Parameters
/// * `plan` - Address lookup table plan list, order must match address_table_lookups in transaction
///
/// # Returns
/// Returns a lookup location list sorted by global account index
pub fn build_lookup_locations(plan: &[AddressLookupPlan]) -> Vec<LookupLocation> {
    let mut locations = Vec::new();

    // Iterate through each lookup table (maintaining transaction order)
    for entry in plan {
        // Add all writable accounts first (order required by Solana spec)
        for &idx in &entry.writable_indexes {
            locations.push(LookupLocation {
                table_account: entry.account_key,
                table_index: idx, // Index within lookup table
                writable: true,
            });
        }
    }

    // Iterate through each lookup table (maintaining transaction order)
    for entry in plan {
        // Then add all readonly accounts
        for &idx in &entry.readonly_indexes {
            locations.push(LookupLocation {
                table_account: entry.account_key,
                table_index: idx, // Index within lookup table
                writable: false,
            });
        }
    }

    locations
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use base64::Engine;
    #[allow(deprecated)]
    use solana_sdk::system_instruction;
    use solana_sdk::{
        hash::Hash,
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        transaction::Transaction,
    };

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
        assert_eq!(parsed.summary.signatures.len(), 1);
        assert_eq!(parsed.summary.static_accounts.len(), 3);
        assert_eq!(parsed.summary.instructions.len(), 1);
        assert_eq!(parsed.summary.static_accounts[0].pubkey, payer.to_string());
    }

    #[test]
    fn parse_base58_transaction() {
        let (versioned, _) = sample_transaction();
        let bytes = bincode::serialize(&versioned).unwrap();
        let base58 = bs58::encode(&bytes).into_string();

        let parsed = parse_raw_transaction(&base58).expect("parse base58");
        assert_eq!(parsed.encoding, RawTransactionEncoding::Base58);
        assert_eq!(parsed.summary.instructions.len(), 1);
    }

    #[test]
    fn test_build_lookup_locations_ordering() {
        // Create two lookup tables, each with writable and readonly indexes
        let table1 = Pubkey::new_unique();
        let table2 = Pubkey::new_unique();

        let plan = vec![
            AddressLookupPlan {
                account_key: table1,
                writable_indexes: vec![0, 1], // table1 writable indexes: 0, 1
                readonly_indexes: vec![2, 3], // table1 readonly indexes: 2, 3
            },
            AddressLookupPlan {
                account_key: table2,
                writable_indexes: vec![5, 6], // table2 writable indexes: 5, 6
                readonly_indexes: vec![7],    // table2 readonly indexes: 7
            },
        ];

        let locations = build_lookup_locations(&plan);

        // Verify order complies with Solana spec (per new implementation):
        // First all tables' writable indexes, then all tables' readonly indexes
        // Global index 0: table1[0] writable
        // Global index 1: table1[1] writable
        // Global index 2: table2[5] writable
        // Global index 3: table2[6] writable
        // Global index 4: table1[2] readonly
        // Global index 5: table1[3] readonly
        // Global index 6: table2[7] readonly

        assert_eq!(locations.len(), 7, "Should have 7 lookup accounts");

        // Verify table1 writable accounts
        assert_eq!(locations[0].table_account, table1);
        assert_eq!(locations[0].table_index, 0);
        assert_eq!(locations[0].writable, true);

        assert_eq!(locations[1].table_account, table1);
        assert_eq!(locations[1].table_index, 1);
        assert_eq!(locations[1].writable, true);

        // Verify table2 writable accounts
        assert_eq!(locations[2].table_account, table2);
        assert_eq!(locations[2].table_index, 5);
        assert_eq!(locations[2].writable, true);

        assert_eq!(locations[3].table_account, table2);
        assert_eq!(locations[3].table_index, 6);
        assert_eq!(locations[3].writable, true);

        // Verify table1 readonly accounts
        assert_eq!(locations[4].table_account, table1);
        assert_eq!(locations[4].table_index, 2);
        assert_eq!(locations[4].writable, false);

        assert_eq!(locations[5].table_account, table1);
        assert_eq!(locations[5].table_index, 3);
        assert_eq!(locations[5].writable, false);

        // Verify table2 readonly accounts
        assert_eq!(locations[6].table_account, table2);
        assert_eq!(locations[6].table_index, 7);
        assert_eq!(locations[6].writable, false);
    }

    #[test]
    fn test_build_lookup_locations_empty() {
        let locations = build_lookup_locations(&[]);
        assert_eq!(
            locations.len(),
            0,
            "Empty lookup plan should return empty locations"
        );
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

        // Writable accounts should be first
        assert_eq!(locations[0].table_index, 10);
        assert_eq!(locations[0].writable, true);

        // Readonly accounts after
        assert_eq!(locations[1].table_index, 20);
        assert_eq!(locations[1].writable, false);

        assert_eq!(locations[2].table_index, 21);
        assert_eq!(locations[2].writable, false);
    }

    #[test]
    fn test_classify_account_reference_with_lookups() {
        use solana_message::{v0, VersionedMessage};
        use solana_sdk::hash::Hash;
        use solana_sdk::message::{v0::MessageAddressTableLookup, MessageHeader};

        // Create 3 static accounts
        let static_key1 = Pubkey::new_unique();
        let static_key2 = Pubkey::new_unique();
        let static_key3 = Pubkey::new_unique();
        let static_accounts = vec![static_key1, static_key2, static_key3];

        // Create 2 lookup tables
        let lookup_table1 = Pubkey::new_unique();
        let lookup_table2 = Pubkey::new_unique();

        let address_table_lookups = vec![
            MessageAddressTableLookup {
                account_key: lookup_table1,
                writable_indexes: vec![0, 1], // 2 writable
                readonly_indexes: vec![2],    // 1 readonly
            },
            MessageAddressTableLookup {
                account_key: lookup_table2,
                writable_indexes: vec![3],    // 1 writable
                readonly_indexes: vec![4, 5], // 2 readonly
            },
        ];

        let message = VersionedMessage::V0(v0::Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 0,
            },
            recent_blockhash: Hash::default(),
            account_keys: static_accounts.clone(),
            address_table_lookups,
            instructions: vec![],
        });

        let plan = MessageAccountPlan {
            static_accounts: static_accounts.clone(),
            address_lookups: build_address_lookup_plan(&message),
        };

        let lookup_locations = build_lookup_locations(&plan.address_lookups);

        // Verify account index mapping (per new order: first all writable, then all readonly)
        // Indexes 0-2: static accounts
        let ref0 = classify_account_reference(&message, 0, &plan, &lookup_locations);
        assert_eq!(ref0.pubkey, Some(static_key1.to_string()));
        assert!(matches!(ref0.source, AccountSourceSummary::Static));

        let ref2 = classify_account_reference(&message, 2, &plan, &lookup_locations);
        assert_eq!(ref2.pubkey, Some(static_key3.to_string()));
        assert!(matches!(ref2.source, AccountSourceSummary::Static));

        // Index 3: lookup_table1[0] writable
        let ref3 = classify_account_reference(&message, 3, &plan, &lookup_locations);
        assert_eq!(ref3.index, 3);
        match &ref3.source {
            AccountSourceSummary::Lookup {
                table_account,
                lookup_index,
                writable,
            } => {
                assert_eq!(*table_account, lookup_table1.to_string());
                assert_eq!(*lookup_index, 0);
                assert_eq!(*writable, true);
            }
            _ => panic!("Expected to be Lookup source"),
        }

        // Index 4: lookup_table1[1] writable
        let ref4 = classify_account_reference(&message, 4, &plan, &lookup_locations);
        match &ref4.source {
            AccountSourceSummary::Lookup {
                table_account,
                lookup_index,
                writable,
            } => {
                assert_eq!(*table_account, lookup_table1.to_string());
                assert_eq!(*lookup_index, 1);
                assert_eq!(*writable, true);
            }
            _ => panic!("Expected to be Lookup source"),
        }

        // Index 5: lookup_table2[3] writable (new order: table2 writable after table1 writable)
        let ref5 = classify_account_reference(&message, 5, &plan, &lookup_locations);
        match &ref5.source {
            AccountSourceSummary::Lookup {
                table_account,
                lookup_index,
                writable,
            } => {
                assert_eq!(*table_account, lookup_table2.to_string());
                assert_eq!(*lookup_index, 3);
                assert_eq!(*writable, true);
            }
            _ => panic!("Expected to be Lookup source"),
        }

        // Index 6: lookup_table1[2] readonly (readonly starts after all writable complete)
        let ref6 = classify_account_reference(&message, 6, &plan, &lookup_locations);
        match &ref6.source {
            AccountSourceSummary::Lookup {
                table_account,
                lookup_index,
                writable,
            } => {
                assert_eq!(*table_account, lookup_table1.to_string());
                assert_eq!(*lookup_index, 2);
                assert_eq!(*writable, false);
            }
            _ => panic!("Expected to be Lookup source"),
        }

        // Index 7: lookup_table2[4] readonly
        let ref7 = classify_account_reference(&message, 7, &plan, &lookup_locations);
        match &ref7.source {
            AccountSourceSummary::Lookup {
                table_account,
                lookup_index,
                writable,
            } => {
                assert_eq!(*table_account, lookup_table2.to_string());
                assert_eq!(*lookup_index, 4);
                assert_eq!(*writable, false);
            }
            _ => panic!("Expected to be Lookup source"),
        }

        // Index 8: lookup_table2[5] readonly
        let ref8 = classify_account_reference(&message, 8, &plan, &lookup_locations);
        match &ref8.source {
            AccountSourceSummary::Lookup {
                table_account,
                lookup_index,
                writable,
            } => {
                assert_eq!(*table_account, lookup_table2.to_string());
                assert_eq!(*lookup_index, 5);
                assert_eq!(*writable, false);
            }
            _ => panic!("Expected to be Lookup source"),
        }

        // Verify total account count: 3 static + 6 lookup
        assert_eq!(lookup_locations.len(), 6);
    }

    #[test]
    fn test_account_ordering_edge_cases() {
        use solana_message::{v0, VersionedMessage};
        use solana_sdk::hash::Hash;
        use solana_sdk::message::{v0::MessageAddressTableLookup, MessageHeader};

        // Test case with only readonly indexes
        let static_key = Pubkey::new_unique();
        let lookup_table = Pubkey::new_unique();

        let address_table_lookups = vec![MessageAddressTableLookup {
            account_key: lookup_table,
            writable_indexes: vec![],        // No writable accounts
            readonly_indexes: vec![0, 1, 2], // Only readonly accounts
        }];

        let message = VersionedMessage::V0(v0::Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 0,
            },
            recent_blockhash: Hash::default(),
            account_keys: vec![static_key],
            address_table_lookups: address_table_lookups.clone(),
            instructions: vec![],
        });

        let plan = MessageAccountPlan {
            static_accounts: vec![static_key],
            address_lookups: build_address_lookup_plan(&message),
        };

        let lookup_locations = build_lookup_locations(&plan.address_lookups);

        // Verify order when only readonly indexes present
        assert_eq!(lookup_locations.len(), 3);

        // Index 1 should be lookup_table[0] readonly
        let ref1 = classify_account_reference(&message, 1, &plan, &lookup_locations);
        match &ref1.source {
            AccountSourceSummary::Lookup {
                lookup_index,
                writable,
                ..
            } => {
                assert_eq!(*lookup_index, 0);
                assert_eq!(*writable, false);
            }
            _ => panic!("Expected to be Lookup source"),
        }

        // Index 2 should be lookup_table[1] readonly
        let ref2 = classify_account_reference(&message, 2, &plan, &lookup_locations);
        match &ref2.source {
            AccountSourceSummary::Lookup {
                lookup_index,
                writable,
                ..
            } => {
                assert_eq!(*lookup_index, 1);
                assert_eq!(*writable, false);
            }
            _ => panic!("Expected to be Lookup source"),
        }

        // Index 3 should be lookup_table[2] readonly
        let ref3 = classify_account_reference(&message, 3, &plan, &lookup_locations);
        match &ref3.source {
            AccountSourceSummary::Lookup {
                lookup_index,
                writable,
                ..
            } => {
                assert_eq!(*lookup_index, 2);
                assert_eq!(*writable, false);
            }
            _ => panic!("Expected to be Lookup source"),
        }
    }

    #[test]
    fn test_two_writable_signers_scenario() {
        use solana_message::{v0, VersionedMessage};
        use solana_sdk::hash::Hash;
        use solana_sdk::message::{v0::MessageAddressTableLookup, MessageHeader};

        // Test scenario: 2 writable signers + lookup table
        // num_readonly_signed_accounts = 0 means all signers are writable
        let signer1 = Pubkey::new_unique();
        let signer2 = Pubkey::new_unique();
        let lookup_table = Pubkey::new_unique();

        let message = VersionedMessage::V0(v0::Message {
            header: MessageHeader {
                num_required_signatures: 2,        // 2 signers
                num_readonly_signed_accounts: 0,   // 0 readonly signers (all are writable)
                num_readonly_unsigned_accounts: 0, // 0 readonly non-signers
            },
            recent_blockhash: Hash::default(),
            account_keys: vec![signer1, signer2], // Only 2 writable signers
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: lookup_table,
                writable_indexes: vec![0, 1],
                readonly_indexes: vec![2],
            }],
            instructions: vec![],
        });

        let plan = MessageAccountPlan {
            static_accounts: vec![signer1, signer2],
            address_lookups: build_address_lookup_plan(&message),
        };

        let lookup_locations = build_lookup_locations(&plan.address_lookups);

        // Verify static accounts
        let ref0 = classify_account_reference(&message, 0, &plan, &lookup_locations);
        assert_eq!(ref0.pubkey, Some(signer1.to_string()));
        assert_eq!(ref0.signer, true, "Index 0 should be signer");
        assert_eq!(ref0.writable, true, "Index 0 should be writable");
        assert!(matches!(ref0.source, AccountSourceSummary::Static));

        let ref1 = classify_account_reference(&message, 1, &plan, &lookup_locations);
        assert_eq!(ref1.pubkey, Some(signer2.to_string()));
        assert_eq!(ref1.signer, true, "Index 1 should be signer");
        assert_eq!(ref1.writable, true, "Index 1 should be writable");
        assert!(matches!(ref1.source, AccountSourceSummary::Static));

        // Verify lookup table accounts start from index 2
        let ref2 = classify_account_reference(&message, 2, &plan, &lookup_locations);
        match &ref2.source {
            AccountSourceSummary::Lookup {
                table_account,
                lookup_index,
                writable,
            } => {
                assert_eq!(*table_account, lookup_table.to_string());
                assert_eq!(*lookup_index, 0, "Should map to lookup table index 0");
                assert_eq!(*writable, true, "Should be writable");
            }
            _ => panic!("Expected index 2 to be Lookup source"),
        }

        let ref3 = classify_account_reference(&message, 3, &plan, &lookup_locations);
        match &ref3.source {
            AccountSourceSummary::Lookup {
                lookup_index,
                writable,
                ..
            } => {
                assert_eq!(*lookup_index, 1, "Should map to lookup table index 1");
                assert_eq!(*writable, true, "Should be writable");
            }
            _ => panic!("Expected index 3 to be Lookup source"),
        }

        let ref4 = classify_account_reference(&message, 4, &plan, &lookup_locations);
        match &ref4.source {
            AccountSourceSummary::Lookup {
                lookup_index,
                writable,
                ..
            } => {
                assert_eq!(*lookup_index, 2, "Should map to lookup table index 2");
                assert_eq!(*writable, false, "Should be readonly");
            }
            _ => panic!("Expected index 4 to be Lookup source"),
        }

        // Verify total account count: 2 static + 3 lookup
        assert_eq!(lookup_locations.len(), 3);
    }

    #[test]
    fn test_mixed_signers_with_lookups() {
        use solana_message::{v0, VersionedMessage};
        use solana_sdk::hash::Hash;
        use solana_sdk::message::{v0::MessageAddressTableLookup, MessageHeader};

        // Test scenario: Mixed signers (1 writable + 1 readonly) + non-signers + lookup table
        let writable_signer = Pubkey::new_unique();
        let readonly_signer = Pubkey::new_unique();
        let writable_non_signer = Pubkey::new_unique();
        let readonly_non_signer = Pubkey::new_unique();
        let lookup_table = Pubkey::new_unique();

        let message = VersionedMessage::V0(v0::Message {
            header: MessageHeader {
                num_required_signatures: 2,        // 2 signers
                num_readonly_signed_accounts: 1,   // 1 readonly signer
                num_readonly_unsigned_accounts: 1, // 1 readonly non-signer
            },
            recent_blockhash: Hash::default(),
            // Sorted per Solana spec: writable signers -> readonly signers -> writable non-signers -> readonly non-signers
            account_keys: vec![
                writable_signer,     // Index 0
                readonly_signer,     // Index 1
                writable_non_signer, // Index 2
                readonly_non_signer, // Index 3
            ],
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: lookup_table,
                writable_indexes: vec![0],
                readonly_indexes: vec![1],
            }],
            instructions: vec![],
        });

        let plan = MessageAccountPlan {
            static_accounts: vec![
                writable_signer,
                readonly_signer,
                writable_non_signer,
                readonly_non_signer,
            ],
            address_lookups: build_address_lookup_plan(&message),
        };

        let lookup_locations = build_lookup_locations(&plan.address_lookups);

        // Verify static accounts
        let ref0 = classify_account_reference(&message, 0, &plan, &lookup_locations);
        assert_eq!(ref0.pubkey, Some(writable_signer.to_string()));
        assert_eq!(ref0.signer, true);
        assert_eq!(ref0.writable, true);

        let ref1 = classify_account_reference(&message, 1, &plan, &lookup_locations);
        assert_eq!(ref1.pubkey, Some(readonly_signer.to_string()));
        assert_eq!(ref1.signer, true);
        assert_eq!(ref1.writable, false);

        let ref2 = classify_account_reference(&message, 2, &plan, &lookup_locations);
        assert_eq!(ref2.pubkey, Some(writable_non_signer.to_string()));
        assert_eq!(ref2.signer, false);
        assert_eq!(ref2.writable, true);

        let ref3 = classify_account_reference(&message, 3, &plan, &lookup_locations);
        assert_eq!(ref3.pubkey, Some(readonly_non_signer.to_string()));
        assert_eq!(ref3.signer, false);
        assert_eq!(ref3.writable, false);

        // Verify lookup table accounts start from index 4
        let ref4 = classify_account_reference(&message, 4, &plan, &lookup_locations);
        match &ref4.source {
            AccountSourceSummary::Lookup {
                lookup_index,
                writable,
                ..
            } => {
                assert_eq!(*lookup_index, 0);
                assert_eq!(*writable, true);
            }
            _ => panic!("Expected index 4 to be Lookup source"),
        }

        let ref5 = classify_account_reference(&message, 5, &plan, &lookup_locations);
        match &ref5.source {
            AccountSourceSummary::Lookup {
                lookup_index,
                writable,
                ..
            } => {
                assert_eq!(*lookup_index, 1);
                assert_eq!(*writable, false);
            }
            _ => panic!("Expected index 5 to be Lookup source"),
        }
    }

    #[test]
    fn test_only_signers_no_lookups() {
        use solana_message::{v0, VersionedMessage};
        use solana_sdk::hash::Hash;
        use solana_sdk::message::MessageHeader;

        // Test scenario: Only signers, no lookup table
        let signer1 = Pubkey::new_unique();
        let signer2 = Pubkey::new_unique();

        let message = VersionedMessage::V0(v0::Message {
            header: MessageHeader {
                num_required_signatures: 2,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 0,
            },
            recent_blockhash: Hash::default(),
            account_keys: vec![signer1, signer2],
            address_table_lookups: vec![], // No lookup table
            instructions: vec![],
        });

        let plan = MessageAccountPlan {
            static_accounts: vec![signer1, signer2],
            address_lookups: build_address_lookup_plan(&message),
        };

        let lookup_locations = build_lookup_locations(&plan.address_lookups);

        // Verify no lookup accounts
        assert_eq!(lookup_locations.len(), 0);

        // Verify static accounts
        let ref0 = classify_account_reference(&message, 0, &plan, &lookup_locations);
        assert_eq!(ref0.pubkey, Some(signer1.to_string()));
        assert_eq!(ref0.signer, true);
        assert_eq!(ref0.writable, true);

        let ref1 = classify_account_reference(&message, 1, &plan, &lookup_locations);
        assert_eq!(ref1.pubkey, Some(signer2.to_string()));
        assert_eq!(ref1.signer, true);
        assert_eq!(ref1.writable, true);

        // Verify out-of-range index returns Unknown
        let ref2 = classify_account_reference(&message, 2, &plan, &lookup_locations);
        assert!(matches!(ref2.source, AccountSourceSummary::Unknown));
    }
}
