use std::collections::HashMap;
use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::{Serialize, Serializer};
use solana_account::{AccountSharedData, WritableAccount};
use solana_pubkey::Pubkey;
use solana_transaction::versioned::TransactionVersion;

use crate::core::{
    account_loader::{ResolvedAccounts, ResolvedLookup},
    balance_changes::{compute_sol_changes, compute_token_changes, extract_mint_decimals_combined},
    executor::{ExecutionStatus, SimulationResult},
    funding::PreparedTokenFunding,
    transaction::{AccountReferenceSummary, AccountSourceSummary, ParsedTransaction},
    types::{Funding, Replacement},
};
use crate::parsers::instruction::{
    ParsedInstruction, ParserRegistry, anchor_idl::is_anchor_cpi_event,
};
use sonar_sim::SimulationMetadata;

use super::BalanceChangeOptions;

#[derive(Serialize)]
pub(super) struct Report {
    pub(super) transaction: TransactionSection,
    pub(super) simulation: SimulationSection,
    replacements: Vec<ReplacementSection>,
    fundings: Vec<FundingSection>,
    token_fundings: Vec<TokenFundingSection>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) sol_balance_changes: Vec<SolBalanceChangeSection>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) token_balance_changes: Vec<TokenBalanceChangeSection>,
}

#[derive(Serialize)]
pub(super) struct SolBalanceChangeSection {
    pub(super) account: String,
    pub(super) before: u64,
    pub(super) after: u64,
    pub(super) change: i128,
    pub(super) change_sol: f64,
}

#[derive(Serialize)]
pub(super) struct TokenBalanceChangeSection {
    pub(super) owner: String,
    pub(super) token_account: String,
    pub(super) mint: String,
    pub(super) before: u64,
    pub(super) after: u64,
    pub(super) change: i128,
    pub(super) decimals: u8,
    pub(super) ui_change: f64,
}

/// Report structure for bundle simulation (multiple transactions).
#[derive(Serialize)]
pub(super) struct BundleReport {
    pub(super) transactions: Vec<BundleTransactionReport>,
    replacements: Vec<ReplacementSection>,
    fundings: Vec<FundingSection>,
    token_fundings: Vec<TokenFundingSection>,
    /// SOL balance changes for the entire bundle (first tx pre -> last tx post)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) sol_balance_changes: Vec<SolBalanceChangeSection>,
    /// Token balance changes for the entire bundle (first tx pre -> last tx post)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) token_balance_changes: Vec<TokenBalanceChangeSection>,
}

#[derive(Serialize)]
pub(super) struct BundleTransactionReport {
    pub(super) index: usize,
    pub(super) transaction: TransactionSection,
    pub(super) simulation: SimulationSection,
}

impl BundleReport {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn from_sources(
        parsed_txs: &[ParsedTransaction],
        resolved: &ResolvedAccounts,
        simulations: &[SimulationResult],
        replacements: &[Replacement],
        fundings: &[Funding],
        token_fundings: &[PreparedTokenFunding],
        parser_registry: &mut ParserRegistry,
        verify_signatures: bool,
        balance_opts: BalanceChangeOptions,
    ) -> Self {
        let resolver = LookupResolver::new(resolved.lookup_details());

        let transactions = parsed_txs
            .iter()
            .zip(simulations)
            .enumerate()
            .map(|(index, (parsed, simulation))| {
                let transaction = TransactionSection::from_sources(
                    parsed,
                    resolved,
                    &resolver,
                    parser_registry,
                    verify_signatures,
                );
                let simulation_section = SimulationSection::from_result(simulation);

                BundleTransactionReport { index, transaction, simulation: simulation_section }
            })
            .collect();

        let replacements = replacements.iter().map(replacement_to_section).collect();

        let fundings = fundings
            .iter()
            .map(|entry| FundingSection {
                pubkey: entry.pubkey.to_string(),
                amount_lamports: entry.amount_lamports,
            })
            .collect();

        let token_fundings = token_fundings
            .iter()
            .map(|entry| TokenFundingSection {
                account: entry.account.to_string(),
                mint: entry.mint.to_string(),
                decimals: entry.decimals,
                ui_amount: entry.ui_amount,
                amount_raw: entry.amount_raw,
            })
            .collect();

        // Compute overall bundle balance changes (first tx pre -> last successful tx post)
        let (sol_balance_changes, token_balance_changes) =
            if balance_opts.show_balance_change && !simulations.is_empty() {
                compute_bundle_overall_balance_changes(resolved, simulations, balance_opts)
            } else {
                (Vec::new(), Vec::new())
            };

        Self {
            transactions,
            replacements,
            fundings,
            token_fundings,
            sol_balance_changes,
            token_balance_changes,
        }
    }
}

