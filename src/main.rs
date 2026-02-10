mod account_loader;
mod balance_changes;
mod cli;
mod executor;
mod funding;
mod instruction_parsers;
mod log_parser;
mod output;
mod token_account_decoder;
mod transaction;

use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use std::io::IsTerminal;

use anyhow::{Context, Result};
use clap::Parser;
use cli::{
    AccountArgs, Cli, ColorMode, Commands, ConvertArgs, DecodeArgs, FetchIdlArgs, PdaArgs,
    ProgramDataArgs, SendArgs, SimulateArgs, TransactionInputArgs,
};
use instruction_parsers::ParserRegistry;
use solana_pubkey::Pubkey;

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
        Commands::Simulate(args) => handle_simulate(args)?,
        Commands::Decode(args) => handle_decode(args)?,
        Commands::FetchIdl(args) => handle_fetch_idl(args)?,
        Commands::Account(args) => handle_account(args)?,
        Commands::Convert(args) => handle_convert(args)?,
        Commands::Pda(args) => handle_pda(args)?,
        Commands::ProgramData(args) => handle_program_data(args)?,
        Commands::Send(args) => handle_send(args)?,
    }
    Ok(())
}

fn handle_fetch_idl(args: FetchIdlArgs) -> Result<()> {
    // Determine program IDs from positional args or --sync-dir
    let program_ids: Vec<Pubkey> = if !args.programs.is_empty() {
        // Parse positional program IDs
        args.programs
            .iter()
            .map(|s| {
                Pubkey::from_str(s.trim())
                    .with_context(|| format!("Invalid program ID: {}", s.trim()))
            })
            .collect::<Result<Vec<_>>>()?
    } else if let Some(ref sync_dir) = args.sync_dir {
        // Scan directory for existing IDL files
        scan_idl_directory(sync_dir)?
    } else {
        return Err(anyhow::anyhow!("Must provide program IDs or --sync-dir"));
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
    let loader = account_loader::AccountLoader::new(args.rpc.rpc_url)?;

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

fn handle_account(args: AccountArgs) -> Result<()> {
    use instruction_parsers::anchor_idl::{IdlRegistry, RawAnchorIdl, parse_account_data};
    use solana_client::rpc_client::RpcClient;

    // Parse the account pubkey
    let account_pubkey = Pubkey::from_str(&args.account)
        .with_context(|| format!("Invalid account pubkey: {}", args.account))?;

    // Create RPC client and fetch the account
    let client = RpcClient::new(&args.rpc.rpc_url);
    let account = client
        .get_account(&account_pubkey)
        .with_context(|| format!("Failed to fetch account: {}", account_pubkey))?;

    let owner = account.owner;

    // If --raw is specified, just print raw data and return
    if args.raw {
        print_raw_account_data(&account);
        return Ok(());
    }

    // Try to decode as SPL Token or Token-2022 account (mint or token account)
    if let Some(token_json) = token_account_decoder::decode_spl_token_account(&account) {
        if args.no_account_meta {
            // Only print the parsed data part
            if let Some(data) = token_json.get("data") {
                println!("{}", serde_json::to_string_pretty(data)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&token_json)?);
            }
        } else {
            // Print the complete token_json (already contains account metadata and data)
            println!("{}", serde_json::to_string_pretty(&token_json)?);
        }
        return Ok(());
    }

    // Try to find IDL: first from local path, then from chain
    let idl_json = try_load_idl_from_path(&args.idl_path, &owner).or_else(|| {
        let loader = account_loader::AccountLoader::new(args.rpc.rpc_url.clone()).ok()?;
        loader.fetch_idl(&owner).ok().flatten()
    });

    let idl_json = match idl_json {
        Some(json) => json,
        None => {
            // Output raw data in Solana JSON RPC format
            print_raw_account_data(&account);
            return Ok(());
        }
    };

    // Parse the IDL
    let raw_idl: RawAnchorIdl =
        serde_json::from_str(&idl_json).with_context(|| "Failed to parse IDL JSON")?;
    let idl = raw_idl.convert(&owner.to_string());

    // Create an empty registry (we only have one IDL)
    let registry = IdlRegistry::new();

    // Parse the account data
    match parse_account_data(&idl, &account.data, &registry)? {
        Some((_type_name, parsed_value)) => {
            if args.no_account_meta {
                println!("{}", serde_json::to_string_pretty(&parsed_value)?);
            } else {
                let output = serde_json::json!({
                    "lamports": account.lamports,
                    "space": account.data.len(),
                    "owner": account.owner.to_string(),
                    "executable": account.executable,
                    "rentEpoch": account.rent_epoch,
                    "data": parsed_value
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
        }
        None => {
            let output = if args.no_account_meta {
                if account.data.len() >= 8 {
                    serde_json::json!({
                        "error": "No matching account type found",
                        "discriminator": hex::encode(&account.data[..8]),
                        "raw_data": hex::encode(&account.data)
                    })
                } else {
                    serde_json::json!({
                        "error": "Account data too short",
                        "raw_data": hex::encode(&account.data)
                    })
                }
            } else if account.data.len() >= 8 {
                serde_json::json!({
                    "lamports": account.lamports,
                    "space": account.data.len(),
                    "owner": account.owner.to_string(),
                    "executable": account.executable,
                    "rentEpoch": account.rent_epoch,
                    "error": "No matching account type found",
                    "discriminator": hex::encode(&account.data[..8]),
                    "raw_data": hex::encode(&account.data)
                })
            } else {
                serde_json::json!({
                    "lamports": account.lamports,
                    "space": account.data.len(),
                    "owner": account.owner.to_string(),
                    "executable": account.executable,
                    "rentEpoch": account.rent_epoch,
                    "error": "Account data too short",
                    "raw_data": hex::encode(&account.data)
                })
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }

    Ok(())
}

/// Try to load IDL from local path (if specified).
/// IDL files are expected to be named `<PROGRAM_ID>.json`.
fn try_load_idl_from_path(idl_path: &Option<PathBuf>, owner: &Pubkey) -> Option<String> {
    let path = idl_path.as_ref()?;
    let idl_file = path.join(format!("{}.json", owner));

    if idl_file.exists() {
        match fs::read_to_string(&idl_file) {
            Ok(content) => {
                log::debug!("Loaded IDL from {}", idl_file.display());
                Some(content)
            }
            Err(e) => {
                log::warn!("Failed to read IDL file {}: {}", idl_file.display(), e);
                None
            }
        }
    } else {
        log::debug!("IDL file not found: {}", idl_file.display());
        None
    }
}

/// Print account data in Solana JSON RPC format.
/// Field order follows Solana Account struct: lamports, data, owner, executable, rent_epoch
fn print_raw_account_data(account: &solana_account::Account) {
    use base64::{Engine as _, engine::general_purpose};
    let data_b64 = general_purpose::STANDARD.encode(&account.data);
    let output = serde_json::json!({
        "lamports": account.lamports,
        "data": data_b64,
        "owner": account.owner.to_string(),
        "executable": account.executable,
        "rentEpoch": account.rent_epoch,
        "space": account.data.len()
    });
    println!("{}", serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()));
}

fn handle_convert(args: ConvertArgs) -> Result<()> {
    let output = cli::convert(&args).map_err(|e| anyhow::anyhow!("{}", e))?;
    println!("{}", output);
    Ok(())
}

fn handle_pda(args: PdaArgs) -> Result<()> {
    let program_id = Pubkey::from_str(&args.program_id)
        .with_context(|| format!("Invalid program ID: {}", args.program_id))?;

    let parsed_seeds = cli::parse_seeds(&args.seeds)
        .map_err(|e| anyhow::anyhow!("Failed to parse seeds: {}", e))?;

    let seed_bytes = cli::seeds_to_bytes(&parsed_seeds)
        .map_err(|e| anyhow::anyhow!("Failed to convert seeds to bytes: {}", e))?;

    // Convert Vec<Vec<u8>> to Vec<&[u8]> for find_program_address
    let seed_slices: Vec<&[u8]> = seed_bytes.iter().map(|v| v.as_slice()).collect();

    let (pda, bump) = Pubkey::find_program_address(&seed_slices, &program_id);

    println!("PDA: {}", pda);
    println!("Bump: {}", bump);

    Ok(())
}

fn handle_send(args: SendArgs) -> Result<()> {
    use solana_client::rpc_client::RpcClient;
    use solana_client::rpc_config::RpcSendTransactionConfig;

    let parsed = transaction::parse_raw_transaction(&args.tx)?;
    let client = RpcClient::new(&args.rpc.rpc_url);

    let config =
        RpcSendTransactionConfig { skip_preflight: args.skip_preflight, ..Default::default() };

    let signature = client
        .send_transaction_with_config(&parsed.transaction, config)
        .context("Failed to send transaction")?;

    println!("{}", signature);
    Ok(())
}

fn handle_program_data(args: ProgramDataArgs) -> Result<()> {
    use sha2::{Digest, Sha256};
    use solana_client::rpc_client::RpcClient;
    use solana_loader_v3_interface::state::UpgradeableLoaderState;
    use solana_sdk_ids::bpf_loader_upgradeable;
    use std::io::Write;

    // Header sizes for different account types
    const PROGRAM_DATA_HEADER_SIZE: usize = 45;
    const BUFFER_HEADER_SIZE: usize = 37;

    let address = Pubkey::from_str(&args.address)
        .with_context(|| format!("Invalid address: {}", args.address))?;

    let client = RpcClient::new(&args.rpc.rpc_url);

    let elf_data = if args.buffer {
        // Buffer mode: fetch buffer account directly
        let account = client
            .get_account(&address)
            .with_context(|| format!("Failed to fetch buffer account: {}", address))?;

        if account.owner != bpf_loader_upgradeable::id() {
            return Err(anyhow::anyhow!(
                "Account {} is not owned by BPF Loader Upgradeable (owner: {})",
                address,
                account.owner
            ));
        }

        // Verify it's a Buffer account
        let state: UpgradeableLoaderState = bincode::deserialize(&account.data)
            .with_context(|| "Failed to deserialize buffer account state")?;

        match state {
            UpgradeableLoaderState::Buffer { .. } => {}
            _ => {
                return Err(anyhow::anyhow!("Account {} is not a Buffer account", address));
            }
        }

        if account.data.len() <= BUFFER_HEADER_SIZE {
            return Err(anyhow::anyhow!(
                "Buffer account data too short: {} bytes",
                account.data.len()
            ));
        }

        account.data[BUFFER_HEADER_SIZE..].to_vec()
    } else {
        // Program mode: fetch program account and then program data account
        let account = client
            .get_account(&address)
            .with_context(|| format!("Failed to fetch program account: {}", address))?;

        if account.owner != bpf_loader_upgradeable::id() {
            return Err(anyhow::anyhow!(
                "Account {} is not owned by BPF Loader Upgradeable (owner: {})",
                address,
                account.owner
            ));
        }

        // Parse program account to get programdata address
        let state: UpgradeableLoaderState = bincode::deserialize(&account.data)
            .with_context(|| "Failed to deserialize program account state")?;

        let programdata_address = match state {
            UpgradeableLoaderState::Program { programdata_address } => {
                Pubkey::new_from_array(programdata_address.to_bytes())
            }
            _ => {
                return Err(anyhow::anyhow!("Account {} is not a Program account", address));
            }
        };

        // Fetch program data account
        let programdata_account = client.get_account(&programdata_address).with_context(|| {
            format!("Failed to fetch program data account: {}", programdata_address)
        })?;

        if programdata_account.data.len() <= PROGRAM_DATA_HEADER_SIZE {
            return Err(anyhow::anyhow!(
                "Program data account too short: {} bytes",
                programdata_account.data.len()
            ));
        }

        programdata_account.data[PROGRAM_DATA_HEADER_SIZE..].to_vec()
    };

    // Handle verification or output
    if let Some(expected_hash) = args.verify_sha256 {
        // Compute SHA256 hash
        let mut hasher = Sha256::new();
        hasher.update(&elf_data);
        let actual_hash = hex::encode(hasher.finalize());

        // Normalize expected hash (remove 0x prefix if present, lowercase)
        let expected_hash =
            expected_hash.strip_prefix("0x").unwrap_or(&expected_hash).to_lowercase();

        if actual_hash == expected_hash {
            println!("true");
            Ok(())
        } else {
            println!("false");
            std::process::exit(1);
        }
    } else if let Some(output_path) = args.output {
        // Write to specified file
        fs::write(&output_path, &elf_data).with_context(|| {
            format!("Failed to write program data to {}", output_path.display())
        })?;
        eprintln!("Wrote {} bytes to {}", elf_data.len(), output_path.display());
        Ok(())
    } else {
        // Output raw binary data to stdout
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(&elf_data).with_context(|| "Failed to write program data to stdout")?;
        Ok(())
    }
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

fn handle_decode(args: DecodeArgs) -> Result<()> {
    let idl_path = args.idl_path.clone();
    let mut parser_registry = ParserRegistry::new(idl_path);

    let DecodeArgs { transaction, rpc, ix_data, idl_path: _ } = args;
    let rpc_url = rpc.rpc_url;
    let TransactionInputArgs { tx, tx_file, output } = transaction;

    // Check if this is a bundle (multiple positional TX arguments)
    if tx.len() > 1 {
        return handle_bundle_decode(tx, &rpc_url, ix_data, output, &mut parser_registry);
    }

    // Single tx: take the first positional arg, or fall back to --tx-file
    let tx_single = tx.into_iter().next();
    let raw_input = transaction::read_raw_transaction(tx_single.clone(), tx_file.as_deref())?;

    // Check if input looks like a transaction signature first
    let raw_tx = if let Some(ref tx_str) = tx_single {
        if transaction::is_transaction_signature(tx_str) {
            log::info!(
                "Input appears to be a transaction signature, attempting to fetch from RPC..."
            );
            transaction::fetch_transaction_from_rpc(&rpc_url, tx_str)?
        } else {
            raw_input
        }
    } else {
        raw_input
    };

    let parsed_tx = transaction::parse_raw_transaction(&raw_tx)?;

    let account_loader = account_loader::AccountLoader::new(rpc_url)?;
    let resolved_accounts = account_loader.load_for_transaction(&parsed_tx.transaction, &[])?;

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

    output::render_transaction_only(
        &parsed_tx,
        &resolved_accounts,
        &mut parser_registry,
        output,
        ix_data,
    )?;

    Ok(())
}

/// Handle bundle decode (multiple transactions decoded without simulation).
fn handle_bundle_decode(
    tx_inputs: Vec<String>,
    rpc_url: &str,
    ix_data: bool,
    output_format: cli::OutputFormat,
    parser_registry: &mut instruction_parsers::ParserRegistry,
) -> Result<()> {
    log::info!("Bundle decode mode: {} transactions", tx_inputs.len());

    let parsed_txs = transaction::parse_multi_raw_transactions(&tx_inputs, rpc_url)?;
    log::info!("Successfully parsed {} transactions", parsed_txs.len());

    let tx_refs: Vec<_> = parsed_txs.iter().map(|p| &p.transaction).collect();

    let account_loader = account_loader::AccountLoader::new(rpc_url.to_string())?;
    let resolved_accounts = account_loader.load_for_transactions(&tx_refs, &[])?;

    let program_ids = collect_program_ids(&resolved_accounts);
    if !program_ids.is_empty() {
        match parser_registry.load_idl_parsers_for_programs(program_ids) {
            Ok(count) if count > 0 => log::info!("Lazy-loaded {} IDL parsers", count),
            Ok(_) => {}
            Err(err) => log::warn!("Failed to load IDL parsers: {:?}", err),
        }
    }

    for (i, parsed_tx) in parsed_txs.iter().enumerate() {
        println!("\n=== Transaction {} of {} ===", i + 1, parsed_txs.len());
        output::render_transaction_only(
            parsed_tx,
            &resolved_accounts,
            parser_registry,
            output_format,
            ix_data,
        )?;
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
        rpc,
        replacements: replacement_args,
        fundings: funding_args,
        token_fundings: token_funding_args,
        ix_data,
        verify_signatures,
        idl_path: _,
        show_balance_change,
        show_raw_log,
        show_ix_detail,
    } = args;
    let rpc_url = rpc.rpc_url;

    let balance_opts = output::BalanceChangeOptions { show_balance_change };
    let log_opts = output::LogDisplayOptions { show_raw_log };
    let TransactionInputArgs { tx, tx_file, output } = transaction;

    let replacements = replacement_args
        .into_iter()
        .map(|raw| cli::parse_program_replacement(&raw).map_err(anyhow::Error::msg))
        .collect::<Result<Vec<_>>>()?;

    let fundings = funding_args
        .into_iter()
        .map(|raw| cli::parse_funding(&raw).map_err(anyhow::Error::msg))
        .collect::<Result<Vec<_>>>()?;

    let token_funding_requests = token_funding_args
        .into_iter()
        .map(|raw| cli::parse_token_funding(&raw).map_err(anyhow::Error::msg))
        .collect::<Result<Vec<_>>>()?;

    // Check if this is a bundle (multiple positional TX arguments)
    if tx.len() > 1 {
        // Bundle simulation mode
        return handle_bundle_simulate(
            tx,
            &rpc_url,
            replacements,
            fundings,
            token_funding_requests,
            ix_data,
            verify_signatures,
            output,
            &mut parser_registry,
            balance_opts,
            log_opts,
        );
    }

    // Single tx: take the first positional arg, or fall back to --tx-file
    let tx_single = tx.into_iter().next();
    let raw_input = transaction::read_raw_transaction(tx_single.clone(), tx_file.as_deref())?;

    // Check if input looks like a transaction signature first
    if let Some(ref tx_str) = tx_single {
        if transaction::is_transaction_signature(tx_str) {
            log::info!(
                "Input appears to be a transaction signature, attempting to fetch from RPC..."
            );
            let fetched_tx = transaction::fetch_transaction_from_rpc(&rpc_url, tx_str)?;
            let parsed_tx = transaction::parse_raw_transaction(&fetched_tx)?;

            let account_loader = account_loader::AccountLoader::new(rpc_url.clone())?;
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
                show_ix_detail,
                verify_signatures,
                balance_opts,
                log_opts,
            )?;

            return Ok(());
        }
    }

    // If not a signature, parse as raw transaction
    let parsed_tx = transaction::parse_raw_transaction(&raw_input)?;

    let account_loader = account_loader::AccountLoader::new(rpc_url)?;
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
        show_ix_detail,
        verify_signatures,
        balance_opts,
        log_opts,
    )?;

    Ok(())
}

/// Handle bundle simulation (multiple transactions executed sequentially).
fn handle_bundle_simulate(
    tx_inputs: Vec<String>,
    rpc_url: &str,
    replacements: Vec<cli::ProgramReplacement>,
    fundings: Vec<cli::Funding>,
    token_funding_requests: Vec<cli::TokenFunding>,
    ix_data: bool,
    verify_signatures: bool,
    output_format: cli::OutputFormat,
    parser_registry: &mut instruction_parsers::ParserRegistry,
    balance_opts: output::BalanceChangeOptions,
    log_opts: output::LogDisplayOptions,
) -> Result<()> {
    log::info!("Bundle simulation mode: {} transactions", tx_inputs.len());

    // Parse all transactions
    let parsed_txs = transaction::parse_multi_raw_transactions(&tx_inputs, rpc_url)?;
    log::info!("Successfully parsed {} transactions", parsed_txs.len());

    // Collect transaction references for account loading
    let tx_refs: Vec<_> = parsed_txs.iter().map(|p| &p.transaction).collect();

    // Load accounts for all transactions
    let account_loader = account_loader::AccountLoader::new(rpc_url.to_string())?;
    let mut resolved_accounts = account_loader.load_for_transactions(&tx_refs, &replacements)?;

    let parsed_tx_refs: Vec<_> = parsed_txs.iter().collect();
    warn_unmatched_addresses(
        &replacements,
        &fundings,
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

    // Load IDL parsers for all programs
    let program_ids = collect_program_ids(&resolved_accounts);
    if !program_ids.is_empty() {
        match parser_registry.load_idl_parsers_for_programs(program_ids) {
            Ok(count) if count > 0 => log::info!("Lazy-loaded {} IDL parsers", count),
            Ok(_) => {}
            Err(err) => log::warn!("Failed to load IDL parsers: {:?}", err),
        }
    }

    // Execute bundle simulation
    let total_tx_count = parsed_txs.len();
    let mut executor = executor::TransactionExecutor::prepare(
        resolved_accounts,
        replacements,
        fundings,
        prepared_token_fundings,
        verify_signatures,
    )?;

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
        output_format,
        ix_data,
        verify_signatures,
        balance_opts,
        log_opts,
    )?;

    Ok(())
}

/// Builds a set of all account keys referenced by the parsed transactions and their
/// resolved address lookup tables.
fn collect_transaction_account_keys(
    parsed_txs: &[&transaction::ParsedTransaction],
    resolved_accounts: &account_loader::ResolvedAccounts,
) -> std::collections::HashSet<Pubkey> {
    use std::collections::HashSet;

    let mut tx_keys: HashSet<Pubkey> = HashSet::new();
    for parsed_tx in parsed_txs {
        tx_keys.extend(parsed_tx.account_plan.static_accounts.iter());
    }
    for lookup in &resolved_accounts.lookups {
        tx_keys.extend(lookup.writable_addresses.iter());
        tx_keys.extend(lookup.readonly_addresses.iter());
    }
    tx_keys
}

/// Finds --replace program IDs that are not present in the given transaction account key set.
fn find_unmatched_replacements(
    replacements: &[cli::ProgramReplacement],
    tx_keys: &std::collections::HashSet<Pubkey>,
) -> Vec<Pubkey> {
    replacements.iter().filter(|r| !tx_keys.contains(&r.program_id)).map(|r| r.program_id).collect()
}

/// Finds --fund-sol pubkeys that are not present in the given transaction account key set.
fn find_unmatched_sol_fundings(
    fundings: &[cli::Funding],
    tx_keys: &std::collections::HashSet<Pubkey>,
) -> Vec<Pubkey> {
    fundings.iter().filter(|f| !tx_keys.contains(&f.pubkey)).map(|f| f.pubkey).collect()
}

/// Finds --fund-token pubkeys (account and mint) that are not present in the given
/// transaction account key set.
fn find_unmatched_token_fundings(
    token_fundings: &[cli::TokenFunding],
    tx_keys: &std::collections::HashSet<Pubkey>,
) -> Vec<Pubkey> {
    let mut unmatched = Vec::new();
    for tf in token_fundings {
        if !tx_keys.contains(&tf.account) {
            unmatched.push(tf.account);
        }
        if let Some(mint) = tf.mint {
            if !tx_keys.contains(&mint) {
                unmatched.push(mint);
            }
        }
    }
    unmatched
}

/// Warns the user when --replace, --fund-sol, or --fund-token addresses are not found
/// in the transaction's account keys, which likely indicates a typo.
fn warn_unmatched_addresses(
    replacements: &[cli::ProgramReplacement],
    fundings: &[cli::Funding],
    token_fundings: &[cli::TokenFunding],
    parsed_txs: &[&transaction::ParsedTransaction],
    resolved_accounts: &account_loader::ResolvedAccounts,
) {
    use colored::Colorize;

    if replacements.is_empty() && fundings.is_empty() && token_fundings.is_empty() {
        return;
    }

    let tx_keys = collect_transaction_account_keys(parsed_txs, resolved_accounts);

    for pubkey in find_unmatched_replacements(replacements, &tx_keys) {
        eprintln!(
            "{} --replace program ID {} is not referenced in the transaction's account keys. Did you mean a different address?",
            "Warning:".yellow().bold(),
            pubkey,
        );
    }

    for pubkey in find_unmatched_sol_fundings(fundings, &tx_keys) {
        eprintln!(
            "{} --fund-sol address {} is not referenced in the transaction's account keys. Did you mean a different address?",
            "Warning:".yellow().bold(),
            pubkey,
        );
    }

    for pubkey in find_unmatched_token_fundings(token_fundings, &tx_keys) {
        eprintln!(
            "{} --fund-token address {} is not referenced in the transaction's account keys. Did you mean a different address?",
            "Warning:".yellow().bold(),
            pubkey,
        );
    }
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
    use super::{
        collect_program_ids, find_unmatched_replacements, find_unmatched_sol_fundings,
        find_unmatched_token_fundings,
    };
    use crate::account_loader;
    use crate::cli;
    use solana_account::Account;
    use solana_pubkey::Pubkey;
    use solana_sdk_ids::system_program;
    use std::collections::{HashMap, HashSet};

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

    #[test]
    fn find_unmatched_sol_fundings_returns_empty_when_all_match() {
        let key_a = Pubkey::new_unique();
        let key_b = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [key_a, key_b].into_iter().collect();

        let fundings = vec![
            cli::Funding { pubkey: key_a, amount_lamports: 1_000_000_000 },
            cli::Funding { pubkey: key_b, amount_lamports: 2_000_000_000 },
        ];

        let unmatched = find_unmatched_sol_fundings(&fundings, &tx_keys);
        assert!(unmatched.is_empty());
    }

    #[test]
    fn find_unmatched_sol_fundings_detects_missing_address() {
        let key_in_tx = Pubkey::new_unique();
        let key_not_in_tx = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [key_in_tx].into_iter().collect();

        let fundings = vec![
            cli::Funding { pubkey: key_in_tx, amount_lamports: 1_000_000_000 },
            cli::Funding { pubkey: key_not_in_tx, amount_lamports: 2_000_000_000 },
        ];

        let unmatched = find_unmatched_sol_fundings(&fundings, &tx_keys);
        assert_eq!(unmatched.len(), 1);
        assert_eq!(unmatched[0], key_not_in_tx);
    }

    #[test]
    fn find_unmatched_sol_fundings_returns_empty_for_no_fundings() {
        let tx_keys: HashSet<Pubkey> = [Pubkey::new_unique()].into_iter().collect();
        let unmatched = find_unmatched_sol_fundings(&[], &tx_keys);
        assert!(unmatched.is_empty());
    }

    #[test]
    fn find_unmatched_token_fundings_detects_missing_account_and_mint() {
        let account_in_tx = Pubkey::new_unique();
        let mint_in_tx = Pubkey::new_unique();
        let account_not_in_tx = Pubkey::new_unique();
        let mint_not_in_tx = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [account_in_tx, mint_in_tx].into_iter().collect();

        let token_fundings = vec![
            cli::TokenFunding { account: account_in_tx, mint: Some(mint_in_tx), amount_raw: 100 },
            cli::TokenFunding {
                account: account_not_in_tx,
                mint: Some(mint_not_in_tx),
                amount_raw: 200,
            },
        ];

        let unmatched = find_unmatched_token_fundings(&token_fundings, &tx_keys);
        assert_eq!(unmatched.len(), 2);
        assert!(unmatched.contains(&account_not_in_tx));
        assert!(unmatched.contains(&mint_not_in_tx));
    }

    #[test]
    fn find_unmatched_token_fundings_returns_empty_when_all_match() {
        let account = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [account, mint].into_iter().collect();

        let token_fundings = vec![cli::TokenFunding { account, mint: Some(mint), amount_raw: 100 }];

        let unmatched = find_unmatched_token_fundings(&token_fundings, &tx_keys);
        assert!(unmatched.is_empty());
    }

    #[test]
    fn find_unmatched_replacements_detects_missing_program_id() {
        let prog_in_tx = Pubkey::new_unique();
        let prog_not_in_tx = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [prog_in_tx].into_iter().collect();

        let replacements = vec![
            cli::ProgramReplacement {
                program_id: prog_in_tx,
                so_path: std::path::PathBuf::from("/tmp/a.so"),
            },
            cli::ProgramReplacement {
                program_id: prog_not_in_tx,
                so_path: std::path::PathBuf::from("/tmp/b.so"),
            },
        ];

        let unmatched = find_unmatched_replacements(&replacements, &tx_keys);
        assert_eq!(unmatched.len(), 1);
        assert_eq!(unmatched[0], prog_not_in_tx);
    }

    #[test]
    fn find_unmatched_replacements_returns_empty_when_all_match() {
        let prog_a = Pubkey::new_unique();
        let prog_b = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [prog_a, prog_b].into_iter().collect();

        let replacements = vec![
            cli::ProgramReplacement {
                program_id: prog_a,
                so_path: std::path::PathBuf::from("/tmp/a.so"),
            },
            cli::ProgramReplacement {
                program_id: prog_b,
                so_path: std::path::PathBuf::from("/tmp/b.so"),
            },
        ];

        let unmatched = find_unmatched_replacements(&replacements, &tx_keys);
        assert!(unmatched.is_empty());
    }
}
