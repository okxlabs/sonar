use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use anyhow::{Result, anyhow};
use solana_commitment_config::CommitmentConfig;
use solana_message::compiled_instruction::CompiledInstruction;
use solana_message::inner_instruction::{InnerInstruction, InnerInstructionsList};
use solana_pubkey::Pubkey;
use solana_transaction_status_client_types::option_serializer::OptionSerializer;
use solana_transaction_status_client_types::{
    UiCompiledInstruction, UiInnerInstructions, UiInstruction, UiTransactionEncoding,
    UiTransactionStatusMeta, UiTransactionTokenBalance,
};
use sonar_sim::internals::{
    ExecutionResult, ExecutionStatus, ResolvedAccounts, ResolvedLookup, ReturnData,
    SimulationMetadata,
};

use solana_account::AccountSharedData;

use crate::cli::TraceArgs;
use crate::core::account_loader;
use crate::core::rpc_client::{GetTransactionConfig, RpcClient};
use crate::core::transaction::{
    ParsedTransaction, encode_transaction_to_base64, parse_raw_transaction,
};
use crate::output::report::{SolBalanceChangeSection, TokenBalanceChangeSection};
use crate::output::{self, BalanceChangeOptions, LogDisplayOptions, RenderOptions};
use crate::parsers::instruction::ParserRegistry;
use crate::utils::progress::Progress;