impl Report {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn from_sources(
        parsed: &ParsedTransaction,
        resolved: &ResolvedAccounts,
        simulation: &SimulationResult,
        replacements: &[Replacement],
        fundings: &[Funding],
        token_fundings: &[PreparedTokenFunding],
        parser_registry: &mut ParserRegistry,
        verify_signatures: bool,
        balance_opts: BalanceChangeOptions,
    ) -> Self {
        let resolver = LookupResolver::new(resolved.lookup_details());
        let transaction = TransactionSection::from_sources(
            parsed,
            resolved,
            &resolver,
            parser_registry,
            verify_signatures,
        );
        let simulation_section = SimulationSection::from_result(simulation);
        let replacements = replacements.iter().map(replacement_to_section).collect();
        // Compute balance changes before fundings is shadowed below
        let (sol_balance_changes, token_balance_changes) =
            if matches!(simulation.status, ExecutionStatus::Succeeded)
                && balance_opts.show_balance_change
            {
                compute_balance_changes_for_single_tx(resolved, simulation, fundings, balance_opts)
            } else {
                (Vec::new(), Vec::new())
            };

        let fundings = fundings
            .iter()
            .map(|entry| FundingSection {
                pubkey: entry.pubkey.to_string(),
                amount_lamports: entry.amount_lamports,
            })
            .collect();
        let token_fundings = token_fundings
            .iter()
            .map(|entry| TokenFundingSection {
                account: entry.account.to_string(),
                mint: entry.mint.to_string(),
                decimals: entry.decimals,
                ui_amount: entry.ui_amount,
                amount_raw: entry.amount_raw,
            })
            .collect();

        Self {
            transaction,
            simulation: simulation_section,
            replacements,
            fundings,
            token_fundings,
            sol_balance_changes,
            token_balance_changes,
        }
    }
}

/// Compute balance changes for single transaction mode.
/// Uses resolved.accounts as pre-state and simulation.post_accounts as post-state.
/// When SOL fundings are present, applies them to the pre-state so that the
/// balance change only reflects the transaction's effect, not the funding itself.
fn compute_balance_changes_for_single_tx(
    resolved: &ResolvedAccounts,
    simulation: &SimulationResult,
    fundings: &[Funding],
    balance_opts: BalanceChangeOptions,
) -> (Vec<SolBalanceChangeSection>, Vec<TokenBalanceChangeSection>) {
    let mut sol_changes = Vec::new();
    let mut token_changes = Vec::new();

    if balance_opts.show_balance_change {
        // Build pre_accounts with SOL fundings applied so pre/post baselines match.
        let funded_accounts;
        let pre_accounts = if fundings.is_empty() {
            &resolved.accounts
        } else {
            let mut accounts = resolved.accounts.clone();
            for funding in fundings {
                let lamports = funding.amount_lamports;
                if let Some(account) = accounts.get_mut(&funding.pubkey) {
                    account.set_lamports(lamports);
                } else {
                    let system_program_id = solana_sdk_ids::system_program::id();
                    accounts.insert(
                        funding.pubkey,
                        AccountSharedData::new(lamports, 0, &system_program_id),
                    );
                }
            }
            funded_accounts = accounts;
            &funded_accounts
        };

        let changes = compute_sol_changes(pre_accounts, &simulation.post_accounts);
        sol_changes = changes
            .into_iter()
            .map(|c| SolBalanceChangeSection {
                account: c.account.to_string(),
                before: c.before,
                after: c.after,
                change: c.change,
                change_sol: c.change as f64 / 1_000_000_000.0,
            })
            .collect();

        let mint_decimals = extract_mint_decimals_combined(pre_accounts, &simulation.post_accounts);
        let changes =
            compute_token_changes(pre_accounts, &simulation.post_accounts, &mint_decimals);
        token_changes = changes
            .into_iter()
            .map(|c| {
                let divisor = 10f64.powi(c.decimals as i32);
                TokenBalanceChangeSection {
                    owner: c.owner.to_string(),
                    token_account: c.account.to_string(),
                    mint: c.mint.to_string(),
                    before: c.before,
                    after: c.after,
                    change: c.change,
                    decimals: c.decimals,
                    ui_change: c.change as f64 / divisor,
                }
            })
            .collect();
    }

    (sol_changes, token_changes)
}

