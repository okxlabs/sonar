pub(crate) use sonar_sim::internals::build_lookup_locations;
pub use sonar_sim::internals::{LookupLocation, MessageAccountPlan, RawTransactionEncoding};

use crate::core::rpc_client::{GetTransactionConfig, RpcClient};
use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::{Deserialize, Deserializer, Serialize};
use solana_commitment_config::CommitmentConfig;
use solana_instruction::{AccountMeta, Instruction};
use solana_message::inner_instruction::InnerInstructionsList;
use solana_message::{Message, VersionedMessage};
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_transaction::versioned::{TransactionVersion, VersionedTransaction};
use solana_transaction_status_client_types::UiTransactionEncoding;
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

impl ParsedTransaction {
    pub fn from_versioned(
        transaction: VersionedTransaction,
        encoding: RawTransactionEncoding,
    ) -> Self {
        let version = transaction.version();
        let account_plan = MessageAccountPlan::from_transaction(&transaction);
        let summary = TransactionSummary::from_transaction(&transaction, &account_plan, Vec::new());
        Self { encoding, version, transaction, summary, account_plan }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TxResolveSource {
    RawInput,
    Cache,
    Rpc,
    Instructions,
}

impl TxResolveSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RawInput => "raw_input",
            Self::Cache => "cache",
            Self::Rpc => "rpc",
            Self::Instructions => "instructions",
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InstructionInput {
    #[serde(alias = "program_id", deserialize_with = "deserialize_pubkey")]
    pub program: Pubkey,
    #[serde(default)]
    pub accounts: Vec<InstructionAccountInput>,
    #[serde(default, deserialize_with = "deserialize_instruction_data")]
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InstructionAccountInput {
    #[serde(deserialize_with = "deserialize_pubkey")]
    pub pubkey: Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
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
    crate::utils::read_cli_input(inline.as_deref(), "transaction").map_err(|e| anyhow!(e))
}

fn deserialize_pubkey<'de, D>(deserializer: D) -> std::result::Result<Pubkey, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    Pubkey::from_str(raw.trim()).map_err(serde::de::Error::custom)
}

fn deserialize_instruction_data<'de, D>(deserializer: D) -> std::result::Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    parse_optional_instruction_data(&raw).map_err(serde::de::Error::custom)
}

/// Parse optional instruction data. The `0x`/`0X` prefix is the explicit
/// switch into hex; everything else is base64. Empty / `0x` / `0X` decode
/// to empty bytes.
fn parse_optional_instruction_data(raw: &str) -> std::result::Result<Vec<u8>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || matches!(trimmed, "0x" | "0X") {
        return Ok(Vec::new());
    }
    if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
        return crate::utils::parse_hex_data(trimmed);
    }
    BASE64_STANDARD
        .decode(trimmed)
        .map_err(|e| format!("invalid base64 instruction data: {e}"))
}

pub fn is_transaction_signature(s: &str) -> bool {
    let trimmed = s.trim();
    Signature::from_str(trimmed).is_ok()
}

pub fn encode_transaction_to_base64(tx: &VersionedTransaction) -> Result<String> {
    let serialized = bincode::serialize(tx).context("Failed to serialize transaction")?;
    Ok(BASE64_STANDARD.encode(serialized))
}

pub fn parse_instruction_inputs_json(raw: &str) -> Result<Vec<InstructionInput>> {
    // Dispatch on the first non-whitespace byte so serde reports precise
    // per-field errors instead of a generic "did not match any variant" from
    // an untagged enum.
    let inputs = match raw.trim_start().as_bytes().first() {
        Some(b'[') => serde_json::from_str::<Vec<InstructionInput>>(raw)
            .context("Failed to parse instruction JSON array")?,
        Some(b'{') => vec![
            serde_json::from_str::<InstructionInput>(raw)
                .context("Failed to parse instruction JSON object")?,
        ],
        _ => anyhow::bail!(
            "Instruction JSON must be an object `{{...}}` or array `[...]` of objects"
        ),
    };
    if inputs.is_empty() {
        anyhow::bail!("Instruction input must contain at least one instruction");
    }
    Ok(inputs)
}

pub fn parse_instruction_input_dsl(raw: &str) -> Result<InstructionInput> {
    let mut program = None;
    let mut accounts = Vec::new();
    let mut data = Vec::new();

    for field in raw.split_whitespace() {
        let (name, value) = field
            .split_once('=')
            .ok_or_else(|| anyhow!("Instruction field `{field}` must use name=value syntax"))?;
        match name {
            "program" | "program_id" => {
                if program.is_some() {
                    anyhow::bail!("Instruction DSL contains duplicate `{name}` field");
                }
                program =
                    Some(Pubkey::from_str(value).with_context(|| {
                        format!("Failed to parse instruction program `{value}`")
                    })?);
            }
            "data" => {
                data = parse_instruction_data_dsl(value)
                    .with_context(|| format!("Failed to parse instruction data `{value}`"))?;
            }
            "accounts" => {
                accounts = parse_instruction_accounts_dsl(value)
                    .with_context(|| format!("Failed to parse instruction accounts `{value}`"))?;
            }
            _ => anyhow::bail!("Unknown instruction DSL field `{name}`"),
        }
    }

    let program = program.ok_or_else(|| anyhow!("Instruction DSL requires program=<PUBKEY>"))?;
    Ok(InstructionInput { program, accounts, data })
}

