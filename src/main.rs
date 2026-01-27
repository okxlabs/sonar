mod account_loader;
mod cli;
mod executor;
mod funding;
mod instruction_parsers;
mod output;
mod transaction;

use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Commands, FetchIdlArgs, SimulateArgs, TransactionInputArgs};
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
        Commands::FetchIdl(args) => handle_fetch_idl(args)?,
    }
    Ok(())
}

fn handle_fetch_idl(args: FetchIdlArgs) -> Result<()> {
    // Determine program IDs from either --programs or --sync-dir
    let program_ids: Vec<Pubkey> = if let Some(ref programs) = args.programs {
        // Parse comma-separated program IDs
        programs
            .split(',')
            .map(|s| {
                Pubkey::from_str(s.trim())
                    .with_context(|| format!("Invalid program ID: {}", s.trim()))
            })
            .collect::<Result<Vec<_>>>()?
    } else if let Some(ref sync_dir) = args.sync_dir {
        // Scan directory for existing IDL files
        scan_idl_directory(sync_dir)?
    } else {
        return Err(anyhow::anyhow!("Must provide either --programs or --sync-dir"));
    };

    if program_ids.is_empty() {
        return Err(anyhow::anyhow!("No program IDs found"));
    }

    // Determine output directory: explicit --output-dir > --sync-dir > current directory
    let output_dir =
        args.output_dir.or_else(|| args.sync_dir.clone()).unwrap_or_else(|| PathBuf::from("."));

    // Create output directory
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("Failed to create output directory: {}", output_dir.display()))?;

    // Create account loader and fetch IDLs
    let loader = account_loader::AccountLoader::new(args.rpc_url)?;

    let mut success_count = 0;
    let mut not_found_count = 0;
    let mut error_count = 0;

    for program_id in &program_ids {
        match loader.fetch_idl(program_id) {
            Ok(Some(idl_json)) => {
                let path = output_dir.join(format!("{}.json", program_id));
                fs::write(&path, &idl_json)
                    .with_context(|| format!("Failed to write IDL file: {}", path.display()))?;
                println!("Saved IDL for {} to {}", program_id, path.display());
                success_count += 1;
            }
            Ok(None) => {
                eprintln!("No IDL found for program: {}", program_id);
                not_found_count += 1;
            }
            Err(e) => {
                eprintln!("Error fetching IDL for {}: {:?}", program_id, e);
                error_count += 1;
            }
        }
    }

    println!(
        "\nSummary: {} saved, {} not found, {} errors",
        success_count, not_found_count, error_count
    );

    Ok(())
}

/// Scans a directory for existing IDL files and extracts program IDs from filenames.
/// Files are expected to be named `<PROGRAM_ID>.json`.
fn scan_idl_directory(dir: &Path) -> Result<Vec<Pubkey>> {
    let mut program_ids = Vec::new();

    let entries = fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory: {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        // Only process .json files
        if path.extension().map_or(false, |ext| ext == "json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                match Pubkey::from_str(stem) {
                    Ok(pubkey) => {
                        program_ids.push(pubkey);
                    }
                    Err(_) => {
                        // Skip files that don't have a valid program ID as filename
                        log::debug!("Skipping file with invalid program ID name: {}", stem);
                    }
                }
            }
        }
    }

    if program_ids.is_empty() {
        return Err(anyhow::anyhow!("No valid IDL files found in directory: {}", dir.display()));
    }

    println!("Found {} IDL files to sync in {}", program_ids.len(), dir.display());
    Ok(program_ids)
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
        token_fundings: token_funding_args,
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

    let token_funding_requests = if parse_only {
        vec![]
    } else {
        token_funding_args
            .into_iter()
            .map(|raw| cli::parse_token_funding(&raw).map_err(anyhow::Error::msg))
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

            let account_loader = account_loader::AccountLoader::new(rpc_url.clone())?;
            let mut resolved_accounts =
                account_loader.load_for_transaction(&parsed_tx.transaction, &replacements)?;
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
                    prepared_token_fundings,
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
                    executor.token_fundings(),
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

    let account_loader = account_loader::AccountLoader::new(rpc_url)?;
    let mut resolved_accounts =
        account_loader.load_for_transaction(&parsed_tx.transaction, &replacements)?;
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
            prepared_token_fundings,
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
            executor.token_fundings(),
            &mut parser_registry,
            output,
            ix_data,
            verify_signatures,
        )?;
    }
    Ok(())
}

/// Collects executable program IDs from resolved accounts for IDL loading.
fn collect_program_ids(resolved_accounts: &account_loader::ResolvedAccounts) -> Vec<Pubkey> {
    let mut program_ids: Vec<_> = resolved_accounts
        .accounts
        .iter()
        .filter(|(_, account)| account.executable)
        .map(|(pubkey, _)| *pubkey)
        .collect();

    program_ids.sort();
    program_ids.dedup();

    if program_ids.is_empty() {
        log::error!("No executable accounts found; IDL parsers will not be loaded");
    }

    program_ids
}

#[cfg(test)]
mod tests {
    use super::collect_program_ids;
    use crate::account_loader;
    use solana_account::Account;
    use solana_pubkey::Pubkey;
    use solana_sdk_ids::system_program;
    use std::collections::HashMap;

    fn executable_account() -> Account {
        Account {
            lamports: 0,
            data: Vec::new(),
            owner: system_program::id(),
            executable: true,
            rent_epoch: 0,
        }
    }

    fn non_executable_account() -> Account {
        Account {
            lamports: 0,
            data: Vec::new(),
            owner: system_program::id(),
            executable: false,
            rent_epoch: 0,
        }
    }

    #[test]
    fn collect_program_ids_only_includes_executable_accounts() {
        let exec_a = Pubkey::new_unique();
        let exec_b = Pubkey::new_unique();
        let non_exec = Pubkey::new_unique();
        let mut accounts = HashMap::new();
        accounts.insert(exec_a, executable_account());
        accounts.insert(exec_b, executable_account());
        accounts.insert(non_exec, non_executable_account());

        let resolved = account_loader::ResolvedAccounts { accounts, lookups: vec![] };

        let program_ids = collect_program_ids(&resolved);

        assert_eq!(program_ids.len(), 2);
        assert!(program_ids.contains(&exec_a));
        assert!(program_ids.contains(&exec_b));
        assert!(!program_ids.contains(&non_exec));
    }

    #[test]
    fn collect_program_ids_returns_empty_when_no_executable_accounts() {
        let mut accounts = HashMap::new();
        accounts.insert(Pubkey::new_unique(), non_executable_account());
        accounts.insert(Pubkey::new_unique(), non_executable_account());

        let resolved = account_loader::ResolvedAccounts { accounts, lookups: vec![] };

        let program_ids = collect_program_ids(&resolved);

        assert!(program_ids.is_empty());
    }
}