/// Compute overall balance changes for the entire bundle.
/// Only computes when ALL transactions in the bundle succeeded.
/// Merges pre/post accounts from all transactions to capture the complete picture:
/// - pre_accounts: earliest state for each account (first occurrence across all txs)
/// - post_accounts: latest state for each account (last occurrence across all txs)
fn compute_bundle_overall_balance_changes(
    resolved: &ResolvedAccounts,
    simulations: &[SimulationResult],
    balance_opts: BalanceChangeOptions,
) -> (Vec<SolBalanceChangeSection>, Vec<TokenBalanceChangeSection>) {
    use solana_account::AccountSharedData;

    if !balance_opts.show_balance_change || simulations.is_empty() {
        return (Vec::new(), Vec::new());
    }

    // Only compute balance changes if ALL transactions succeeded
    let all_succeeded =
        simulations.iter().all(|sim| matches!(sim.status, ExecutionStatus::Succeeded));
    if !all_succeeded {
        return (Vec::new(), Vec::new());
    }

    // Merge pre_accounts from all simulations: keep the earliest state for each account.
    // Iterating in order means the first time we see an account is its true initial state
    // before the bundle started.
    let mut pre_accounts: HashMap<Pubkey, AccountSharedData> = HashMap::new();
    for sim in simulations {
        for (k, v) in &sim.pre_accounts {
            pre_accounts.entry(*k).or_insert_with(|| v.clone());
        }
    }

    // Merge post_accounts from all simulations: keep the latest state for each account.
    // Always overwrite so the last transaction's state wins, reflecting the final
    // state after the entire bundle.
    let mut post_accounts: HashMap<Pubkey, AccountSharedData> = HashMap::new();
    for sim in simulations {
        for (k, v) in &sim.post_accounts {
            post_accounts.insert(*k, v.clone());
        }
    }

    // Compute SOL balance changes
    let sol_changes: Vec<SolBalanceChangeSection> =
        compute_sol_changes(&pre_accounts, &post_accounts)
            .into_iter()
            .map(|c| SolBalanceChangeSection {
                account: c.account.to_string(),
                before: c.before,
                after: c.after,
                change: c.change,
                change_sol: c.change as f64 / 1_000_000_000.0,
            })
            .collect();

    // Extract mint decimals from both resolved accounts and merged post accounts
    let mint_decimals = extract_mint_decimals_combined(&resolved.accounts, &post_accounts);
    let token_changes: Vec<TokenBalanceChangeSection> =
        compute_token_changes(&pre_accounts, &post_accounts, &mint_decimals)
            .into_iter()
            .map(|c| {
                let divisor = 10f64.powi(c.decimals as i32);
                TokenBalanceChangeSection {
                    owner: c.owner.to_string(),
                    token_account: c.account.to_string(),
                    mint: c.mint.to_string(),
                    before: c.before,
                    after: c.after,
                    change: c.change,
                    decimals: c.decimals,
                    ui_change: c.change as f64 / divisor,
                }
            })
            .collect();

    (sol_changes, token_changes)
}

