use std::borrow::Cow;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use solana_pubkey::Pubkey;

use crate::cli::{self, SimulateArgs, TransactionInputArgs};
use crate::parsers::instruction::ParserRegistry;
use crate::utils::progress::Progress;
use crate::{core::account_file, core::transaction, output};
use sonar_sim::internals::{
    ExecutionOptions, PreparedSimulation, SimulationOptions, StateMutationOptions,
    apply_ix_account_ops, apply_ix_data_patches, prepare_token_fundings,
};

use super::common::{
    CachePrepareArgs, build_cache_location, resolve_and_derive_cache_key,
    resolve_cache_and_prepare, resolve_from_instructions, warn_unmatched_addresses,
};

/// Parse every `--ix` value into `InstructionInput`s, in CLI order. Each value
/// is auto-detected: `@<path>` reads a file (`~` expanded, `@/dev/stdin` to
/// pipe), then the inline-or-file content is treated as JSON when it starts
/// with `{`/`[` and as named-field DSL otherwise.
fn load_instruction_args(args: &[String]) -> Result<Vec<transaction::InstructionInput>> {
    let mut inputs = Vec::new();
    for raw in args {
        inputs.extend(load_instruction_arg(raw)?);
    }
    Ok(inputs)
}

fn load_instruction_arg(raw: &str) -> Result<Vec<transaction::InstructionInput>> {
    // Resolve `@<path>` to file content; otherwise borrow the value inline.
    let (content, file_path): (Cow<str>, Option<&str>) =
        if let Some(path_str) = raw.strip_prefix('@') {
            if path_str.is_empty() {
                bail!("--ix `@` requires a file path (e.g. `@instructions.json`)");
            }
            let expanded = crate::utils::config::expand_tilde(path_str);
            let content = std::fs::read_to_string(&expanded)
                .with_context(|| format!("Failed to read instruction file `{path_str}`"))?;
            (Cow::Owned(content), Some(path_str))
        } else {
            (Cow::Borrowed(raw), None)
        };

    // Names the source for error context; only built on the failure path.
    let context = |format_name: &str| match file_path {
        Some(path) => format!("Failed to parse instruction file `{path}` as {format_name}"),
        None => format!("Failed to parse --ix value as {format_name}"),
    };

    if transaction::looks_like_json(&content) {
        transaction::parse_instruction_inputs_json(&content).with_context(|| context("JSON"))
    } else {
        transaction::parse_instruction_input_dsl(&content)
            .map(|input| vec![input])
            .with_context(|| context("instruction DSL"))
    }
}

/// What kind of simulation the user asked for, validated up front. Each
/// variant carries exactly the inputs that mode needs — unrelated combinations
/// (e.g. `--payer` without `--ix*`) are unrepresentable.
enum SimulateMode {
    /// Multiple positional TXs — atomic bundle simulation.
    Bundle(Vec<String>),
    /// At most one positional TX; an empty vec defers to stdin downstream.
    Single(Vec<String>),
    /// Synthesize a transaction from instruction inputs and a fee payer.
    /// `instructions` holds the raw `--ix` values, parsed lazily by
    /// `load_instruction_args`.
    Instructions { payer: Pubkey, instructions: Vec<String> },
}

impl SimulateMode {
    fn from_args(
        tx: Vec<String>,
        payer: Option<String>,
        instructions: Vec<String>,
        verify_signatures: bool,
    ) -> Result<Self> {
        if instructions.is_empty() {
            if payer.is_some() {
                bail!("--payer can only be used with instruction input");
            }
            return if tx.len() > 1 { Ok(Self::Bundle(tx)) } else { Ok(Self::Single(tx)) };
        }

        // Belt-and-suspenders: clap's `conflicts_with = "tx"` on `--ix` already
        // rejects this combination before we get here, but `from_args` is a
        // standalone unit and validates its own inputs.
        if !tx.is_empty() {
            bail!("--ix cannot be combined with TX positional arguments");
        }
        if verify_signatures {
            bail!(
                "--check-sig cannot be used with --ix because synthesized transactions are unsigned"
            );
        }
        let payer = match payer {
            Some(raw) => raw
                .parse::<Pubkey>()
                .with_context(|| format!("Failed to parse --payer pubkey `{raw}`"))?,
            None => transaction::default_payer(),
        };
        Ok(Self::Instructions { payer, instructions })
    }
}

