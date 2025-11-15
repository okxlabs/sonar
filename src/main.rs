mod account_loader;
mod cli;
mod executor;
mod instruction_parsers;
mod output;
mod transaction;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, SimulateArgs, TransactionInputArgs};
use instruction_parsers::ParserRegistry;
use solana_pubkey::Pubkey;

fn main() {
    if let Err(err) = run() {
        eprintln!("Execution failed: {err:?}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();
    match cli.command {
        Commands::Simulate(args) => handle_simulate(args)?,
    }
    Ok(())
}

fn handle_simulate(args: SimulateArgs) -> Result<()> {
    // Only attempt IDL-based parsing when the user explicitly supplies a directory
    let idl_path = args.idl_path.clone();
    let mut parser_registry = ParserRegistry::new(idl_path);

    log::debug!("Created parser registry with lazy IDL loading support");
    let SimulateArgs {
        transaction,
        rpc_url,
        replacements: replacement_args,
        fundings: funding_args,
        parse_only,
        ix_data,
        verify_signatures,
        idl_path: _,
    } = args;
    let TransactionInputArgs { tx, tx_file, output } = transaction;

    let replacements = if parse_only {
        vec![]
    } else {
        replacement_args
            .into_iter()
            .map(|raw| cli::parse_program_replacement(&raw).map_err(anyhow::Error::msg))
            .collect::<Result<Vec<_>>>()?
    };

    let fundings = if parse_only {
        vec![]
    } else {
        funding_args
            .into_iter()
            .map(|raw| cli::parse_funding(&raw).map_err(anyhow::Error::msg))
            .collect::<Result<Vec<_>>>()?
    };

    let raw_input = transaction::read_raw_transaction(tx.clone(), tx_file.as_deref())?;

    // Check if input looks like a transaction signature first
    if let Some(ref tx_str) = tx {
        if transaction::is_transaction_signature(tx_str) {
            log::info!(
                "Input appears to be a transaction signature, attempting to fetch from RPC..."
            );
            let fetched_tx = transaction::fetch_transaction_from_rpc(&rpc_url, tx_str)?;
            let parsed_tx = transaction::parse_raw_transaction(&fetched_tx)?;

            // Extract all program IDs (including those that might be used in inner instructions)
            let program_ids = collect_all_program_ids(&parsed_tx);

            // Load IDL parsers for all programs used in this transaction
            match parser_registry.load_idl_parsers_for_programs(program_ids) {
                Ok(count) if count > 0 => log::info!("Lazy-loaded {} IDL parsers", count),
                Ok(_) => {}
                Err(err) => log::warn!("Failed to load IDL parsers: {:?}", err),
            }

            let account_loader = account_loader::AccountLoader::new(rpc_url)?;
            let resolved_accounts =
                account_loader.load_for_transaction(&parsed_tx.transaction, &replacements)?;

            if parse_only {
                output::render_transaction_only(
                    &parsed_tx,
                    &resolved_accounts,
                    &mut parser_registry,
                    output,
                    ix_data,
                )?;
            } else {
                let mut executor = executor::TransactionExecutor::prepare(
                    resolved_accounts,
                    replacements,
                    fundings,
                    verify_signatures,
                )?;
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
                    &mut parser_registry,
                    output,
                    ix_data,
                    verify_signatures,
                )?;
            }
            return Ok(());
        }
    }

    // If not a signature, parse as raw transaction
    let parsed_tx = transaction::parse_raw_transaction(&raw_input)?;

    // Extract all program IDs (including from inner instructions) for lazy IDL loading
    let program_ids = collect_all_program_ids(&parsed_tx);

    // Load IDL parsers for all programs used in this transaction
    match parser_registry.load_idl_parsers_for_programs(program_ids) {
        Ok(count) if count > 0 => log::info!("Lazy-loaded {} IDL parsers", count),
        Ok(_) => {}
        Err(err) => log::warn!("Failed to load IDL parsers: {:?}", err),
    }

    let account_loader = account_loader::AccountLoader::new(rpc_url)?;
    let resolved_accounts =
        account_loader.load_for_transaction(&parsed_tx.transaction, &replacements)?;

    if parse_only {
        output::render_transaction_only(
            &parsed_tx,
            &resolved_accounts,
            &mut parser_registry,
            output,
            ix_data,
        )?;
    } else {
        let mut executor = executor::TransactionExecutor::prepare(
            resolved_accounts,
            replacements,
            fundings,
            verify_signatures,
        )?;
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
            &mut parser_registry,
            output,
            ix_data,
            verify_signatures,
        )?;
    }
    Ok(())
}

/// Collects all program IDs from the transaction account plan
/// In Solana, all accounts that could be accessed (including program accounts in inner instructions)
/// must be included in the transaction's account keys (static accounts or address lookups) before execution.
/// Therefore, we can extract all program IDs from the account plan without needing to parse inner instructions.
fn collect_all_program_ids(parsed_tx: &transaction::ParsedTransaction) -> Vec<Pubkey> {
    let mut program_ids = vec![];

    // Collect from static accounts (these are always present in the transaction)
    for program_id in &parsed_tx.account_plan.static_accounts {
        program_ids.push(*program_id);
    }

    // Collect from address lookup tables
    for lookup_plan in &parsed_tx.account_plan.address_lookups {
        // The account_key here is the lookup table account itself (which could be executable if it's a program)
        program_ids.push(lookup_plan.account_key);
    }

    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    program_ids.into_iter().filter(|id| seen.insert(*id)).collect()
}