#[derive(Serialize)]
pub(super) struct TransactionSection {
    pub(super) encoding: String,
    pub(super) version: String,
    pub(super) size_bytes: usize,
    pub(super) signatures: Vec<String>,
    pub(super) recent_blockhash: String,
    pub(super) static_accounts: Vec<AccountEntry>,
    pub(super) lookups: Vec<LookupSection>,
    pub(super) instructions: Vec<InstructionSection>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(super) verify_signatures: bool,
}

impl TransactionSection {
    pub(super) fn from_sources(
        parsed: &ParsedTransaction,
        resolved: &ResolvedAccounts,
        resolver: &LookupResolver,
        parser_registry: &mut ParserRegistry,
        verify_signatures: bool,
    ) -> Self {
        let encoding = match parsed.encoding {
            crate::core::transaction::RawTransactionEncoding::Base58 => "base58",
            crate::core::transaction::RawTransactionEncoding::Base64 => "base64",
        }
        .to_string();

        let version = match parsed.version {
            TransactionVersion::Legacy(_) => "legacy".to_string(),
            TransactionVersion::Number(v) => format!("v{v}"),
        };

        let static_accounts = parsed
            .summary
            .static_accounts
            .iter()
            .map(|entry| AccountEntry {
                index: entry.index,
                pubkey: entry.pubkey.clone(),
                signer: entry.signer,
                writable: entry.writable,
            })
            .collect();

        let instructions = parsed
            .summary
            .instructions
            .iter()
            .map(|ix| {
                InstructionSection::from_summary(
                    ix,
                    resolver,
                    &parsed.summary.inner_instructions,
                    parsed,
                    parser_registry,
                )
            })
            .collect();

        let lookups = resolved.lookups.iter().map(LookupSection::from_lookup).collect();
        let size_bytes =
            bincode::serialize(&parsed.transaction).map(|serialized| serialized.len()).unwrap_or(0);

        Self {
            encoding,
            version,
            size_bytes,
            signatures: parsed.summary.signatures.clone(),
            recent_blockhash: parsed.summary.recent_blockhash.clone(),
            static_accounts,
            lookups,
            instructions,
            verify_signatures,
        }
    }
}

#[derive(Serialize)]
pub(super) struct AccountEntry {
    pub(super) index: usize,
    pub(super) pubkey: String,
    pub(super) signer: bool,
    pub(super) writable: bool,
}

#[derive(Serialize)]
pub(super) struct LookupSection {
    pub(super) account_key: String,
    pub(super) writable: Vec<LookupAddressEntry>,
    pub(super) readonly: Vec<LookupAddressEntry>,
}

impl LookupSection {
    fn from_lookup(lookup: &ResolvedLookup) -> Self {
        let writable = lookup
            .writable_indexes
            .iter()
            .zip(&lookup.writable_addresses)
            .map(|(idx, key)| LookupAddressEntry { index: *idx, pubkey: key.to_string() })
            .collect();
        let readonly = lookup
            .readonly_indexes
            .iter()
            .zip(&lookup.readonly_addresses)
            .map(|(idx, key)| LookupAddressEntry { index: *idx, pubkey: key.to_string() })
            .collect();

        Self { account_key: lookup.account_key.to_string(), writable, readonly }
    }
}

#[derive(Serialize)]
pub(super) struct LookupAddressEntry {
    pub(super) index: u8,
    pub(super) pubkey: String,
}

