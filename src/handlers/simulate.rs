use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::cli::{self, SimulateArgs, TransactionInputArgs};
use crate::parsers::instruction::ParserRegistry;
use crate::utils::progress::Progress;
use crate::{core::account_loader, core::executor, core::funding, core::transaction, output};

use super::{auto_fetch_missing_idls, collect_program_ids, warn_unmatched_addresses};

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
        // Build simulation options (token_fundings populated inside handle_bundle).
        let sim_opts = executor::SimulationOptions {
            replacements,
            fundings,
            data_patches,
            verify_signatures,
            slot,
            timestamp,
            ..Default::default()
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

    // Single tx: take the first positional arg, or fall back to stdin
    let tx_single = tx.into_iter().next();
    let raw_input = transaction::read_raw_transaction(tx_single)?;

    let parsed_tx = transaction::parse_transaction_input(&raw_input, &rpc_url, Some(&progress))?;

    let cache_key = crate::core::cache::derive_cache_key_single(&raw_input, &parsed_tx.transaction);
    let (tx_cache_dir, offline) =
        crate::core::cache::resolve_cache_state(cache, &cache_dir, refresh_cache, &cache_key);

    let account_loader = account_loader::create_loader(
        rpc_url.clone(),
        tx_cache_dir.clone(),
        offline,
        Some(progress.clone()),
    )?;
    let mut resolved_accounts = account_loader.load_for_transaction(&parsed_tx.transaction)?;

    warn_unmatched_addresses(
        &replacements,
        &fundings,
        &token_funding_requests,
        &[&parsed_tx],
        &resolved_accounts,
    );

    let prepared_token_fundings = if token_funding_requests.is_empty() {
        Vec::new()
    } else {
        funding::prepare_token_fundings(
            &account_loader,
            &mut resolved_accounts,
            &token_funding_requests,
        )?
    };

    let program_ids = collect_program_ids(&resolved_accounts);

    if program_ids.is_empty() {
        log::error!("No executable accounts found after RPC load; skipping IDL parsing");
    } else {
        if !no_idl_fetch && !offline {
            let idl_fetcher =
                account_loader::create_idl_fetcher(&account_loader, Some(progress.clone()));
            match auto_fetch_missing_idls(
                &idl_fetcher,
                &parser_registry,
                &program_ids,
                &resolved_accounts,
                Some(&progress),
            ) {
                Ok(count) if count > 0 => log::info!("Auto-fetched {} missing IDLs", count),
                Ok(_) => {}
                Err(err) => log::warn!("Failed to auto-fetch missing IDLs: {:#}", err),
            }
        }

        // Load IDL parsers for all programs used in this transaction
        match parser_registry.load_idl_parsers_for_programs(program_ids) {
            Ok(count) if count > 0 => log::info!("Lazy-loaded {} IDL parsers", count),
            Ok(_) => {}
            Err(err) => log::warn!("Failed to load IDL parsers: {:?}", err),
        }
    }

    if !offline {
        if let Some(ref dir) = tx_cache_dir {
            executor::dump_accounts_to_dir(
                &resolved_accounts,
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
                    account_count: resolved_accounts.accounts.len(),
                },
            )
            .context("Failed to write cache metadata")?;
        }
    }

    let sim_opts = executor::SimulationOptions {
        replacements,
        fundings,
        token_fundings: prepared_token_fundings,
        data_patches,
        verify_signatures,
        slot,
        timestamp,
    };
    let mut executor = executor::TransactionExecutor::prepare(resolved_accounts, sim_opts)?;

    let simulation = executor.simulate(&parsed_tx.transaction)?;

    // Update transaction summary with inner instructions from simulation
    let mut updated_tx = parsed_tx;
    updated_tx.summary = transaction::TransactionSummary::from_transaction(
        &updated_tx.transaction,
        &updated_tx.account_plan,
        simulation.meta.inner_instructions.clone(),
    );

    progress.finish();
    output::render(
        &updated_tx,
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
    mut sim_opts: executor::SimulationOptions,
    render_opts: &output::RenderOptions,
    parser_registry: &mut ParserRegistry,
    cache: bool,
    cache_dir: Option<PathBuf>,
    refresh_cache: bool,
    no_idl_fetch: bool,
    progress: &Progress,
) -> Result<()> {
    log::info!("Bundle simulation mode: {} transactions", tx_inputs.len());

    let parsed_txs =
        transaction::parse_multi_raw_transactions(&tx_inputs, rpc_url, Some(progress))?;
    log::info!("Successfully parsed {} transactions", parsed_txs.len());

    let cache_key = crate::core::cache::derive_cache_key_bundle(&tx_inputs, &parsed_txs);
    let (bundle_cache_dir, offline) =
        crate::core::cache::resolve_cache_state(cache, &cache_dir, refresh_cache, &cache_key);

    let tx_refs: Vec<_> = parsed_txs.iter().map(|p| &p.transaction).collect();

    let account_loader = account_loader::create_loader(
        rpc_url.to_string(),
        bundle_cache_dir.clone(),
        offline,
        Some(progress.clone()),
    )?;
    let mut resolved_accounts = account_loader.load_for_transactions(&tx_refs)?;

    let parsed_tx_refs: Vec<_> = parsed_txs.iter().collect();
    warn_unmatched_addresses(
        &sim_opts.replacements,
        &sim_opts.fundings,
        &token_funding_requests,
        &parsed_tx_refs,
        &resolved_accounts,
    );

    // Prepare token fundings
    let prepared_token_fundings = if token_funding_requests.is_empty() {
        Vec::new()
    } else {
        funding::prepare_token_fundings(
            &account_loader,
            &mut resolved_accounts,
            &token_funding_requests,
        )?
    };
    sim_opts.token_fundings = prepared_token_fundings;

    // Load IDL parsers for all programs
    let program_ids = collect_program_ids(&resolved_accounts);
    if !program_ids.is_empty() {
        if !no_idl_fetch && !offline {
            let idl_fetcher =
                account_loader::create_idl_fetcher(&account_loader, Some(progress.clone()));
            match auto_fetch_missing_idls(
                &idl_fetcher,
                parser_registry,
                &program_ids,
                &resolved_accounts,
                Some(progress),
            ) {
                Ok(count) if count > 0 => log::info!("Auto-fetched {} missing IDLs", count),
                Ok(_) => {}
                Err(err) => log::warn!("Failed to auto-fetch missing IDLs: {:#}", err),
            }
        }

        match parser_registry.load_idl_parsers_for_programs(program_ids) {
            Ok(count) if count > 0 => log::info!("Lazy-loaded {} IDL parsers", count),
            Ok(_) => {}
            Err(err) => log::warn!("Failed to load IDL parsers: {:?}", err),
        }
    }

    if !offline {
        if let Some(ref dir) = bundle_cache_dir {
            let required_accounts: std::collections::HashSet<_> = parsed_txs
                .iter()
                .flat_map(|tx| tx.account_plan.static_accounts.iter().copied())
                .collect();
            let required_accounts: Vec<_> = required_accounts.into_iter().collect();
            executor::dump_accounts_to_dir(&resolved_accounts, &required_accounts, dir)
                .context("Failed to write account cache")?;
            crate::core::cache::write_meta_json(
                dir,
                &crate::core::cache::CacheMeta {
                    created_at: chrono::Utc::now().to_rfc3339(),
                    sonar_version: env!("CARGO_PKG_VERSION").to_string(),
                    cache_type: "bundle".to_string(),
                    inputs: tx_inputs.clone(),
                    rpc_url: rpc_url.to_string(),
                    account_count: resolved_accounts.accounts.len(),
                },
            )
            .context("Failed to write cache metadata")?;
        }
    }

    // Execute bundle simulation
    let total_tx_count = parsed_txs.len();
    let mut executor = executor::TransactionExecutor::prepare(resolved_accounts, sim_opts)?;

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
