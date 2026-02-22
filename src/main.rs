mod cli;
mod core;
mod handlers;
mod output;
mod parsers;
mod utils;

use std::io::IsTerminal;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use cli::{Cli, ColorMode, Commands};

fn main() {
    if let Err(err) = run() {
        // Use alternate Display format ({:#}) for user-friendly single-line error chain
        // instead of Debug format ({:?}) which outputs the full anyhow backtrace
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}

/// Returns true when the user typed only a subcommand name after the binary,
/// with no subcommand-specific arguments (global flags like --color are ignored).
fn is_bare_subcommand() -> bool {
    let known_global_flags: &[&str] = &["--color"];
    let mut args = std::env::args().skip(1); // skip binary name
    let mut non_global = 0u32;
    while let Some(arg) = args.next() {
        if known_global_flags.contains(&arg.as_str()) {
            args.next(); // skip the flag's value
        } else {
            non_global += 1;
        }
    }
    non_global <= 1
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
            ) && is_bare_subcommand()
            {
                // User typed just the subcommand name with no further arguments;
                // print subcommand help instead of the clap error.
                // Exit 2 (usage error) to distinguish from explicit --help (exit 0).
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

    let command = match cli.command {
        Some(cmd) => cmd,
        None => {
            Cli::command().print_help()?;
            std::process::exit(2);
        }
    };

    match command {
        Commands::Simulate(args) => handlers::simulate::handle(args)?,
        Commands::Decode(args) => handlers::decode::handle(args)?,
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