#[derive(Serialize)]
pub(super) struct InstructionSection {
    pub(super) index: usize,
    pub(super) program: InstructionAccountEntry,
    pub(super) accounts: Vec<InstructionAccountEntry>,
    #[serde(serialize_with = "serialize_bytes_as_hex")]
    pub(super) data: Box<[u8]>,
    pub(super) parsed: Option<ParsedInstruction>,
    pub(super) inner_instructions: Vec<InnerInstructionSection>,
}

impl InstructionSection {
    fn from_summary(
        summary: &crate::core::transaction::InstructionSummary,
        resolver: &LookupResolver,
        inner_instructions_list: &[solana_message::inner_instruction::InnerInstructions],
        parsed: &ParsedTransaction,
        parser_registry: &mut ParserRegistry,
    ) -> Self {
        let program =
            InstructionAccountEntry::from_reference_with_resolver(&summary.program, Some(resolver));
        let accounts = summary
            .accounts
            .iter()
            .map(|account| {
                InstructionAccountEntry::from_reference_with_resolver(account, Some(resolver))
            })
            .collect();

        // Try to parse the instruction
        let parsed_instruction = if let Some(program_pubkey) = &summary.program.pubkey {
            if let Ok(program_id) = Pubkey::from_str(program_pubkey) {
                parser_registry.parse_instruction(summary, &program_id)
            } else {
                None
            }
        } else {
            None
        };

        let inner_instructions = if summary.index < inner_instructions_list.len() {
            inner_instructions_list[summary.index]
                .iter()
                .enumerate()
                .map(|(inner_idx, inner_ix)| {
                    InnerInstructionSection::from_inner_instruction(
                        inner_ix,
                        resolver,
                        &format!("{}.{}", summary.index + 1, inner_idx + 1),
                        parsed,
                        parser_registry,
                    )
                })
                .collect()
        } else {
            Vec::new()
        };

        Self {
            index: summary.index,
            program,
            accounts,
            data: summary.data.clone(),
            parsed: parsed_instruction,
            inner_instructions,
        }
    }
}

#[derive(Serialize)]
pub(super) struct InnerInstructionSection {
    pub(super) label: String,
    pub(super) program: InstructionAccountEntry,
    pub(super) accounts: Vec<InstructionAccountEntry>,
    #[serde(serialize_with = "serialize_bytes_as_hex")]
    pub(super) data: Box<[u8]>,
    pub(super) parsed: Option<ParsedInstruction>,
}

fn parse_inner_instruction_as_regular(
    inner_ix: &solana_message::inner_instruction::InnerInstruction,
    message: &solana_message::VersionedMessage,
    account_plan: &crate::core::transaction::MessageAccountPlan,
    lookup_locations: &[crate::core::transaction::LookupLocation],
    parser_registry: &mut ParserRegistry,
    program_id: &Pubkey,
) -> Option<ParsedInstruction> {
    let inner_accounts: Vec<crate::core::transaction::AccountReferenceSummary> = inner_ix
        .instruction
        .accounts
        .iter()
        .map(|account_index| {
            crate::core::transaction::classify_account_reference(
                message,
                *account_index as usize,
                account_plan,
                lookup_locations,
            )
        })
        .collect();

    let inner_summary = crate::core::transaction::InstructionSummary {
        index: 0, // Inner instruction index doesn't matter for parsing
        program: crate::core::transaction::AccountReferenceSummary {
            index: inner_ix.instruction.program_id_index as usize,
            pubkey: Some(program_id.to_string()),
            signer: false,
            writable: false,
            source: crate::core::transaction::AccountSourceSummary::Static,
        },
        accounts: inner_accounts,
        data: inner_ix.instruction.data.clone().into_boxed_slice(),
    };
    parser_registry.parse_instruction(&inner_summary, program_id)
}

