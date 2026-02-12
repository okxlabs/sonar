use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::cli::{self, SimulateArgs, TransactionInputArgs};
use crate::instruction_parsers::ParserRegistry;
use crate::{account_loader, executor, funding, output, transaction};

use super::{collect_program_ids, warn_unmatched_addresses};

pub(crate) fn handle(args: SimulateArgs) -> Result<()> {
    // Only attempt IDL-based parsing when the user explicitly supplies a directory
    let idl_dir = args.idl_dir.clone();
    let mut parser_registry = ParserRegistry::new(idl_dir);

    log::debug!("Created parser registry with lazy IDL loading support");
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
        dump_accounts,
        load_accounts,
        offline,
    } = args;
    let rpc_url = rpc.rpc_url;

    let TransactionInputArgs { tx, tx_file, output } = transaction;

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
        format: output,
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
            dump_accounts,
            load_accounts,
            offline,
        );
    }

    // Single tx: take the first positional arg, or fall back to --tx-file / stdin
    let tx_single = tx.into_iter().next();
    let raw_input = transaction::read_raw_transaction(tx_single, tx_file.as_deref())?;

    // Check if input looks like a transaction signature (works for all input methods)
    if transaction::is_transaction_signature(&raw_input) {
        log::info!("Input appears to be a transaction signature, attempting to fetch from RPC...");
        let fetched_tx = transaction::fetch_transaction_from_rpc(&rpc_url, &raw_input)?;
        let parsed_tx = transaction::parse_raw_transaction(&fetched_tx)?;

        let account_loader =
            account_loader::AccountLoader::new(rpc_url.clone(), load_accounts.clone(), offline)?;
        let mut resolved_accounts =
            account_loader.load_for_transaction(&parsed_tx.transaction, &replacements)?;
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
            match parser_registry.load_idl_parsers_for_programs(program_ids) {
                Ok(count) if count > 0 => log::info!("Lazy-loaded {} IDL parsers", count),
                Ok(_) => {}
                Err(err) => log::warn!("Failed to load IDL parsers: {:?}", err),
            }
        }

        // Dump original RPC account data before --replace / --patch-data
        if let Some(ref dump_dir) = dump_accounts {
            executor::dump_accounts_to_dir(&resolved_accounts.accounts, dump_dir)
                .context("Failed to dump accounts")?;
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

        return Ok(());
    }

    // If not a signature, parse as raw transaction
    let parsed_tx = transaction::parse_raw_transaction(&raw_input)?;

    let account_loader = account_loader::AccountLoader::new(rpc_url, load_accounts, offline)?;
    let mut resolved_accounts =
        account_loader.load_for_transaction(&parsed_tx.transaction, &replacements)?;

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
        // Load IDL parsers for all programs used in this transaction
        match parser_registry.load_idl_parsers_for_programs(program_ids) {
            Ok(count) if count > 0 => log::info!("Lazy-loaded {} IDL parsers", count),
            Ok(_) => {}
            Err(err) => log::warn!("Failed to load IDL parsers: {:?}", err),
        }
    }

    // Dump original RPC account data before --replace / --patch-data
    if let Some(ref dump_dir) = dump_accounts {
        executor::dump_accounts_to_dir(&resolved_accounts.accounts, dump_dir)
            .context("Failed to dump accounts")?;
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
    dump_accounts: Option<PathBuf>,
    load_accounts: Option<PathBuf>,
    offline: bool,
) -> Result<()> {
    log::info!("Bundle simulation mode: {} transactions", tx_inputs.len());

    // Parse all transactions
    let parsed_txs = transaction::parse_multi_raw_transactions(&tx_inputs, rpc_url)?;
    log::info!("Successfully parsed {} transactions", parsed_txs.len());

    // Collect transaction references for account loading
    let tx_refs: Vec<_> = parsed_txs.iter().map(|p| &p.transaction).collect();

    // Load accounts for all transactions
    let account_loader =
        account_loader::AccountLoader::new(rpc_url.to_string(), load_accounts, offline)?;
    let mut resolved_accounts =
        account_loader.load_for_transactions(&tx_refs, &sim_opts.replacements)?;

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
        match parser_registry.load_idl_parsers_for_programs(program_ids) {
            Ok(count) if count > 0 => log::info!("Lazy-loaded {} IDL parsers", count),
            Ok(_) => {}
            Err(err) => log::warn!("Failed to load IDL parsers: {:?}", err),
        }
    }

    // Dump original RPC account data before --replace / --patch-data
    if let Some(ref dump_dir) = dump_accounts {
        executor::dump_accounts_to_dir(&resolved_accounts.accounts, dump_dir)
            .context("Failed to dump accounts")?;
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
