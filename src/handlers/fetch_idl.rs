use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result};
use solana_pubkey::Pubkey;

use crate::account_loader;
use crate::cli::FetchIdlArgs;

pub(crate) fn handle(args: FetchIdlArgs) -> Result<()> {
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
    let loader = account_loader::AccountLoader::new(args.rpc.rpc_url, None, false, None)?;

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
                eprintln!("Error fetching IDL for {}: {:#}", program_id, e);
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
        if path.extension().is_some_and(|ext| ext == "json") {
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