impl InnerInstructionSection {
    fn from_inner_instruction(
        inner_ix: &solana_message::inner_instruction::InnerInstruction,
        resolver: &LookupResolver,
        label: &str,
        parsed: &ParsedTransaction,
        parser_registry: &mut ParserRegistry,
    ) -> Self {
        // Resolve inner instruction accounts using the same logic as outer instructions
        let message = &parsed.transaction.message;
        let lookup_locations =
            crate::core::transaction::build_lookup_locations(&parsed.account_plan.address_lookups);

        let program = {
            let ref_summary = crate::core::transaction::classify_account_reference(
                message,
                inner_ix.instruction.program_id_index as usize,
                &parsed.account_plan,
                &lookup_locations,
            );
            InstructionAccountEntry::from_reference_with_resolver(&ref_summary, Some(resolver))
        };

        let accounts: Vec<InstructionAccountEntry> = inner_ix
            .instruction
            .accounts
            .iter()
            .map(|account_index| {
                let ref_summary = crate::core::transaction::classify_account_reference(
                    message,
                    *account_index as usize,
                    &parsed.account_plan,
                    &lookup_locations,
                );
                InstructionAccountEntry::from_reference_with_resolver(&ref_summary, Some(resolver))
            })
            .collect();

        // Try to parse the inner instruction
        let parsed_instruction = if let Ok(program_id) = Pubkey::from_str(&program.pubkey) {
            // First check if this is a CPI event
            let temp_summary = crate::core::transaction::InstructionSummary {
                index: 0,
                program: crate::core::transaction::AccountReferenceSummary {
                    index: inner_ix.instruction.program_id_index as usize,
                    pubkey: Some(program_id.to_string()),
                    signer: false,
                    writable: false,
                    source: crate::core::transaction::AccountSourceSummary::Static,
                },
                accounts: Vec::new(), // Not needed for CPI event detection
                data: inner_ix.instruction.data.clone().into_boxed_slice(),
            };

            // Check for CPI event first
            if is_anchor_cpi_event(&temp_summary) {
                // Try to parse as CPI event
                let cpi_result = parser_registry.parse_cpi_event(
                    &temp_summary,
                    &program_id,
                    message,
                    &parsed.account_plan,
                    &lookup_locations,
                );
                log::debug!(
                    "CPI event parse result for {}: {:?}",
                    program_id,
                    cpi_result.is_some()
                );
                cpi_result
            } else {
                parse_inner_instruction_as_regular(
                    inner_ix,
                    message,
                    &parsed.account_plan,
                    &lookup_locations,
                    parser_registry,
                    &program_id,
                )
            }
        } else {
            None
        };

        Self {
            label: label.to_string(),
            program,
            accounts,
            data: inner_ix.instruction.data.clone().into_boxed_slice(),
            parsed: parsed_instruction,
        }
    }
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub(super) enum InstructionAccountSource {
    Static,
    Lookup,
}

#[derive(Serialize)]
pub(super) struct InstructionAccountEntry {
    pub(super) index: usize,
    pub(super) pubkey: String,
    pub(super) signer: bool,
    pub(super) writable: bool,
    pub(super) source: InstructionAccountSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) lookup_table: Option<LookupReference>,
}

impl InstructionAccountEntry {
    fn from_reference_with_resolver(
        reference: &AccountReferenceSummary,
        resolver: Option<&LookupResolver>,
    ) -> Self {
        let (pubkey, source, lookup_table) = match &reference.source {
            AccountSourceSummary::Static => (
                reference.pubkey.clone().unwrap_or_else(|| "<missing>".into()),
                InstructionAccountSource::Static,
                None,
            ),
            AccountSourceSummary::Lookup { table_account, lookup_index, writable } => {
                let resolved =
                    resolver.and_then(|res| res.resolve(table_account, *writable, *lookup_index));
                let pubkey = resolved
                    .or_else(|| reference.pubkey.clone())
                    .unwrap_or_else(|| "<lookup-not-resolved>".into());
                let lookup_ref = LookupReference {
                    account_key: table_account.clone(),
                    index: *lookup_index,
                    writable: *writable,
                };
                (pubkey, InstructionAccountSource::Lookup, Some(lookup_ref))
            }
            AccountSourceSummary::Unknown => {
                unreachable!("Account source must be static or lookup table")
            }
        };

        Self {
            index: reference.index,
            pubkey,
            signer: reference.signer,
            writable: reference.writable,
            source,
            lookup_table,
        }
    }
}

