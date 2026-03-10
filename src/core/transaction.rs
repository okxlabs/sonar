pub(crate) use sonar_sim::build_lookup_locations;
pub use sonar_sim::{LookupLocation, MessageAccountPlan, RawTransactionEncoding};

#[cfg(test)]
use sonar_sim::AddressLookupPlan;

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::Serialize;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_commitment_config::CommitmentConfig;
use solana_message::VersionedMessage;
use solana_message::inner_instruction::InnerInstructionsList;
use solana_signature::Signature;
use solana_transaction::versioned::{TransactionVersion, VersionedTransaction};
use solana_transaction_status_client_types::UiTransactionEncoding;
use std::io::{IsTerminal, Read};
use std::str::FromStr;

use crate::utils::progress::Progress;

// ---------------------------------------------------------------------------
// CLI-specific ParsedTransaction (adds `summary` field not present in sonar-sim)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ParsedTransaction {
    pub encoding: RawTransactionEncoding,
    pub version: TransactionVersion,
    pub transaction: VersionedTransaction,
    pub summary: TransactionSummary,
    pub account_plan: MessageAccountPlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TxResolveSource {
    RawInput,
    Cache,
    Rpc,
}

impl TxResolveSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RawInput => "raw_input",
            Self::Cache => "cache",
            Self::Rpc => "rpc",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedTxInput {
    pub original_input: String,
    pub raw_tx_base64: String,
    pub parsed_tx: ParsedTransaction,
    pub source: TxResolveSource,
}

#[derive(Debug, Clone)]
pub struct TxInputResolver {
    rpc_url: String,
    cache_location: Option<crate::core::cache::CacheLocation>,
}

// ---------------------------------------------------------------------------
// Transaction summary types (CLI-only, String-based for human-readable output)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// CLI-only I/O functions
// ---------------------------------------------------------------------------

pub fn read_raw_transaction(inline: Option<String>) -> Result<String> {
    if let Some(tx) = inline {
        let trimmed = tx.trim();
        if trimmed.is_empty() {
            Err(anyhow!("Raw transaction string cannot be empty"))
        } else {
            Ok(trimmed.to_owned())
        }
    } else if !std::io::stdin().is_terminal() {
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
        Err(anyhow!("No transaction provided. Pass TX as a positional argument or pipe via stdin"))
    }
}

pub fn is_transaction_signature(s: &str) -> bool {
    let trimmed = s.trim();
    Signature::from_str(trimmed).is_ok()
}

pub fn encode_transaction_to_base64(tx: &VersionedTransaction) -> Result<String> {
    let serialized = bincode::serialize(tx).context("Failed to serialize transaction")?;
    Ok(BASE64_STANDARD.encode(serialized))
}

pub fn fetch_transaction_from_rpc(
    rpc_url: &str,
    signature: &str,
    _progress: Option<&Progress>,
) -> Result<String> {
    let parsed_sig =
        signature.parse().with_context(|| format!("Invalid signature format: {}", signature))?;

    let client = RpcClient::new(rpc_url);
    let config = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::Base64),
        commitment: Some(CommitmentConfig::confirmed()),
        max_supported_transaction_version: Some(0),
    };

    let response = client.get_transaction_with_config(&parsed_sig, config).map_err(|e| {
        log::error!("RPC get_transaction error: {:?}", e);
        anyhow!("Failed to fetch transaction for signature: {}. Error: {}", signature, e)
    })?;

    let tx = response
        .transaction
        .transaction
        .decode()
        .ok_or_else(|| anyhow!("Failed to decode transaction from RPC response"))?;
    encode_transaction_to_base64(&tx)
}

// ---------------------------------------------------------------------------
// Transaction parsing (wraps sonar-sim, adds summary)
// ---------------------------------------------------------------------------