pub(crate) fn handle(args: TraceArgs, json: bool) -> Result<()> {
    let progress = Progress::new();
    let rpc_url = args.rpc.rpc_url;
    let rpc_batch_size = args.rpc.rpc_batch_size;
    let idl_dir = args.idl_dir.clone();
    let mut parser_registry = ParserRegistry::new(idl_dir);

    let signature = args.signature.trim().to_string();
    let parsed_sig =
        signature.parse().map_err(|_| anyhow!("Invalid transaction signature: {}", signature))?;

    progress.set_message("Fetching transaction from RPC...");
    let client = RpcClient::new(&rpc_url);
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
    let raw_base64 = encode_transaction_to_base64(&tx)?;

    let meta = response.meta;
    let inner_instructions = meta
        .as_ref()
        .and_then(|m| match &m.inner_instructions {
            OptionSerializer::Some(ui_inner) => Some(convert_ui_inner_instructions(ui_inner)),
            _ => None,
        })
        .unwrap_or_default();

    let mut parsed_tx = parse_raw_transaction(&raw_base64)?;
    parsed_tx.summary.inner_instructions = inner_instructions;

    // Resolve lookups and ordered keys from historical meta.loaded_addresses,
    // not live ALT state — ALTs may have been closed since the tx confirmed.
    let (meta_lookups, ordered_keys) = resolve_from_meta(&parsed_tx, &meta);

    // Fetch only program accounts for IDL parsing — trace doesn't need
    // sysvars, token accounts, or ALTs like simulate/decode do.
    let resolved_accounts = load_accounts_and_idls(
        &client,
        &rpc_url,
        &parsed_tx,
        &ordered_keys,
        &mut parser_registry,
        args.no_idl_fetch,
        meta_lookups,
        &progress,
        rpc_batch_size,
    );

    progress.finish();

    let execution_result = build_execution_result(&meta);

    let (sol_changes, token_changes) = if args.show_balance_change {
        compute_balance_changes(&meta, &ordered_keys)
    } else {
        (Vec::new(), Vec::new())
    };

    let render_opts = RenderOptions {
        json,
        show_ix_data: args.ix_data,
        show_ix_detail: args.show_ix_detail,
        verify_signatures: false,
        balance_opts: BalanceChangeOptions { show_balance_change: args.show_balance_change },
        log_opts: LogDisplayOptions { raw_log: args.raw_log },
    };

    output::render_trace(
        &parsed_tx,
        &resolved_accounts,
        &execution_result,
        &mut parser_registry,
        &render_opts,
        sol_changes,
        token_changes,
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Program-only account loading for IDL parsing
// ---------------------------------------------------------------------------

/// Fetch outer instruction program accounts for IDL parsing, then mark
/// CPI program accounts as executable using inner instruction data already
/// in memory (zero extra RPC calls for the executable flag).
fn load_accounts_and_idls(
    client: &RpcClient,
    rpc_url: &str,
    parsed_tx: &ParsedTransaction,
    ordered_keys: &[Pubkey],
    parser_registry: &mut ParserRegistry,
    no_idl_fetch: bool,
    meta_lookups: Vec<ResolvedLookup>,
    progress: &Progress,
    rpc_batch_size: usize,
) -> ResolvedAccounts {
    let mut program_ids: Vec<Pubkey> = parsed_tx
        .summary
        .instructions
        .iter()
        .filter_map(|ix| ix.program.pubkey.as_ref())
        .filter_map(|s| Pubkey::from_str(s).ok())
        .collect();
    program_ids.sort();
    program_ids.dedup();

    progress.set_message("Fetching program accounts...");
    let mut accounts: HashMap<Pubkey, AccountSharedData> = HashMap::new();
    for chunk in program_ids.chunks(rpc_batch_size) {
        match client.get_multiple_accounts(chunk) {
            Ok(results) => {
                for (pubkey, account) in chunk.iter().zip(results) {
                    if let Some(account) = account {
                        accounts.insert(*pubkey, AccountSharedData::from(account));
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to fetch program accounts: {:#}", e);
            }
        }
    }

    // Mark CPI program accounts as executable using inner instruction data
    // already in memory — no extra RPC calls needed.
    for group in &parsed_tx.summary.inner_instructions {
        for inner_ix in group {
            let idx = inner_ix.instruction.program_id_index as usize;
            if let Some(&pubkey) = ordered_keys.get(idx) {
                accounts.entry(pubkey).or_insert_with(|| {
                    AccountSharedData::from(solana_account::Account {
                        executable: true,
                        ..Default::default()
                    })
                });
            }
        }
    }

    let resolved = ResolvedAccounts { accounts, lookups: meta_lookups };

    // Delegate to the shared IDL pipeline (auto-fetch + load from disk)
    match account_loader::create_loader(rpc_url.to_string(), None, false, Some(progress.clone()), rpc_batch_size) {
        Ok(loader) => {
            super::common::run_idl_pipeline(
                &loader,
                parser_registry,
                &resolved,
                no_idl_fetch,
                false,
                Some(progress),
            );
        }
        Err(e) => {
            log::warn!("Failed to create account loader for IDL fetching: {:#}", e);
        }
    }

    resolved
}

// ---------------------------------------------------------------------------
// RPC metadata → native type conversions
// ---------------------------------------------------------------------------

/// Convert RPC UiInnerInstructions to native InnerInstructionsList.
///
/// InnerInstructionsList is Vec<Vec<InnerInstruction>>, indexed by outer instruction index.
/// UiInnerInstructions has an explicit `index` field for the outer instruction index.
fn convert_ui_inner_instructions(ui_inner: &[UiInnerInstructions]) -> InnerInstructionsList {
    let max_index = ui_inner.iter().map(|g| g.index as usize).max().unwrap_or(0);
    let mut result: InnerInstructionsList = vec![Vec::new(); max_index + 1];

    for group in ui_inner {
        let idx = group.index as usize;
        for ui_ix in &group.instructions {
            if let UiInstruction::Compiled(compiled) = ui_ix {
                result[idx].push(convert_ui_compiled_to_inner(compiled));
            }
            // UiInstruction::Parsed skipped: lacks raw account indices.
            // With Base64 encoding request, inner instructions arrive as Compiled.
        }
    }
    result
}

fn convert_ui_compiled_to_inner(ui: &UiCompiledInstruction) -> InnerInstruction {
    let data = bs58::decode(&ui.data).into_vec().unwrap_or_else(|e| {
        log::warn!("Failed to decode inner instruction data as base58: {}", e);
        Vec::new()
    });
    InnerInstruction {
        instruction: CompiledInstruction {
            program_id_index: ui.program_id_index,
            accounts: ui.accounts.clone(),
            data,
        },
        stack_height: ui.stack_height.unwrap_or(0).try_into().unwrap_or(0),
    }
}

// ---------------------------------------------------------------------------
// Balance change computation (directly from RPC arrays)
// ---------------------------------------------------------------------------

/// Resolve lookup addresses and build the ordered account key list from
/// `meta.loaded_addresses` (historical data) rather than live ALT state.
///
/// Returns (ResolvedLookup list, ordered account keys). The ordered keys
/// match Solana's v0 account ordering: static keys, then all writable
/// lookup addresses across all tables, then all readonly — matching the
/// RPC balance array indices.
fn resolve_from_meta(
    parsed_tx: &ParsedTransaction,
    meta: &Option<UiTransactionStatusMeta>,
) -> (Vec<ResolvedLookup>, Vec<Pubkey>) {
    let mut ordered_keys: Vec<Pubkey> = parsed_tx.account_plan.static_accounts.clone();

    let Some(meta) = meta else {
        return (Vec::new(), ordered_keys);
    };
    let OptionSerializer::Some(loaded) = &meta.loaded_addresses else {
        return (Vec::new(), ordered_keys);
    };

    let writable_addrs: Vec<Pubkey> =
        loaded.writable.iter().filter_map(|s| Pubkey::from_str(s).ok()).collect();
    let readonly_addrs: Vec<Pubkey> =
        loaded.readonly.iter().filter_map(|s| Pubkey::from_str(s).ok()).collect();

    // Ordered keys: static ++ all writable lookups ++ all readonly lookups
    ordered_keys.extend_from_slice(&writable_addrs);
    ordered_keys.extend_from_slice(&readonly_addrs);

    // Distribute flat address lists across per-table ResolvedLookup entries
    let mut w_offset = 0;
    let mut r_offset = 0;
    let lookups = parsed_tx
        .account_plan
        .address_lookups
        .iter()
        .map(|lookup| {
            let w_count = lookup.writable_indexes.len();
            let r_count = lookup.readonly_indexes.len();
            let w = writable_addrs.get(w_offset..w_offset + w_count).unwrap_or_default().to_vec();
            let r = readonly_addrs.get(r_offset..r_offset + r_count).unwrap_or_default().to_vec();
            w_offset += w_count;
            r_offset += r_count;

            ResolvedLookup {
                account_key: lookup.account_key,
                writable_indexes: lookup.writable_indexes.clone(),
                writable_addresses: w,
                readonly_indexes: lookup.readonly_indexes.clone(),
                readonly_addresses: r,
            }
        })
        .collect();

    (lookups, ordered_keys)
}

/// Compute SOL and token balance changes directly from RPC balance arrays.
fn compute_balance_changes(
    meta: &Option<UiTransactionStatusMeta>,
    ordered_keys: &[Pubkey],
) -> (Vec<SolBalanceChangeSection>, Vec<TokenBalanceChangeSection>) {
    let Some(meta) = meta else {
        return (Vec::new(), Vec::new());
    };

    let sol_changes: Vec<SolBalanceChangeSection> = meta
        .pre_balances
        .iter()
        .zip(&meta.post_balances)
        .enumerate()
        .filter(|(_, (pre, post))| pre != post)
        .filter_map(|(i, (pre, post))| {
            let pubkey = ordered_keys.get(i)?;
            let change = *post as i128 - *pre as i128;
            Some(SolBalanceChangeSection {
                account: pubkey.to_string(),
                before: *pre,
                after: *post,
                change,
                change_sol: change as f64 / 1_000_000_000.0,
            })
        })
        .collect();

    let pre_tokens = match &meta.pre_token_balances {
        OptionSerializer::Some(b) => b.as_slice(),
        _ => &[],
    };
    let post_tokens = match &meta.post_token_balances {
        OptionSerializer::Some(b) => b.as_slice(),
        _ => &[],
    };
    let token_changes = compute_token_balance_changes(pre_tokens, post_tokens, ordered_keys);

    (sol_changes, token_changes)
}

/// Match pre/post token balances by account_index and compute diffs.
fn compute_token_balance_changes(
    pre: &[UiTransactionTokenBalance],
    post: &[UiTransactionTokenBalance],
    ordered_keys: &[Pubkey],
) -> Vec<TokenBalanceChangeSection> {
    let mut changes = Vec::new();

    let pre_map: HashMap<u8, &UiTransactionTokenBalance> =
        pre.iter().map(|b| (b.account_index, b)).collect();

    for post_tb in post {
        let account_index = post_tb.account_index;
        let Some(pubkey) = ordered_keys.get(account_index as usize) else { continue };

        let pre_amount: u64 = pre_map
            .get(&account_index)
            .and_then(|tb| tb.ui_token_amount.amount.parse().ok())
            .unwrap_or(0);
        let post_amount: u64 = post_tb.ui_token_amount.amount.parse().unwrap_or(0);

        if pre_amount != post_amount {
            changes.push(make_token_change(post_tb, pubkey, pre_amount, post_amount));
        }
    }

    // Closed token accounts: exist in pre but not in post
    let post_indices: HashSet<u8> = post.iter().map(|b| b.account_index).collect();
    for pre_tb in pre {
        if post_indices.contains(&pre_tb.account_index) {
            continue;
        }
        let Some(pubkey) = ordered_keys.get(pre_tb.account_index as usize) else { continue };
        let pre_amount: u64 = pre_tb.ui_token_amount.amount.parse().unwrap_or(0);
        if pre_amount != 0 {
            changes.push(make_token_change(pre_tb, pubkey, pre_amount, 0));
        }
    }

    changes
}

fn make_token_change(
    tb: &UiTransactionTokenBalance,
    pubkey: &Pubkey,
    before: u64,
    after: u64,
) -> TokenBalanceChangeSection {
    let change = after as i128 - before as i128;
    let decimals = tb.ui_token_amount.decimals;
    let divisor = 10f64.powi(decimals as i32);
    TokenBalanceChangeSection {
        owner: match &tb.owner {
            OptionSerializer::Some(s) => s.clone(),
            _ => String::new(),
        },
        token_account: pubkey.to_string(),
        mint: tb.mint.clone(),
        before,
        after,
        change,
        decimals,
        ui_change: change as f64 / divisor,
    }
}

// ---------------------------------------------------------------------------
// Execution result construction
// ---------------------------------------------------------------------------

/// Build ExecutionResult from RPC metadata for SimulationSection rendering.
/// Inner instructions go through TransactionSection (via parsed_tx.summary), not here.
fn build_execution_result(meta: &Option<UiTransactionStatusMeta>) -> ExecutionResult {
    let Some(meta) = meta else {
        return ExecutionResult {
            status: ExecutionStatus::Failed("No execution metadata in RPC response".into()),
            meta: SimulationMetadata::default(),
            post_accounts: HashMap::new(),
            pre_accounts: HashMap::new(),
        };
    };

    let status = match &meta.err {
        Some(err) => ExecutionStatus::Failed(format!("{:?}", err)),
        None => ExecutionStatus::Succeeded,
    };

    let logs = match &meta.log_messages {
        OptionSerializer::Some(logs) => logs.clone(),
        _ => Vec::new(),
    };

    let compute_units_consumed = match &meta.compute_units_consumed {
        OptionSerializer::Some(cu) => *cu,
        _ => 0,
    };

    let return_data = match &meta.return_data {
        OptionSerializer::Some(rd) => {
            let program_id = rd.program_id.parse::<Pubkey>().unwrap_or_default();
            let (data_str, _encoding) = &rd.data;
            let data = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data_str)
                .unwrap_or_default();
            ReturnData { program_id, data }
        }
        _ => ReturnData::default(),
    };

    ExecutionResult {
        status,
        meta: SimulationMetadata {
            logs,
            inner_instructions: Vec::new(),
            compute_units_consumed,
            return_data,
        },
        post_accounts: HashMap::new(),
        pre_accounts: HashMap::new(),
    }
}