fn parse_cli_args<T>(args: Vec<String>, parser: fn(&str) -> Result<T, String>) -> Result<Vec<T>> {
    args.iter().map(|raw| parser(raw).map_err(anyhow::Error::msg)).collect()
}

/// Apply instruction-level mutations (account patches, data patches) and rebuild
/// the transaction summary so the renderer sees the updated state.
fn apply_ix_mutations(
    parsed_tx: &mut transaction::ParsedTransaction,
    ix_account_ops: &[sonar_sim::internals::InstructionAccountOp],
    ix_data_patches: &[sonar_sim::internals::InstructionDataPatch],
) -> Result<()> {
    if !ix_account_ops.is_empty() {
        parsed_tx.account_plan = apply_ix_account_ops(&mut parsed_tx.transaction, ix_account_ops)
            .context("Failed to apply instruction account ops")?;
    }
    if !ix_data_patches.is_empty() {
        apply_ix_data_patches(&mut parsed_tx.transaction, ix_data_patches)
            .context("Failed to apply instruction data patches")?;
    }
    if !ix_account_ops.is_empty() || !ix_data_patches.is_empty() {
        parsed_tx.summary = transaction::TransactionSummary::from_transaction(
            &parsed_tx.transaction,
            &parsed_tx.account_plan,
            Vec::new(),
        );
    }
    Ok(())
}