pub fn parse_raw_transaction(raw: &str) -> Result<ParsedTransaction> {
    let sim_parsed = sonar_sim::parse_raw_transaction(raw)?;
    let summary = TransactionSummary::from_transaction(
        &sim_parsed.transaction,
        &sim_parsed.account_plan,
        Vec::new(),
    );
    Ok(ParsedTransaction {
        encoding: sim_parsed.encoding,
        version: sim_parsed.version,
        transaction: sim_parsed.transaction,
        summary,
        account_plan: sim_parsed.account_plan,
    })
}

impl TxInputResolver {
    pub fn new(
        rpc_url: impl Into<String>,
        cache_location: Option<crate::core::cache::CacheLocation>,
    ) -> Self {
        Self { rpc_url: rpc_url.into(), cache_location }
    }

    pub fn resolve_one(&self, input: &str, progress: Option<&Progress>) -> Result<ResolvedTxInput> {
        match parse_raw_transaction(input) {
            Ok(parsed_tx) => Ok(ResolvedTxInput {
                original_input: input.to_string(),
                raw_tx_base64: encode_transaction_to_base64(&parsed_tx.transaction)?,
                parsed_tx,
                source: TxResolveSource::RawInput,
            }),
            Err(raw_err) => {
                if !is_transaction_signature(input) {
                    return Err(anyhow!(
                        "Failed to parse transaction input.\n- Raw parse failed: {raw_err}\n- Signature fallback skipped: input does not look like a transaction signature"
                    ));
                }

                let signature = input.trim();
                if let Some(cached) = self.lookup_cached_raw_tx(signature) {
                    match parse_raw_transaction(&cached) {
                        Ok(parsed_tx) => {
                            return Ok(ResolvedTxInput {
                                original_input: signature.to_string(),
                                raw_tx_base64: cached,
                                parsed_tx,
                                source: TxResolveSource::Cache,
                            });
                        }
                        Err(cache_parse_err) => {
                            log::warn!(
                                "Cached raw transaction parse failed for signature {}: {:#}",
                                signature,
                                cache_parse_err
                            );
                        }
                    }
                }

                log::info!(
                    "Raw parse failed; input looks like a transaction signature, fetching from RPC..."
                );
                let fetched =
                    fetch_transaction_from_rpc(&self.rpc_url, signature, progress).map_err(
                        |fetch_err| {
                            anyhow!(
                                "Failed to parse transaction input.\n- Raw parse failed: {raw_err}\n- Signature fetch failed: {fetch_err}"
                            )
                        },
                    )?;

                let parsed_tx = parse_raw_transaction(&fetched).map_err(|fetched_parse_err| {
                    anyhow!(
                        "Failed to parse transaction input.\n- Raw parse failed: {raw_err}\n- Signature fetch succeeded but parsing fetched transaction failed: {fetched_parse_err}"
                    )
                })?;

                Ok(ResolvedTxInput {
                    original_input: signature.to_string(),
                    raw_tx_base64: fetched,
                    parsed_tx,
                    source: TxResolveSource::Rpc,
                })
            }
        }
    }

    pub fn resolve_many(
        &self,
        inputs: &[String],
        progress: Option<&Progress>,
    ) -> Result<Vec<ResolvedTxInput>> {
        inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                self.resolve_one(input, progress)
                    .with_context(|| format!("Failed to parse transaction {}", index + 1))
            })
            .collect()
    }

    fn lookup_cached_raw_tx(&self, signature: &str) -> Option<String> {
        use crate::core::cache::CacheLocation;
        let dir = match self.cache_location.as_ref()? {
            CacheLocation::Auto(root) => root.join(signature.trim()),
            CacheLocation::Explicit(dir) => dir.clone(),
        };
        let meta = crate::core::cache::read_meta_json(&dir).ok()?;
        meta.transactions
            .iter()
            .find(|tx| tx.input.trim() == signature.trim())
            .or_else(|| meta.transactions.first())
            .map(|tx| tx.raw_tx.clone())
    }
}

// ---------------------------------------------------------------------------
// TransactionSummary construction
// ---------------------------------------------------------------------------

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

/// Classifies account index as static account or lookup table account (CLI String-based version).
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

