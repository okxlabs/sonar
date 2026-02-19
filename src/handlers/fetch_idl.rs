use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result};
use solana_pubkey::Pubkey;

use crate::core::account_loader;
use crate::cli::FetchIdlArgs;
use crate::utils::progress::Progress;

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

    let progress = Progress::new();
    let loader =
        account_loader::AccountLoader::new(args.rpc.rpc_url, None, false, Some(progress.clone()))?;

    let results = loader.fetch_idls(&program_ids);
    progress.finish();

    let mut not_found = Vec::new();
    let mut errors = Vec::new();

    for (program_id, result) in &results {
        match result {
            Ok(Some(idl_json)) => {
                // Pretty-print the JSON; fall back to original if parsing fails
                let formatted = match serde_json::from_str::<serde_json::Value>(idl_json) {
                    Ok(value) => {
                        serde_json::to_string_pretty(&value).unwrap_or_else(|_| idl_json.clone())
                    }
                    Err(_) => idl_json.clone(),
                };
                let path = output_dir.join(format!("{}.json", program_id));
                fs::write(&path, &formatted)
                    .with_context(|| format!("Failed to write IDL file: {}", path.display()))?;
                println!("{}", path.display());
            }
            Ok(None) => not_found.push(program_id),
            Err(e) => errors.push((program_id, e)),
        }
    }

    for id in &not_found {
        eprintln!("no IDL found: {}", id);
    }
    for (id, e) in &errors {
        eprintln!("error {}: {:#}", id, e);
    }

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

    eprintln!("syncing {} IDLs from {}", program_ids.len(), dir.display());
    Ok(program_ids)
}