fn parse_instruction_data_dsl(raw: &str) -> Result<Vec<u8>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || matches!(trimmed, "0x" | "0X") {
        return Ok(Vec::new());
    }
    crate::utils::parse_hex_data(trimmed).map_err(anyhow::Error::msg)
}

fn parse_instruction_accounts_dsl(raw: &str) -> Result<Vec<InstructionAccountInput>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    trimmed.split(',').map(parse_instruction_account_dsl).collect()
}

fn parse_instruction_account_dsl(raw: &str) -> Result<InstructionAccountInput> {
    let trimmed = raw.trim();
    let (pubkey_str, flags) = trimmed.split_once(':').unwrap_or((trimmed, ""));
    if pubkey_str.is_empty() {
        anyhow::bail!("Instruction account entry is missing a pubkey");
    }
    let pubkey = Pubkey::from_str(pubkey_str)
        .with_context(|| format!("Failed to parse instruction account `{pubkey_str}`"))?;

    let mut is_signer = false;
    let mut is_writable = false;
    if trimmed.contains(':') && flags.is_empty() {
        anyhow::bail!("Instruction account `{pubkey_str}` has empty account flags");
    }
    for flag in flags.chars() {
        match flag {
            's' => is_signer = true,
            'w' => is_writable = true,
            _ => anyhow::bail!(
                "Unknown instruction account flag `{flag}` for `{pubkey_str}`; valid flags are `s` (signer) and `w` (writable)"
            ),
        }
    }

    Ok(InstructionAccountInput { pubkey, is_signer, is_writable })
}

pub fn build_transaction_from_instructions(
    payer: Pubkey,
    inputs: &[InstructionInput],
) -> Result<ParsedTransaction> {
    if inputs.is_empty() {
        anyhow::bail!("Instruction input must contain at least one instruction");
    }

    let instructions: Vec<_> = inputs
        .iter()
        .map(|input| {
            let accounts = input
                .accounts
                .iter()
                .map(|account| {
                    if account.is_writable {
                        AccountMeta::new(account.pubkey, account.is_signer)
                    } else {
                        AccountMeta::new_readonly(account.pubkey, account.is_signer)
                    }
                })
                .collect();
            Instruction { program_id: input.program, accounts, data: input.data.clone() }
        })
        .collect();

    let message = Message::new(&instructions, Some(&payer));
    let signature_count = message.header.num_required_signatures as usize;
    let transaction = VersionedTransaction {
        signatures: vec![Signature::default(); signature_count],
        message: VersionedMessage::Legacy(message),
    };

    Ok(ParsedTransaction::from_versioned(transaction, RawTransactionEncoding::Base64))
}

pub fn fetch_transaction_from_rpc(
    rpc_url: &str,
    signature: &str,
    _progress: Option<&Progress>,
) -> Result<String> {
    let parsed_sig =
        signature.parse().with_context(|| format!("Invalid signature format: {}", signature))?;

    let client = RpcClient::new(rpc_url);
    let config = GetTransactionConfig {
        encoding: UiTransactionEncoding::Base64,
        commitment: CommitmentConfig::confirmed(),
        max_supported_transaction_version: Some(0),
    };

    let response = client.get_transaction_with_config(&parsed_sig, config).map_err(|e| {
        log::error!("RPC get_transaction error: {:?}", e);
        anyhow!("Failed to fetch transaction for signature: {}. Error: {}", signature, e)
    })?;

    let tx = response
        .transaction
        .decode()
        .ok_or_else(|| anyhow!("Failed to decode transaction from RPC response"))?;
    encode_transaction_to_base64(&tx)
}

// ---------------------------------------------------------------------------
// Transaction parsing (wraps sonar-sim, adds summary)
// ---------------------------------------------------------------------------

