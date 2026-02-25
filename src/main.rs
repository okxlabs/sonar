mod cli;
mod converters;
mod core;
mod handlers;
mod output;
mod parsers;
mod utils;

use std::io::IsTerminal;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use cli::{Cli, Commands};

fn main() {
    if let Err(err) = run() {
        // Use alternate Display format ({:#}) for user-friendly single-line error chain
        // instead of Debug format ({:?}) which outputs the full anyhow backtrace
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}

/// Returns true when the user typed only a subcommand name after the binary,
/// with no subcommand-specific arguments.
fn is_bare_subcommand() -> bool {
    let arg_count = std::env::args().skip(1).count();
    arg_count <= 1
}

fn print_subcommand_help(name: &str) -> Result<()> {
    let mut cmd = Cli::command();
    let sub = cmd.find_subcommand_mut(name).expect("known subcommand");
    sub.print_help()?;
    std::process::exit(2);
}

fn run() -> Result<()> {
    env_logger::init();

    // Load ~/.config/sonar/config.toml and inject values into env vars
    // before clap parses, so that CLI arg > env var > config file > default.
    crate::utils::config::load_and_apply();

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            if matches!(
                err.kind(),
                clap::error::ErrorKind::MissingRequiredArgument
                    | clap::error::ErrorKind::MissingSubcommand
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) && is_bare_subcommand()
            {
                let mut args: Vec<String> = std::env::args().collect();
                args.push("--help".to_string());
                if let Err(help_err) = Cli::try_parse_from(&args) {
                    let _ = help_err.print();
                }
                std::process::exit(2);
            }
            err.exit();
        }
    };

    // Disable color when NO_COLOR is set (https://no-color.org) or stdout is not a TTY
    if std::env::var_os("NO_COLOR").is_some() || !std::io::stdout().is_terminal() {
        colored::control::set_override(false);
    }

    let command = match cli.command {
        Some(cmd) => cmd,
        None => {
            Cli::command().print_help()?;
            std::process::exit(2);
        }
    };

    match command {
        Commands::Simulate(args) => {
            if args.transaction.tx.is_empty() && std::io::stdin().is_terminal() {
                print_subcommand_help("simulate")?;
            }
            handlers::simulate::handle(args)?
        }
        Commands::Decode(args) => {
            if args.transaction.tx.is_empty() && std::io::stdin().is_terminal() {
                print_subcommand_help("decode")?;
            }
            handlers::decode::handle(args)?
        }
        Commands::Idl(args) => handlers::idl::handle(args)?,
        Commands::Account(args) => handlers::account::handle(args)?,
        Commands::Cache(args) => handlers::cache::handle(args)?,
        Commands::Convert(args) => handlers::convert::handle(args)?,
        Commands::Pda(args) => handlers::pda::handle(args)?,
        Commands::ProgramData(args) => handlers::program_elf::handle(args)?,
        Commands::Send(args) => handlers::send::handle(args)?,
        Commands::Config(args) => handlers::config::handle(args)?,
        Commands::Completions(args) => {
            handlers::completions::handle(args);
            return Ok(());
        }
    }
    Ok(())
}