#[cfg(test)]
mod tests {
    use std::fs;

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
    fn tx_input_resolver_reports_raw_and_signature_failure_branches() {
        let signature = "3PtGYH77LhhQqTXP4SmDVJ85hmDieWsgXCUbn14v7gYyVYPjZzygUQhTk3bSTYnfA48vCM1rmWY7zWL3j1EVKmEy";
        let cache_root =
            std::env::temp_dir().join(format!("sonar-resolver-empty-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cache_root);
        let resolver = TxInputResolver::new(
            "not a url",
            Some(crate::core::cache::CacheLocation::Auto(cache_root.clone())),
        );
        let err = resolver
            .resolve_one(signature, None)
            .expect_err("auto mode should fail when rpc url is invalid");
        let message = err.to_string();
        assert!(message.contains("Raw parse failed"));
        assert!(message.contains("Signature fetch failed"));
        let _ = std::fs::remove_dir_all(&cache_root);
    }

    #[test]
    fn tx_input_resolver_reports_fallback_skipped_for_non_signature() {
        let resolver = TxInputResolver::new(
            "http://localhost:8899",
            Some(crate::core::cache::CacheLocation::Auto(std::env::temp_dir())),
        );
        let err = resolver
            .resolve_one("not-a-signature", None)
            .expect_err("non-signature should skip signature fallback");
        let message = err.to_string();
        assert!(message.contains("Raw parse failed"));
        assert!(message.contains("Signature fallback skipped"));
    }

    #[test]
    fn tx_input_resolver_prefers_cache_for_signature() {
        let (versioned, _) = sample_transaction();
        let bytes = bincode::serialize(&versioned).unwrap();
        let raw_base64 = BASE64_STANDARD.encode(&bytes);
        let signature = "3PtGYH77LhhQqTXP4SmDVJ85hmDieWsgXCUbn14v7gYyVYPjZzygUQhTk3bSTYnfA48vCM1rmWY7zWL3j1EVKmEy";

        let cache_root =
            std::env::temp_dir().join(format!("sonar-resolver-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&cache_root);
        let cache_dir = cache_root.join(signature);

        crate::core::cache::write_meta_json(
            &cache_dir,
            &crate::core::cache::CacheMeta {
                created_at: "2026-02-22T10:00:00Z".to_string(),
                sonar_version: "0.3.0".to_string(),
                cache_type: "single".to_string(),
                transactions: vec![crate::core::cache::CacheTransaction {
                    input: signature.to_string(),
                    raw_tx: raw_base64.clone(),
                    resolved_from: "rpc".to_string(),
                }],
                rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
                account_count: 1,
            },
        )
        .unwrap();

        let resolver = TxInputResolver::new(
            "not a url",
            Some(crate::core::cache::CacheLocation::Auto(cache_root.clone())),
        );
        let resolved = resolver.resolve_many(&[signature.to_string()], None).unwrap();

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].source, TxResolveSource::Cache);
        assert_eq!(resolved[0].original_input, signature);
        assert_eq!(resolved[0].raw_tx_base64, raw_base64);

        let _ = fs::remove_dir_all(&cache_root);
    }

    #[test]
    fn tx_input_resolver_marks_raw_source() {
        let (versioned, _) = sample_transaction();
        let bytes = bincode::serialize(&versioned).unwrap();
        let raw_base64 = BASE64_STANDARD.encode(&bytes);

        let resolver = TxInputResolver::new(
            "http://127.0.0.1:1",
            Some(crate::core::cache::CacheLocation::Auto(std::env::temp_dir())),
        );
        let resolved = resolver.resolve_many(std::slice::from_ref(&raw_base64), None).unwrap();

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].source, TxResolveSource::RawInput);
        assert_eq!(resolved[0].raw_tx_base64, raw_base64);
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

        assert_eq!(locations.len(), 7, "Should have 7 lookup accounts");

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

        assert_eq!(locations[0].table_index, 10);
        assert!(locations[0].writable);

        assert_eq!(locations[1].table_index, 20);
        assert!(!locations[1].writable);

        assert_eq!(locations[2].table_index, 21);
        assert!(!locations[2].writable);
    }
}