pub(crate) fn handle(args: SimulateArgs, json: bool) -> Result<()> {
    let progress = Progress::new();
    let SimulateArgs {
        transaction,
        rpc,
        payer: payer_arg,
        instructions: instruction_args,
        overrides: override_args,
        fundings: funding_args,
        token_fundings: token_funding_args,
        ix_account_patches: ix_account_patch_args,
        ix_data_patches: ix_data_patch_args,
        ix_data,
        verify_signatures,
        idl_dir,
        show_balance_change,
        raw_log,
        show_ix_detail,
        history_slot,
        timestamp,
        slot,
        data_patches: data_patch_args,
        account_closures: account_closure_args,
        cache,
        cache_dir,
        refresh_cache,
        no_idl_fetch,
        ix_account_inserts: ix_account_insert_args,
        ix_account_removes: ix_account_remove_args,
    } = args;
    // --cache-dir or --refresh-cache imply --cache
    let cache = cache || cache_dir.is_some() || refresh_cache;
    let rpc_batch_size = rpc.rpc_batch_size;
    let rpc_url = rpc.rpc_url;
    let resolver_cache_location = Some(build_cache_location(&cache_dir));
    let mut parser_registry = ParserRegistry::new(idl_dir);
    log::debug!("Created parser registry with lazy IDL loading support");

    let TransactionInputArgs { tx } = transaction;
    let mode = SimulateMode::from_args(tx, payer_arg, instruction_args, verify_signatures)?;

    let account_overrides = parse_cli_args(override_args, cli::parse_override)?;
    let mut sol_fundings = parse_cli_args(funding_args, cli::parse_funding)?;
    // Auto-fund the placeholder payer when the user omitted --payer in ix mode.
    // Skips the injection if the user already funded the address explicitly.
    if let SimulateMode::Instructions { payer, .. } = &mode {
        if *payer == transaction::default_payer()
            && !sol_fundings.iter().any(|f| f.pubkey == *payer)
        {
            sol_fundings.push(cli::SolFunding {
                pubkey: *payer,
                amount_lamports: transaction::DEFAULT_PAYER_LAMPORTS,
            });
        }
    }
    let token_funding_requests = parse_cli_args(token_funding_args, cli::parse_token_funding)?;
    let account_data_patches = parse_cli_args(data_patch_args, cli::parse_data_patch)?;
    let account_closures = parse_cli_args(account_closure_args, cli::parse_close_account)?;
    // CLI flags surface three families separately for ergonomics, but all feed
    // into one ordered op list. Concatenate in flag-listed order: patches,
    // inserts, removes. Within each family, CLI argument order is preserved.
    let mut ix_account_ops: Vec<sonar_sim::internals::InstructionAccountOp> =
        parse_cli_args(ix_account_patch_args, cli::parse_ix_account_patch)?;
    ix_account_ops.extend(parse_cli_args(ix_account_insert_args, cli::parse_ix_account_insert)?);
    ix_account_ops.extend(parse_cli_args(ix_account_remove_args, cli::parse_ix_account_remove)?);
    let ix_data_patches = parse_cli_args(ix_data_patch_args, cli::parse_ix_data_patch)?;

    // Build rendering options once; shared across all code paths.
    let render_opts = output::RenderOptions {
        json,
        show_ix_data: ix_data,
        show_ix_detail,
        verify_signatures,
        balance_opts: output::BalanceChangeOptions { show_balance_change },
        log_opts: output::LogDisplayOptions { raw_log },
    };

    let resolved = match mode {
        SimulateMode::Bundle(txs) => {
            let sim_opts = SimulationOptions {
                execution: ExecutionOptions {
                    signature_verification: verify_signatures.into(),
                    slot,
                    timestamp,
                },
                mutations: StateMutationOptions {
                    account_closures,
                    account_overrides,
                    sol_fundings,
                    account_data_patches,
                    ..Default::default()
                },
            };
            return handle_bundle(
                txs,
                &rpc_url,
                resolver_cache_location,
                token_funding_requests,
                ix_account_ops,
                ix_data_patches,
                sim_opts,
                &render_opts,
                &mut parser_registry,
                cache,
                cache_dir,
                refresh_cache,
                no_idl_fetch,
                rpc_batch_size,
                history_slot,
                &progress,
            );
        }
        SimulateMode::Single(tx) => {
            resolve_and_derive_cache_key(tx, &rpc_url, resolver_cache_location, &progress)?
        }
        SimulateMode::Instructions { payer, instructions } => {
            let inputs = load_instruction_args(&instructions)?;
            resolve_from_instructions(payer, inputs)?
        }
    };
    let resolved_input = resolved
        .resolved_txs
        .into_iter()
        .next()
        .expect("single input resolve should produce one transaction");
    let raw_input = resolved_input.original_input.clone();
    let cached_raw_tx = resolved_input.raw_tx_base64.clone();
    let resolved_from = resolved_input.source.as_str().to_string();
    let mut parsed_tx = resolved_input.parsed_tx;

    apply_ix_mutations(&mut parsed_tx, &ix_account_ops, &ix_data_patches)?;

    let cache_args = CachePrepareArgs {
        rpc_url: &rpc_url,
        cache_enabled: cache,
        cache_dir: &cache_dir,
        refresh_cache,
        no_idl_fetch,
        rpc_batch_size,
    };
    let cached = resolve_cache_and_prepare(
        &cache_args,
        &resolved.cache_key,
        std::slice::from_ref(&parsed_tx),
        &mut parser_registry,
        &progress,
        history_slot,
    )?;
    let tx_cache_dir = cached.cache_dir;
    let offline = cached.offline;
    let mut prepared = cached.prepared;

    warn_unmatched_addresses(
        &account_overrides,
        &sol_fundings,
        &token_funding_requests,
        &account_closures,
        &[&parsed_tx],
        &prepared.resolved_accounts,
    );

    let prepared_token_fundings = if token_funding_requests.is_empty() {
        Vec::new()
    } else {
        prepare_token_fundings(
            &mut prepared.account_loader,
            &prepared.resolved_accounts,
            &token_funding_requests,
        )?
    };

    if !offline {
        if let Some(ref dir) = tx_cache_dir {
            account_file::dump_accounts_to_dir(
                &prepared.resolved_accounts,
                &parsed_tx.account_plan.static_accounts,
                dir,
            )
            .context("Failed to write account cache")?;
            crate::core::cache::write_meta_json(
                dir,
                &crate::core::cache::CacheMeta {
                    created_at: chrono::Utc::now().to_rfc3339(),
                    sonar_version: env!("CARGO_PKG_VERSION").to_string(),
                    cache_type: "single".to_string(),
                    transactions: vec![crate::core::cache::CacheTransaction {
                        input: raw_input.clone(),
                        raw_tx: cached_raw_tx,
                        resolved_from,
                    }],
                    rpc_url: rpc_url.clone(),
                    account_count: prepared.resolved_accounts.accounts.len(),
                },
            )
            .context("Failed to write cache metadata")?;
        }
    }

    let sim_opts = SimulationOptions {
        execution: ExecutionOptions {
            signature_verification: verify_signatures.into(),
            slot,
            timestamp,
        },
        mutations: StateMutationOptions {
            account_closures,
            account_overrides,
            sol_fundings,
            token_fundings: prepared_token_fundings,
            account_data_patches,
        },
    };
    let mut runner =
        PreparedSimulation::prepare(prepared.resolved_accounts, sim_opts)?.into_runner();

    let simulation = runner.execute(&parsed_tx.transaction)?;

    // Update transaction summary with inner instructions from simulation
    parsed_tx.summary = transaction::TransactionSummary::from_transaction(
        &parsed_tx.transaction,
        &parsed_tx.account_plan,
        simulation.meta.inner_instructions.clone(),
    );

    progress.finish();
    let mutations = runner.mutations();
    let ctx = output::SimulationContext {
        account_closures: &mutations.account_closures,
        account_overrides: &mutations.account_overrides,
        fundings: &mutations.sol_fundings,
        token_fundings: &mutations.token_fundings,
    };
    output::render(
        &parsed_tx,
        runner.resolved_accounts(),
        &simulation,
        &ctx,
        &mut parser_registry,
        &render_opts,
    )?;

    Ok(())
}

