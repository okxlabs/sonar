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
use base64::Engine;
use clap::Parser;
use cli::{
    AccountArgs, B2nArgs, B58B64Args, B64B58Args, Cli, Commands, FetchIdlArgs, N2bArgs, PdaArgs,
    ProgramDataArgs, SendArgs, SimulateArgs, TransactionInputArgs,
};
use instruction_parsers::ParserRegistry;
use num_bigint::BigUint;
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
        Commands::Account(args) => handle_account(args)?,
        Commands::B2n(args) => handle_b2n(args)?,
        Commands::N2b(args) => handle_n2b(args)?,
        Commands::Pda(args) => handle_pda(args)?,
        Commands::B64B58(args) => handle_b64_b58(args)?,
        Commands::B58B64(args) => handle_b58_b64(args)?,
        Commands::ProgramData(args) => handle_program_data(args)?,
        Commands::Send(args) => handle_send(args)?,
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

fn handle_account(args: AccountArgs) -> Result<()> {
    use instruction_parsers::anchor_idl::{IdlRegistry, RawAnchorIdl, parse_account_data};
    use solana_client::rpc_client::RpcClient;

    let verbose = args.verbose;

    // Parse the account pubkey
    let account_pubkey = Pubkey::from_str(&args.account)
        .with_context(|| format!("Invalid account pubkey: {}", args.account))?;

    // Create RPC client and fetch the account
    let client = RpcClient::new(&args.rpc_url);
    let account = client
        .get_account(&account_pubkey)
        .with_context(|| format!("Failed to fetch account: {}", account_pubkey))?;

    let owner = account.owner;
    if verbose {
        eprintln!("Account: {}", account_pubkey);
        eprintln!("Owner: {}", owner);
        eprintln!("Data length: {} bytes", account.data.len());
        eprintln!();
    }

    // If --raw is specified, just print raw data and return
    if args.raw {
        print_raw_account_data(&account);
        return Ok(());
    }

    // Try to find IDL: first from local path, then from chain
    let idl_json = try_load_idl_from_path(&args.idl_path, &owner).or_else(|| {
        if verbose {
            eprintln!("IDL not found locally, fetching from chain...");
        }
        let loader = account_loader::AccountLoader::new(args.rpc_url.clone()).ok()?;
        match loader.fetch_idl(&owner) {
            Ok(Some(json)) => Some(json),
            Ok(None) => None,
            Err(e) => {
                if verbose {
                    eprintln!("Failed to fetch IDL from chain: {:?}", e);
                }
                None
            }
        }
    });

    let idl_json = match idl_json {
        Some(json) => json,
        None => {
            if verbose {
                eprintln!("No Anchor IDL found for program: {}", owner);
            }
            // Output raw data in Solana JSON RPC format
            print_raw_account_data(&account);
            return Ok(());
        }
    };

    // Parse the IDL
    let raw_idl: RawAnchorIdl =
        serde_json::from_str(&idl_json).with_context(|| "Failed to parse IDL JSON")?;
    let idl = raw_idl.convert(&owner.to_string());

    if verbose {
        eprintln!("IDL: {} v{}", idl.metadata.name, idl.metadata.version);
        eprintln!();
    }

    // Create an empty registry (we only have one IDL)
    let registry = IdlRegistry::new();

    // Parse the account data
    match parse_account_data(&idl, &account.data, &registry)? {
        Some((type_name, parsed_value)) => {
            if verbose {
                eprintln!("Account type: {}", type_name);
                eprintln!();
            }
            // OrderedJsonValue implements Serialize, so we can use it directly
            println!("{}", serde_json::to_string_pretty(&parsed_value)?);
        }
        None => {
            // Output error as JSON object
            let output = if account.data.len() >= 8 {
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
fn print_raw_account_data(account: &solana_account::Account) {
    let data_hex = hex::encode(&account.data);
    let output = serde_json::json!({
        "data": data_hex,
        "executable": account.executable,
        "lamports": account.lamports,
        "owner": account.owner.to_string(),
        "rentEpoch": account.rent_epoch,
        "space": account.data.len()
    });
    println!("{}", serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()));
}

fn handle_b2n(args: B2nArgs) -> Result<()> {
    let bytes = if let Some(ref hex) = args.hex {
        cli::parse_bytes_input(hex, None)
    } else if let Some(ref hex_array) = args.hex_array {
        cli::parse_bytes_input(hex_array, Some(cli::ByteFormat::HexArray))
    } else if let Some(ref dec_array) = args.dec_array {
        cli::parse_bytes_input(dec_array, Some(cli::ByteFormat::DecArray))
    } else {
        return Err(anyhow::anyhow!("Must specify one of: HEX, -x, or -d"));
    }
    .map_err(|e| anyhow::anyhow!("Parse error: {}", e))?;

    let num = if args.be { BigUint::from_bytes_be(&bytes) } else { BigUint::from_bytes_le(&bytes) };

    println!("{}", num);
    Ok(())
}

fn handle_n2b(args: N2bArgs) -> Result<()> {
    let num = cli::parse_number(&args.number).map_err(|e| anyhow::anyhow!("Parse error: {}", e))?;

    let bytes = if args.be { num.to_bytes_be() } else { num.to_bytes_le() };

    // Determine output format (default: hex)
    let format = if args.hex_array {
        cli::ByteFormat::HexArray
    } else if args.dec_array {
        cli::ByteFormat::DecArray
    } else {
        cli::ByteFormat::Hex
    };

    println!("{}", cli::format_bytes(&bytes, format, args.space, args.prefix));
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

fn handle_b64_b58(args: B64B58Args) -> Result<()> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&args.input)
        .with_context(|| "Invalid base64 input")?;
    let result = bs58::encode(&bytes).into_string();
    println!("{}", result);
    Ok(())
}

fn handle_b58_b64(args: B58B64Args) -> Result<()> {
    let bytes = bs58::decode(&args.input).into_vec().with_context(|| "Invalid base58 input")?;
    let result = base64::engine::general_purpose::STANDARD.encode(&bytes);
    println!("{}", result);
    Ok(())
}

fn handle_send(args: SendArgs) -> Result<()> {
    use solana_client::rpc_client::RpcClient;
    use solana_client::rpc_config::RpcSendTransactionConfig;

    let parsed = transaction::parse_raw_transaction(&args.tx)?;
    let client = RpcClient::new(&args.rpc_url);

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

    let client = RpcClient::new(&args.rpc_url);

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
    if let Some(expected_hash) = args.verify {
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

    // Check if this is a bundle (multiple transactions separated by comma)
    if let Some(ref tx_str) = tx {
        let tx_inputs = cli::parse_multi_tx(tx_str);
        if tx_inputs.len() > 1 {
            // Bundle simulation mode
            return handle_bundle_simulate(
                tx_inputs,
                &rpc_url,
                replacements,
                fundings,
                token_funding_requests,
                parse_only,
                ix_data,
                verify_signatures,
                output,
                &mut parser_registry,
            );
        }
    }

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

/// Handle bundle simulation (multiple transactions executed sequentially).
fn handle_bundle_simulate(
    tx_inputs: Vec<String>,
    rpc_url: &str,
    replacements: Vec<cli::ProgramReplacement>,
    fundings: Vec<cli::Funding>,
    token_funding_requests: Vec<cli::TokenFunding>,
    parse_only: bool,
    ix_data: bool,
    verify_signatures: bool,
    output_format: cli::OutputFormat,
    parser_registry: &mut instruction_parsers::ParserRegistry,
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

    if parse_only {
        // For parse-only mode, just render all transactions without simulation
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
        return Ok(());
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
    )?;

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
