mod account_loader;
mod cli;
mod executor;
mod output;
mod transaction;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, SimulateArgs};

fn main() {
    if let Err(err) = run() {
        eprintln!("执行失败: {err:?}");
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
    let SimulateArgs {
        tx,
        tx_file,
        rpc_url,
        replacements: replacement_args,
        output,
    } = args;

    let replacements = replacement_args
        .into_iter()
        .map(|raw| cli::parse_program_replacement(&raw).map_err(anyhow::Error::msg))
        .collect::<Result<Vec<_>>>()?;

    let raw_input = transaction::read_raw_transaction(tx, tx_file.as_deref())?;
    let parsed_tx = transaction::parse_raw_transaction(&raw_input)?;

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