/// Handle bundle simulation (multiple transactions executed sequentially).
#[allow(clippy::too_many_arguments)]
fn handle_bundle(
    tx_inputs: Vec<String>,
    rpc_url: &str,
    resolver_cache_location: Option<crate::core::cache::CacheLocation>,
    token_funding_requests: Vec<cli::TokenFunding>,
    ix_account_ops: Vec<sonar_sim::internals::InstructionAccountOp>,
    ix_data_patches: Vec<sonar_sim::internals::InstructionDataPatch>,
    mut sim_opts: SimulationOptions,
    render_opts: &output::RenderOptions,
    parser_registry: &mut ParserRegistry,
    cache: bool,
    cache_dir: Option<PathBuf>,
    refresh_cache: bool,
    no_idl_fetch: bool,
    rpc_batch_size: usize,
    history_slot: Option<u64>,
    progress: &Progress,
) -> Result<()> {
    log::info!("Bundle simulation mode: {} transactions", tx_inputs.len());

    let resolved =
        resolve_and_derive_cache_key(tx_inputs, rpc_url, resolver_cache_location, progress)?;
    let resolved_txs = resolved.resolved_txs;
    let mut parsed_txs: Vec<_> = resolved_txs.iter().map(|tx| tx.parsed_tx.clone()).collect();
    log::info!("Successfully parsed {} transactions", parsed_txs.len());

    for parsed_tx in &mut parsed_txs {
        apply_ix_mutations(parsed_tx, &ix_account_ops, &ix_data_patches)?;
    }

    let cache_args = CachePrepareArgs {
        rpc_url,
        cache_enabled: cache,
        cache_dir: &cache_dir,
        refresh_cache,
        no_idl_fetch,
        rpc_batch_size,
    };
    let cached = resolve_cache_and_prepare(
        &cache_args,
        &resolved.cache_key,
        &parsed_txs,
        parser_registry,
        progress,
        history_slot,
    )?;
    let bundle_cache_dir = cached.cache_dir;
    let offline = cached.offline;
    let mut prepared = cached.prepared;

    let parsed_tx_refs: Vec<_> = parsed_txs.iter().collect();
    warn_unmatched_addresses(
        &sim_opts.mutations.account_overrides,
        &sim_opts.mutations.sol_fundings,
        &token_funding_requests,
        &sim_opts.mutations.account_closures,
        &parsed_tx_refs,
        &prepared.resolved_accounts,
    );

    // Prepare token fundings
    let prepared_token_fundings = if token_funding_requests.is_empty() {
        Vec::new()
    } else {
        prepare_token_fundings(
            &mut prepared.account_loader,
            &prepared.resolved_accounts,
            &token_funding_requests,
        )?
    };
    sim_opts.mutations.token_fundings = prepared_token_fundings;

    if !offline {
        if let Some(ref dir) = bundle_cache_dir {
            let required_accounts: std::collections::HashSet<_> = parsed_txs
                .iter()
                .flat_map(|tx| tx.account_plan.static_accounts.iter().copied())
                .collect();
            let required_accounts: Vec<_> = required_accounts.into_iter().collect();
            account_file::dump_accounts_to_dir(
                &prepared.resolved_accounts,
                &required_accounts,
                dir,
            )
            .context("Failed to write account cache")?;
            crate::core::cache::write_meta_json(
                dir,
                &crate::core::cache::CacheMeta {
                    created_at: chrono::Utc::now().to_rfc3339(),
                    sonar_version: env!("CARGO_PKG_VERSION").to_string(),
                    cache_type: "bundle".to_string(),
                    transactions: resolved_txs
                        .iter()
                        .map(|tx| crate::core::cache::CacheTransaction {
                            input: tx.original_input.clone(),
                            raw_tx: tx.raw_tx_base64.clone(),
                            resolved_from: tx.source.as_str().to_string(),
                        })
                        .collect(),
                    rpc_url: rpc_url.to_string(),
                    account_count: prepared.resolved_accounts.accounts.len(),
                },
            )
            .context("Failed to write cache metadata")?;
        }
    }

    // Execute bundle simulation
    let total_tx_count = parsed_txs.len();
    let mut runner =
        PreparedSimulation::prepare(prepared.resolved_accounts, sim_opts)?.into_runner();

    let tx_refs: Vec<_> = parsed_txs.iter().map(|p| &p.transaction).collect();
    let bundle_results = runner.execute_bundle(&tx_refs);
    if bundle_results.skipped_count() > 0 {
        log::warn!(
            "Bundle: {}/{} transactions skipped due to prior failure",
            bundle_results.skipped_count(),
            bundle_results.total(),
        );
    }
    let simulations: Vec<_> = bundle_results
        .into_executed()
        .into_iter()
        .enumerate()
        .map(|(index, result)| {
            result.map_err(|err| {
                anyhow::anyhow!("Bundle execution internal error at tx #{}: {}", index + 1, err)
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // Update transaction summaries with inner instructions from simulation
    // Note: simulations may be shorter than parsed_txs due to fail-fast behavior
    let executed_count = simulations.len();
    let updated_txs: Vec<_> = parsed_txs
        .into_iter()
        .take(executed_count)
        .zip(simulations.iter())
        .map(|(mut parsed_tx, simulation)| {
            parsed_tx.summary = transaction::TransactionSummary::from_transaction(
                &parsed_tx.transaction,
                &parsed_tx.account_plan,
                simulation.meta.inner_instructions.clone(),
            );
            parsed_tx
        })
        .collect();

    progress.finish();
    let mutations = runner.mutations();
    let ctx = output::SimulationContext {
        account_closures: &mutations.account_closures,
        account_overrides: &mutations.account_overrides,
        fundings: &mutations.sol_fundings,
        token_fundings: &mutations.token_fundings,
    };
    output::render_bundle(
        &updated_txs,
        total_tx_count,
        runner.resolved_accounts(),
        &simulations,
        &ctx,
        parser_registry,
        render_opts,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::handlers::common::cache_read_dir;
    use std::path::PathBuf;

    #[test]
    fn cache_read_dir_keeps_cache_when_not_refreshing() {
        let dir = Some(PathBuf::from("/tmp/sonar-cache"));
        let selected = cache_read_dir(dir.clone(), false);
        assert_eq!(selected, dir);
    }

    #[test]
    fn cache_read_dir_ignores_cache_when_refreshing() {
        let dir = Some(PathBuf::from("/tmp/sonar-cache"));
        let selected = cache_read_dir(dir, true);
        assert!(selected.is_none());
    }
}
