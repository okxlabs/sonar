use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bs58::decode::Error as Base58Error;
use serde::Serialize;
use solana_message::VersionedMessage;
use solana_message::inner_instruction::InnerInstructionsList;
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_transaction::versioned::{TransactionVersion, VersionedTransaction};
use std::str::FromStr;

use crate::utils::progress::Progress;

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
        Self { static_accounts, address_lookups }
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
    Lookup { table_account: String, lookup_index: u8, writable: bool },
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

pub fn read_raw_transaction(inline: Option<String>) -> Result<String> {
    if let Some(tx) = inline {
        let trimmed = tx.trim();
        if trimmed.is_empty() {
            Err(anyhow!("Raw transaction string cannot be empty"))
        } else {
            Ok(trimmed.to_owned())
        }
    } else {
        use std::io::{IsTerminal, Read};
        if !std::io::stdin().is_terminal() {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("Failed to read transaction from stdin")?;
            let trimmed = buf.trim();
            if trimmed.is_empty() {
                Err(anyhow!("No transaction data received from stdin"))
            } else {
                Ok(trimmed.to_owned())
            }
        } else {
            Err(anyhow!(
                "No transaction provided. Pass TX as a positional argument or pipe via stdin"
            ))
        }
    }
}

pub fn is_transaction_signature(s: &str) -> bool {
    let trimmed = s.trim();
    Signature::from_str(trimmed).is_ok()
}

pub fn fetch_transaction_from_rpc(
    rpc_url: &str,
    signature: &str,
    progress: Option<&Progress>,
) -> Result<String> {
    use crate::core::account_loader::AccountLoader;

    let loader = AccountLoader::new(rpc_url.to_string(), None, false, progress.cloned())?;
    let tx = loader.fetch_transaction_by_signature(signature)?;

    let serialized = bincode::serialize(&tx).context("Failed to serialize transaction")?;

    Ok(BASE64_STANDARD.encode(serialized))
}

pub fn parse_raw_transaction(raw: &str) -> Result<ParsedTransaction> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("Raw transaction string is empty"));
    }

    let mut errors = Vec::new();

    for encoding in [RawTransactionEncoding::Base64, RawTransactionEncoding::Base58] {
        match decode_bytes(trimmed, encoding) {
            Ok(bytes) => match bincode::deserialize::<VersionedTransaction>(&bytes) {
                Ok(transaction) => {
                    let version = transaction.version();
                    let account_plan = MessageAccountPlan::from_transaction(&transaction);
                    let summary = TransactionSummary::from_transaction(
                        &transaction,
                        &account_plan,
                        Vec::new(),
                    );
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

    let merged = errors.into_iter().map(|err| err.to_string()).collect::<Vec<_>>().join("； ");
    Err(anyhow!("Failed to parse raw transaction: {merged}"))
}

pub fn parse_transaction_input(
    input: &str,
    rpc_url: &str,
    progress: Option<&Progress>,
) -> Result<ParsedTransaction> {
    match parse_raw_transaction(input) {
        Ok(parsed) => Ok(parsed),
        Err(raw_err) => {
            if !is_transaction_signature(input) {
                return Err(anyhow!(
                    "Failed to parse transaction input.\n- Raw parse failed: {raw_err}\n- Signature fallback skipped: input does not look like a transaction signature"
                ));
            }

            log::info!(
                "Raw parse failed; input looks like a transaction signature, fetching from RPC..."
            );
            let fetched = fetch_transaction_from_rpc(rpc_url, input, progress).map_err(|fetch_err| {
                anyhow!(
                    "Failed to parse transaction input.\n- Raw parse failed: {raw_err}\n- Signature fetch failed: {fetch_err}"
                )
            })?;

            parse_raw_transaction(&fetched).map_err(|fetched_parse_err| {
                anyhow!(
                    "Failed to parse transaction input.\n- Raw parse failed: {raw_err}\n- Signature fetch succeeded but parsing fetched transaction failed: {fetched_parse_err}"
                )
            })
        }
    }
}

pub fn collect_account_plan(tx: &VersionedTransaction) -> MessageAccountPlan {
    MessageAccountPlan::from_transaction(tx)
}

/// Parse multiple transaction inputs, each using auto strategy:
/// try raw parse first, then fallback to signature fetch when applicable.
pub fn parse_multi_raw_transactions(
    raws: &[String],
    rpc_url: &str,
    progress: Option<&Progress>,
) -> Result<Vec<ParsedTransaction>> {
    raws.iter()
        .enumerate()
        .map(|(index, raw)| {
            parse_transaction_input(raw, rpc_url, progress)
                .with_context(|| format!("Failed to parse transaction {}", index + 1))
        })
        .collect()
}

fn decode_bytes(input: &str, encoding: RawTransactionEncoding) -> Result<Vec<u8>> {
    match encoding {
        RawTransactionEncoding::Base58 => {
            bs58::decode(input).into_vec().map_err(|err| map_base58_error(input, err))
        }
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
        anyhow!(
            "{base_message}. Base64 characteristic characters detected, you may need to try Base64 encoding"
        )
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
pub(crate) fn classify_account_reference(
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
///    a. All accounts corresponding to writable_indexes from all address lookup tables (in address_table_lookups order)
///    b. All accounts corresponding to readonly_indexes from all address lookup tables (in address_table_lookups order)
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
    fn is_transaction_signature_uses_strict_signature_parser() {
        let signature = "3PtGYH77LhhQqTXP4SmDVJ85hmDieWsgXCUbn14v7gYyVYPjZzygUQhTk3bSTYnfA48vCM1rmWY7zWL3j1EVKmEy";
        assert!(is_transaction_signature(signature));
        assert!(!is_transaction_signature("invalid-signature"));
    }

    #[test]
    fn auto_mode_reports_raw_and_signature_failure_branches() {
        let signature = "3PtGYH77LhhQqTXP4SmDVJ85hmDieWsgXCUbn14v7gYyVYPjZzygUQhTk3bSTYnfA48vCM1rmWY7zWL3j1EVKmEy";
        let err = parse_transaction_input(signature, "not a url", None)
            .expect_err("auto mode should fail when rpc url is invalid");
        let message = err.to_string();
        assert!(message.contains("Raw parse failed"));
        assert!(message.contains("Signature fetch failed"));
    }

    #[test]
    fn parse_transaction_input_reports_fallback_skipped_for_non_signature() {
        let err = parse_transaction_input("not-a-signature", "http://localhost:8899", None)
            .expect_err("non-signature should skip signature fallback");
        let message = err.to_string();
        assert!(message.contains("Raw parse failed"));
        assert!(message.contains("Signature fallback skipped"));
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
        assert_eq!(locations.len(), 0, "Empty lookup plan should return empty locations");
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
}
