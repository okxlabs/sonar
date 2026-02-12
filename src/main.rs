mod account_loader;
mod balance_changes;
mod cli;
mod config;
mod executor;
mod funding;
mod handlers;
mod instruction_parsers;
mod log_parser;
mod native_ids;
mod output;
mod progress;
mod token_account_decoder;
mod transaction;

use std::io::IsTerminal;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, ColorMode, Commands};

fn main() {
    if let Err(err) = run() {
        // Use alternate Display format ({:#}) for user-friendly single-line error chain
        // instead of Debug format ({:?}) which outputs the full anyhow backtrace
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    env_logger::init();

    // Load ~/.config/sonar/config.toml and inject values into env vars
    // before clap parses, so that CLI arg > env var > config file > default.
    config::load_and_apply();

    let cli = Cli::parse();

    // Initialize color control based on --color flag, NO_COLOR env var, and TTY detection
    // Reference: https://no-color.org
    match cli.color {
        ColorMode::Never => colored::control::set_override(false),
        ColorMode::Always => colored::control::set_override(true),
        ColorMode::Auto => {
            if std::env::var_os("NO_COLOR").is_some() || !std::io::stdout().is_terminal() {
                colored::control::set_override(false);
            }
        }
    }

    match cli.command {
        Commands::Simulate(args) => handlers::simulate::handle(args)?,
        Commands::Decode(args) => handlers::decode::handle(args)?,
        Commands::FetchIdl(args) => handlers::fetch_idl::handle(args)?,
        Commands::Account(args) => handlers::account::handle(args)?,
        Commands::Convert(args) => handlers::convert::handle(args)?,
        Commands::Pda(args) => handlers::pda::handle(args)?,
        Commands::ProgramData(args) => handlers::program_data::handle(args)?,
        Commands::Send(args) => handlers::send::handle(args)?,
        Commands::Completions(args) => {
            handlers::completions::handle(args);
            return Ok(());
        }
    }
    Ok(())
}