#[derive(Serialize)]
pub(super) struct LookupReference {
    account_key: String,
    index: u8,
    writable: bool,
}

#[derive(Serialize)]
pub(super) struct SimulationSection {
    pub(super) status: SimulationStatusReport,
    pub(super) compute_units_consumed: u64,
    pub(super) logs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    return_data: Option<ReturnDataReport>,
    pub(super) post_account_count: usize,
}

impl SimulationSection {
    fn from_result(result: &SimulationResult) -> Self {
        let (status, post_account_count) = match &result.status {
            ExecutionStatus::Succeeded => {
                (SimulationStatusReport::Succeeded, result.post_accounts.len())
            }
            ExecutionStatus::Failed(error) => {
                (SimulationStatusReport::Failed { error: error.clone() }, 0)
            }
        };

        Self {
            status,
            compute_units_consumed: result.meta.compute_units_consumed,
            logs: result.meta.logs.clone(),
            return_data: ReturnDataReport::from_metadata(&result.meta),
            post_account_count,
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub(super) enum SimulationStatusReport {
    Succeeded,
    Failed { error: String },
}

#[derive(Serialize)]
struct ReturnDataReport {
    program_id: String,
    size: usize,
    data_base64: String,
}

impl ReturnDataReport {
    fn from_metadata(meta: &SimulationMetadata) -> Option<Self> {
        if meta.return_data.data.is_empty() {
            None
        } else {
            Some(ReturnDataReport {
                program_id: meta.return_data.program_id.to_string(),
                size: meta.return_data.data.len(),
                data_base64: BASE64_STANDARD.encode(&meta.return_data.data),
            })
        }
    }
}

#[derive(Serialize)]
struct FundingSection {
    pubkey: String,
    amount_lamports: u64,
}

#[derive(Serialize)]
struct TokenFundingSection {
    account: String,
    mint: String,
    decimals: u8,
    ui_amount: f64,
    amount_raw: u64,
}

#[derive(Serialize)]
struct ReplacementSection {
    #[serde(rename = "type")]
    replacement_type: String,
    pubkey: String,
    path: String,
}

fn serialize_bytes_as_hex<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("0x{}", hex::encode(bytes)))
}

fn replacement_to_section(entry: &Replacement) -> ReplacementSection {
    match entry {
        Replacement::Program { program_id, so_path } => ReplacementSection {
            replacement_type: "program".to_string(),
            pubkey: program_id.to_string(),
            path: so_path.display().to_string(),
        },
        Replacement::Account { pubkey, source_path, .. } => ReplacementSection {
            replacement_type: "account".to_string(),
            pubkey: pubkey.to_string(),
            path: source_path.display().to_string(),
        },
    }
}

pub(super) struct LookupResolver {
    entries: HashMap<(String, bool, u8), String>,
}

impl LookupResolver {
    pub(super) fn new(lookups: &[ResolvedLookup]) -> Self {
        let mut entries = HashMap::new();
        for lookup in lookups {
            let account_key = lookup.account_key.to_string();
            for (idx, key) in lookup.writable_indexes.iter().zip(&lookup.writable_addresses) {
                entries.insert((account_key.clone(), true, *idx), key.to_string());
            }
            for (idx, key) in lookup.readonly_indexes.iter().zip(&lookup.readonly_addresses) {
                entries.insert((account_key.clone(), false, *idx), key.to_string());
            }
        }
        Self { entries }
    }

    fn resolve(&self, table: &str, writable: bool, index: u8) -> Option<String> {
        self.entries.get(&(table.to_string(), writable, index)).cloned()
    }
}
