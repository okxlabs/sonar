use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::cli::{self, SimulateArgs, TransactionInputArgs};
use crate::parsers::instruction::ParserRegistry;
use crate::utils::progress::Progress;
use crate::{core::account_file, core::transaction, output};
use sonar_sim::{
    ExecutionOptions, PreparedSimulation, SimulationOptions, StateMutationOptions,
    apply_ix_account_appends, apply_ix_account_patches, apply_ix_data_patches,
    prepare_token_fundings,
};

use super::{prepare_accounts_and_idls, resolve_inputs_to_txs, warn_unmatched_addresses};

/// Apply instruction-level mutations (account patches, data patches) and rebuild
/// the transaction summary so the renderer sees the updated state.
fn apply_ix_mutations(
    parsed_tx: &mut transaction::ParsedTransaction,
    ix_account_patches: &[sonar_sim::InstructionAccountPatch],
    ix_account_appends: &[sonar_sim::InstructionAccountAppend],
    ix_data_patches: &[sonar_sim::InstructionDataPatch],
) -> Result<()> {
    if !ix_account_patches.is_empty() {
        parsed_tx.account_plan =
            apply_ix_account_patches(&mut parsed_tx.transaction, ix_account_patches)
                .context("Failed to apply instruction account patches")?;
    }
    if !ix_account_appends.is_empty() {
        parsed_tx.account_plan =
            apply_ix_account_appends(&mut parsed_tx.transaction, ix_account_appends)
                .context("Failed to apply instruction account appends")?;
    }
    if !ix_data_patches.is_empty() {
        apply_ix_data_patches(&mut parsed_tx.transaction, ix_data_patches)
            .context("Failed to apply instruction data patches")?;
    }
    if !ix_account_patches.is_empty()
        || !ix_account_appends.is_empty()
        || !ix_data_patches.is_empty()
    {
        parsed_tx.summary = transaction::TransactionSummary::from_transaction(
            &parsed_tx.transaction,
            &parsed_tx.account_plan,
            Vec::new(),
        );
    }
    Ok(())
}

fn cache_read_dir(cache_dir: Option<PathBuf>, refresh_cache: bool) -> Option<PathBuf> {
    if refresh_cache { None } else { cache_dir }
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
        overrides: override_args,
        fundings: funding_args,
        token_fundings: token_funding_args,
        ix_account_patches: ix_account_patch_args,
        ix_data_patches: ix_data_patch_args,
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
        ix_account_appends: ix_account_append_args,
    } = args;
    let rpc_url = rpc.rpc_url;
    let resolver_cache_location = Some(if cache_dir.is_some() {
        crate::core::cache::CacheLocation::Explicit(crate::core::cache::resolve_cache_dir(
            &cache_dir,
        ))
    } else {
        crate::core::cache::CacheLocation::Auto(crate::core::cache::resolve_cache_dir(&cache_dir))
    });

    let TransactionInputArgs { tx, json } = transaction;

    let overrides = override_args
        .into_iter()
        .map(|raw| cli::parse_override(&raw).map_err(anyhow::Error::msg))
        .collect::<Result<Vec<_>>>()?;

    let sol_fundings = funding_args
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

    let ix_account_patches = ix_account_patch_args
        .into_iter()
        .map(|raw| cli::parse_ix_account_patch(&raw).map_err(anyhow::Error::msg))
        .collect::<Result<Vec<_>>>()?;

    let ix_account_appends = ix_account_append_args
        .into_iter()
        .map(|raw| cli::parse_ix_account_append(&raw).map_err(anyhow::Error::msg))
        .collect::<Result<Vec<_>>>()?;

    let ix_data_patches = ix_data_patch_args
        .into_iter()
        .map(|raw| cli::parse_ix_data_patch(&raw).map_err(anyhow::Error::msg))
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
                overrides,
                sol_fundings,
                data_patches,
                ..Default::default()
            },
        };
        return handle_bundle(
            tx,
            &rpc_url,
            resolver_cache_location,
            token_funding_requests,
            ix_account_patches,
            ix_account_appends,
            ix_data_patches,
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

    let parsed_inputs =
        resolve_inputs_to_txs(tx, &rpc_url, resolver_cache_location, &progress, false)?;
    let resolved_input = parsed_inputs
        .resolved_txs
        .into_iter()
        .next()
        .expect("single input resolve should produce one transaction");
    let raw_input = resolved_input.original_input.clone();
    let cached_raw_tx = resolved_input.raw_tx_base64.clone();
    let resolved_from = resolved_input.source.as_str().to_string();
    let mut parsed_tx = resolved_input.parsed_tx;

    apply_ix_mutations(&mut parsed_tx, &ix_account_patches, &ix_account_appends, &ix_data_patches)?;

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
        &overrides,
        &sol_fundings,
        &token_funding_requests,
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
            overrides,
            sol_fundings,
            token_fundings: prepared_token_fundings,
            data_patches,
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
    output::render(
        &parsed_tx,
        runner.resolved_accounts(),
        &simulation,
        runner.overrides(),
        runner.sol_fundings(),
        runner.token_fundings(),
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
    ix_account_patches: Vec<sonar_sim::InstructionAccountPatch>,
    ix_account_appends: Vec<sonar_sim::InstructionAccountAppend>,
    ix_data_patches: Vec<sonar_sim::InstructionDataPatch>,
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

    let parsed_inputs =
        resolve_inputs_to_txs(tx_inputs, rpc_url, resolver_cache_location, progress, true)?;
    let resolved_txs = parsed_inputs.resolved_txs;
    let tx_inputs: Vec<_> = resolved_txs.iter().map(|tx| tx.original_input.clone()).collect();
    let mut parsed_txs: Vec<_> = resolved_txs.iter().map(|tx| tx.parsed_tx.clone()).collect();
    log::info!("Successfully parsed {} transactions", parsed_txs.len());

    for parsed_tx in &mut parsed_txs {
        apply_ix_mutations(parsed_tx, &ix_account_patches, &ix_account_appends, &ix_data_patches)?;
    }

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
        &sim_opts.mutations.overrides,
        &sim_opts.mutations.sol_fundings,
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

    let bundle_results = runner.execute_bundle(&tx_refs);
    let simulations: Vec<_> = bundle_results
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
    output::render_bundle(
        &updated_txs,
        total_tx_count,
        runner.resolved_accounts(),
        &simulations,
        runner.overrides(),
        runner.sol_fundings(),
        runner.token_fundings(),
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
