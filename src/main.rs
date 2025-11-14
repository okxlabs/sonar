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
    // Create parser registry and load IDL-based parsers
    let mut parser_registry = ParserRegistry::new();

    // Load IDL parsers from specified path or default directory
    let _idl_registry = match &args.idl_path {
        Some(idl_path) => {
            log::info!("Loading IDLs from custom path: {}", idl_path.display());
            match instruction_parsers::load_idl_parsers_from_path(idl_path) {
                Ok(idl_registry) => {
                    log::info!(
                        "Loaded {} IDL files for instruction parsing",
                        idl_registry.program_ids().len()
                    );
                    parser_registry.register_idl_parsers(&idl_registry);
                    idl_registry
                }
                Err(err) => {
                    log::warn!("Failed to load IDL parsers from custom path: {:?}", err);
                    instruction_parsers::IdlRegistry::new()
                }
            }
        }
        None => {
            // Load from default idl/ directory
            match instruction_parsers::load_idl_parsers() {
                Ok(idl_registry) => {
                    log::info!(
                        "Loaded {} IDL files for instruction parsing",
                        idl_registry.program_ids().len()
                    );
                    parser_registry.register_idl_parsers(&idl_registry);
                    idl_registry
                }
                Err(err) => {
                    log::warn!("Failed to load IDL parsers from default directory: {:?}", err);
                    instruction_parsers::IdlRegistry::new()
                }
            }
        }
    };

    log::debug!("Registered parsers for {} programs", parser_registry.parser_count());
    let SimulateArgs {
        transaction,
        rpc_url,
        replacements: replacement_args,
        fundings: funding_args,
        parse_only,
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

            let account_loader = account_loader::AccountLoader::new(rpc_url)?;
            let resolved_accounts =
                account_loader.load_for_transaction(&parsed_tx.transaction, &replacements)?;

            if parse_only {
                output::render_transaction_only(
                    &parsed_tx,
                    &resolved_accounts,
                    &parser_registry,
                    output,
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
                    &parser_registry,
                    output,
                    verify_signatures,
                )?;
            }
            return Ok(());
        }
    }

    // If not a signature, parse as raw transaction
    let parsed_tx = transaction::parse_raw_transaction(&raw_input)?;

    let account_loader = account_loader::AccountLoader::new(rpc_url)?;
    let resolved_accounts =
        account_loader.load_for_transaction(&parsed_tx.transaction, &replacements)?;

    if parse_only {
        output::render_transaction_only(&parsed_tx, &resolved_accounts, &parser_registry, output)?;
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
            &parser_registry,
            output,
            verify_signatures,
        )?;
    }
    Ok(())
}