pub fn parse_raw_transaction(raw: &str) -> Result<ParsedTransaction> {
    let sim_parsed = sonar_sim::internals::parse_raw_transaction(raw)?;
    Ok(ParsedTransaction::from_versioned(sim_parsed.transaction, sim_parsed.encoding))
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
    fn build_transaction_from_instruction_inputs_keeps_all_instructions() {
        let payer = Pubkey::new_unique();
        let program = Pubkey::new_unique();
        let first_account = Pubkey::new_unique();
        let second_account = Pubkey::new_unique();
        let inputs = vec![
            InstructionInput {
                program,
                accounts: vec![InstructionAccountInput {
                    pubkey: first_account,
                    is_signer: true,
                    is_writable: true,
                }],
                data: vec![1, 2],
            },
            InstructionInput {
                program,
                accounts: vec![InstructionAccountInput {
                    pubkey: second_account,
                    is_signer: false,
                    is_writable: false,
                }],
                data: vec![3, 4],
            },
        ];

        let parsed = build_transaction_from_instructions(payer, &inputs)
            .expect("instruction inputs should build transaction");

        assert_eq!(parsed.summary.signatures.len(), 2);
        assert_eq!(parsed.summary.static_accounts[0].pubkey, payer.to_string());
        assert_eq!(parsed.summary.instructions.len(), 2);
        assert_eq!(
            parsed.summary.instructions[0].program.pubkey.as_deref(),
            Some(program.to_string().as_str())
        );
        assert_eq!(parsed.summary.instructions[0].data.as_ref(), &[1, 2]);
        assert_eq!(parsed.summary.instructions[1].data.as_ref(), &[3, 4]);
    }

    #[test]
    fn parse_instruction_inputs_json_accepts_single_object_and_defaults() {
        let program = Pubkey::new_unique();
        let raw = format!(r#"{{"program":"{program}"}}"#);

        let inputs = parse_instruction_inputs_json(&raw).expect("single instruction parses");

        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].program, program);
        assert!(inputs[0].accounts.is_empty());
        assert!(inputs[0].data.is_empty());
    }

    #[test]
    fn parse_instruction_inputs_json_accepts_arrays_and_hex_data() {
        let first_program = Pubkey::new_unique();
        let second_program = Pubkey::new_unique();
        let account = Pubkey::new_unique();
        let raw = format!(
            r#"[{{"program":"{first_program}","data":"0x0102"}},{{"program_id":"{second_program}","accounts":[{{"pubkey":"{account}","is_signer":false,"is_writable":true}}],"data":"0x0304"}}]"#
        );

        let inputs = parse_instruction_inputs_json(&raw).expect("instruction array parses");

        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].data, vec![1, 2]);
        assert_eq!(inputs[1].program, second_program);
        assert_eq!(inputs[1].accounts[0].pubkey, account);
        assert!(!inputs[1].accounts[0].is_signer);
        assert!(inputs[1].accounts[0].is_writable);
        assert_eq!(inputs[1].data, vec![3, 4]);
    }

    #[test]
    fn parse_instruction_inputs_json_rejects_missing_account_flags() {
        let program = Pubkey::new_unique();
        let account = Pubkey::new_unique();
        // Missing both is_signer and is_writable.
        let raw = format!(
            r#"{{"program":"{program}","accounts":[{{"pubkey":"{account}"}}]}}"#
        );
        let err = parse_instruction_inputs_json(&raw)
            .expect_err("missing is_signer/is_writable should fail");
        let chain = format!("{err:#}");
        assert!(
            chain.contains("is_signer") || chain.contains("is_writable"),
            "expected serde error to mention the missing field, got: {chain}"
        );
    }

    #[test]
    fn parse_instruction_input_dsl_accepts_named_fields() {
        let program = Pubkey::new_unique();
        let signer_writable = Pubkey::new_unique();
        let readonly = Pubkey::new_unique();
        let raw = format!("data=0x0102 program={program} accounts={signer_writable}:sw,{readonly}");

        let input = parse_instruction_input_dsl(&raw).expect("dsl instruction parses");

        assert_eq!(input.program, program);
        assert_eq!(input.data, vec![1, 2]);
        assert_eq!(input.accounts.len(), 2);
        assert_eq!(input.accounts[0].pubkey, signer_writable);
        assert!(input.accounts[0].is_signer);
        assert!(input.accounts[0].is_writable);
        assert_eq!(input.accounts[1].pubkey, readonly);
        assert!(!input.accounts[1].is_signer);
        assert!(!input.accounts[1].is_writable);
    }

    #[test]
    fn parse_instruction_inputs_json_decodes_base64_when_no_0x_prefix() {
        let program = Pubkey::new_unique();
        // "world" base64-encoded; no prefix needed — anything without `0x` is base64.
        let raw = format!(r#"{{"program":"{program}","data":"d29ybGQ="}}"#);
        let inputs = parse_instruction_inputs_json(&raw).expect("base64 data parses");
        assert_eq!(inputs[0].data, b"world");
    }

    #[test]
    fn parse_instruction_inputs_json_decodes_hex_with_0x_prefix() {
        let program = Pubkey::new_unique();
        let raw = format!(r#"{{"program":"{program}","data":"0xdeadbeef"}}"#);
        let inputs = parse_instruction_inputs_json(&raw).expect("hex data parses");
        assert_eq!(inputs[0].data, vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn parse_instruction_input_dsl_accepts_hex_chars_without_prefix() {
        let program = Pubkey::new_unique();
        let raw = format!("program={program} data=f8c69e91e17587c8");
        let input = parse_instruction_input_dsl(&raw).expect("hex parses");
        assert_eq!(input.data, vec![0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x75, 0x87, 0xc8]);
    }

    #[test]
    fn parse_instruction_input_dsl_rejects_b64_prefix() {
        let program = Pubkey::new_unique();
        let raw = format!("program={program} data=b64:aGVsbG8=");
        parse_instruction_input_dsl(&raw)
            .expect_err("DSL no longer accepts base64 — `:` is not a hex character");
    }
}
