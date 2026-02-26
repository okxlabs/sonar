use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::cli::{self, SimulateArgs, TransactionInputArgs};
use crate::parsers::instruction::ParserRegistry;
use crate::utils::progress::Progress;
use crate::{core::executor as core_executor, core::transaction, output};
use sonar_sim::{
    ExecutionOptions, SimulationOptions, StateMutationOptions, TransactionExecutor,
    prepare_token_fundings,
};

use super::{parse_inputs_to_txs, prepare_accounts_and_idls, warn_unmatched_addresses};

fn cache_read_dir(cache_dir: Option<PathBuf>, refresh_cache: bool) -> Option<PathBuf> {
    if refresh_cache {
        None
    } else {
        cache_dir
    }
}

pub(crate) fn handle(args: SimulateArgs) -> Result<()> {
    // Initialize instruction parser registry (uses configured/default IDL directory).
    let idl_dir = args.idl_dir.clone();
    let mut parser_registry = ParserRegistry::new(idl_dir);

    log::debug!("Created parser registry with lazy IDL loading support");
    let progress = Progress::new();
    let SimulateArgs {
        transaction,
        rpc,
        replacements: replacement_args,
        fundings: funding_args,
        token_fundings: token_funding_args,
        ix_data,
        verify_signatures,
        idl_dir: _,
        show_balance_change,
        raw_log,
        show_ix_detail,
        timestamp,
        slot,
        data_patches: data_patch_args,
        cache,
        cache_dir,
        refresh_cache,
        no_idl_fetch,
    } = args;
    let rpc_url = rpc.rpc_url;

    let TransactionInputArgs { tx, json } = transaction;

    let replacements = replacement_args
        .into_iter()
        .map(|raw| cli::parse_replacement(&raw).map_err(anyhow::Error::msg))
        .collect::<Result<Vec<_>>>()?;

    let fundings = funding_args
        .into_iter()
        .map(|raw| cli::parse_funding(&raw).map_err(anyhow::Error::msg))
        .collect::<Result<Vec<_>>>()?;

    let token_funding_requests = token_funding_args
        .into_iter()
        .map(|raw| cli::parse_token_funding(&raw).map_err(anyhow::Error::msg))
        .collect::<Result<Vec<_>>>()?;

    let data_patches = data_patch_args
        .into_iter()
        .map(|raw| cli::parse_data_patch(&raw).map_err(anyhow::Error::msg))
        .collect::<Result<Vec<_>>>()?;

    // Build rendering options once; shared across all code paths.
    let render_opts = output::RenderOptions {
        json,
        show_ix_data: ix_data,
        show_ix_detail,
        verify_signatures,
        balance_opts: output::BalanceChangeOptions { show_balance_change },
        log_opts: output::LogDisplayOptions { raw_log },
    };

    // Check if this is a bundle (multiple positional TX arguments)
    if tx.len() > 1 {
        let sim_opts = SimulationOptions {
            execution: ExecutionOptions {
                signature_verification: verify_signatures.into(),
                slot,
                timestamp,
            },
            mutations: StateMutationOptions {
                replacements,
                fundings,
                data_patches,
                ..Default::default()
            },
        };
        return handle_bundle(
            tx,
            &rpc_url,
            token_funding_requests,
            sim_opts,
            &render_opts,
            &mut parser_registry,
            cache,
            cache_dir,
            refresh_cache,
            no_idl_fetch,
            &progress,
        );
    }

    let parsed_inputs = parse_inputs_to_txs(tx, &rpc_url, &progress, false)?;
    let mut parsed_txs = parsed_inputs.parsed_txs;
    let raw_input = parsed_inputs
        .raw_inputs
        .into_iter()
        .next()
        .expect("single input parse should produce one raw input");
    let mut parsed_tx =
        parsed_txs.pop().expect("single input parse should produce one parsed transaction");

    let cache_key = crate::core::cache::derive_cache_key_single(&raw_input, &parsed_tx.transaction);
    let (tx_cache_dir, offline) =
        crate::core::cache::resolve_cache_state(cache, &cache_dir, refresh_cache, &cache_key);
    let cache_read_dir_for_load = cache_read_dir(tx_cache_dir.clone(), refresh_cache);

    let mut prepared = prepare_accounts_and_idls(
        &rpc_url,
        cache_read_dir_for_load,
        offline,
        std::slice::from_ref(&parsed_tx),
        &mut parser_registry,
        no_idl_fetch,
        &progress,
    )?;

    warn_unmatched_addresses(
        &replacements,
        &fundings,
        &token_funding_requests,
        &[&parsed_tx],
        &prepared.resolved_accounts,
    );

    let prepared_token_fundings = if token_funding_requests.is_empty() {
        Vec::new()
    } else {
        prepare_token_fundings(
            &mut prepared.account_loader,
            &mut prepared.resolved_accounts,
            &token_funding_requests,
        )?
    };

    if !offline {
        if let Some(ref dir) = tx_cache_dir {
            core_executor::dump_accounts_to_dir(
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
                    inputs: vec![raw_input.clone()],
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
            replacements,
            fundings,
            token_fundings: prepared_token_fundings,
            data_patches,
        },
    };
    let mut executor = TransactionExecutor::prepare(prepared.resolved_accounts, sim_opts)?;

    let simulation = executor.execute(&parsed_tx.transaction)?;

    // Update transaction summary with inner instructions from simulation
    parsed_tx.summary = transaction::TransactionSummary::from_transaction(
        &parsed_tx.transaction,
        &parsed_tx.account_plan,
        simulation.meta.inner_instructions.clone(),
    );

    progress.finish();
    output::render(
        &parsed_tx,
        executor.resolved_accounts(),
        &simulation,
        executor.replacements(),
        executor.fundings(),
        executor.token_fundings(),
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
    token_funding_requests: Vec<cli::TokenFunding>,
    mut sim_opts: SimulationOptions,
    render_opts: &output::RenderOptions,
    parser_registry: &mut ParserRegistry,
    cache: bool,
    cache_dir: Option<PathBuf>,
    refresh_cache: bool,
    no_idl_fetch: bool,
    progress: &Progress,
) -> Result<()> {
    log::info!("Bundle simulation mode: {} transactions", tx_inputs.len());

    let parsed_inputs = parse_inputs_to_txs(tx_inputs, rpc_url, progress, true)?;
    let tx_inputs = parsed_inputs.raw_inputs;
    let parsed_txs = parsed_inputs.parsed_txs;
    log::info!("Successfully parsed {} transactions", parsed_txs.len());

    let cache_key = crate::core::cache::derive_cache_key_bundle(&tx_inputs, &parsed_txs);
    let (bundle_cache_dir, offline) =
        crate::core::cache::resolve_cache_state(cache, &cache_dir, refresh_cache, &cache_key);
    let cache_read_dir_for_load = cache_read_dir(bundle_cache_dir.clone(), refresh_cache);

    let tx_refs: Vec<_> = parsed_txs.iter().map(|p| &p.transaction).collect();
    let mut prepared = prepare_accounts_and_idls(
        rpc_url,
        cache_read_dir_for_load,
        offline,
        &parsed_txs,
        parser_registry,
        no_idl_fetch,
        progress,
    )?;

    let parsed_tx_refs: Vec<_> = parsed_txs.iter().collect();
    warn_unmatched_addresses(
        &sim_opts.mutations.replacements,
        &sim_opts.mutations.fundings,
        &token_funding_requests,
        &parsed_tx_refs,
        &prepared.resolved_accounts,
    );

    // Prepare token fundings
    let prepared_token_fundings = if token_funding_requests.is_empty() {
        Vec::new()
    } else {
        prepare_token_fundings(
            &mut prepared.account_loader,
            &mut prepared.resolved_accounts,
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
            core_executor::dump_accounts_to_dir(
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
                    inputs: tx_inputs.clone(),
                    rpc_url: rpc_url.to_string(),
                    account_count: prepared.resolved_accounts.accounts.len(),
                },
            )
            .context("Failed to write cache metadata")?;
        }
    }

    // Execute bundle simulation
    let total_tx_count = parsed_txs.len();
    let mut executor = TransactionExecutor::prepare(prepared.resolved_accounts, sim_opts)?;

    let simulations = executor.execute_bundle(&tx_refs);

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
    output::render_bundle(
        &updated_txs,
        total_tx_count,
        executor.resolved_accounts(),
        &simulations,
        executor.replacements(),
        executor.fundings(),
        executor.token_fundings(),
        parser_registry,
        render_opts,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::cache_read_dir;
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
