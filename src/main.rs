mod account_loader;
mod cli;
mod executor;
mod output;
mod transaction;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, ParseArgs, SimulateArgs, TransactionInputArgs};

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
        Commands::Parse(args) => handle_parse(args)?,
    }
    Ok(())
}

fn handle_simulate(args: SimulateArgs) -> Result<()> {
    let SimulateArgs {
        transaction,
        rpc_url,
        replacements: replacement_args,
    } = args;
    let TransactionInputArgs {
        tx,
        tx_file,
        output,
    } = transaction;

    let replacements = replacement_args
        .into_iter()
        .map(|raw| cli::parse_program_replacement(&raw).map_err(anyhow::Error::msg))
        .collect::<Result<Vec<_>>>()?;

    let raw_input = transaction::read_raw_transaction(tx.clone(), tx_file.as_deref())?;
    
    let parsed_tx = match transaction::parse_raw_transaction(&raw_input) {
        Ok(tx) => tx,
        Err(parse_err) => {
            if let Some(ref tx_str) = tx {
                if transaction::is_transaction_signature(tx_str) {
                    log::info!("Input appears to be a transaction signature, attempting to fetch from RPC...");
                    match transaction::fetch_transaction_from_rpc(&rpc_url, tx_str) {
                        Ok(fetched_tx) => transaction::parse_raw_transaction(&fetched_tx)?,
                        Err(fetch_err) => {
                            return Err(anyhow::anyhow!(
                                "Failed to parse as raw transaction: {}\nAlso failed to fetch as signature: {}",
                                parse_err,
                                fetch_err
                            ));
                        }
                    }
                } else {
                    return Err(parse_err);
                }
            } else {
                return Err(parse_err);
            }
        }
    };

    let account_loader = account_loader::AccountLoader::new(rpc_url)?;
    let prepared_accounts =
        account_loader.load_for_transaction(&parsed_tx.transaction, &replacements)?;

    let mut executor = executor::TransactionExecutor::prepare(prepared_accounts, replacements)?;
    let simulation = executor.simulate(&parsed_tx.transaction)?;

    output::render(
        &parsed_tx,
        executor.resolved_accounts(),
        &simulation,
        executor.replacements(),
        output,
    )?;
    Ok(())
}

fn handle_parse(args: ParseArgs) -> Result<()> {
    let ParseArgs {
        transaction,
        rpc_url,
    } = args;
    let TransactionInputArgs {
        tx,
        tx_file,
        output,
    } = transaction;

    let raw_input = transaction::read_raw_transaction(tx.clone(), tx_file.as_deref())?;
    
    let parsed_tx = match transaction::parse_raw_transaction(&raw_input) {
        Ok(tx) => tx,
        Err(parse_err) => {
            if let Some(ref tx_str) = tx {
                if transaction::is_transaction_signature(tx_str) {
                    log::info!("Input appears to be a transaction signature, attempting to fetch from RPC...");
                    match transaction::fetch_transaction_from_rpc(&rpc_url, tx_str) {
                        Ok(fetched_tx) => transaction::parse_raw_transaction(&fetched_tx)?,
                        Err(fetch_err) => {
                            return Err(anyhow::anyhow!(
                                "Failed to parse as raw transaction: {}\nAlso failed to fetch as signature: {}",
                                parse_err,
                                fetch_err
                            ));
                        }
                    }
                } else {
                    return Err(parse_err);
                }
            } else {
                return Err(parse_err);
            }
        }
    };

    let account_loader = account_loader::AccountLoader::new(rpc_url)?;
    let resolved_accounts = account_loader.load_for_transaction(&parsed_tx.transaction, &[])?;

    output::render_transaction_only(&parsed_tx, &resolved_accounts, output)?;
    Ok(())
}
