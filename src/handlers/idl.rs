use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result};
use solana_pubkey::Pubkey;

use crate::cli::{IdlAddressArgs, IdlArgs, IdlFetchArgs, IdlSubcommands, IdlSyncArgs};
use crate::core::account_loader;
use crate::utils::progress::Progress;

pub(crate) fn handle(args: IdlArgs) -> Result<()> {
    match args.command {
        IdlSubcommands::Fetch(args) => handle_fetch(args),
        IdlSubcommands::Sync(args) => handle_sync(args),
        IdlSubcommands::Address(args) => handle_address(args),
    }
}

fn handle_fetch(args: IdlFetchArgs) -> Result<()> {
    let program_ids = parse_program_ids(&args.programs)?;
    let output_dir = resolve_output_dir(args.output_dir, None);
    fetch_and_write_idls(program_ids, args.rpc.rpc_url, output_dir)
}

fn handle_sync(args: IdlSyncArgs) -> Result<()> {
    let (program_ids, default_output_dir) = collect_program_ids_from_sync_path(&args.path)?;
    let output_dir = resolve_output_dir(args.output_dir, default_output_dir.as_deref());
    fetch_and_write_idls(program_ids, args.rpc.rpc_url, output_dir)
}

fn handle_address(args: IdlAddressArgs) -> Result<()> {
    let program_id = Pubkey::from_str(args.program.trim())
        .with_context(|| format!("Invalid program ID: {}", args.program.trim()))?;
    let idl_address = account_loader::get_idl_address(&program_id)?;
    println!("{idl_address}");
    Ok(())
}

fn parse_program_ids(raw_programs: &[String]) -> Result<Vec<Pubkey>> {
    let program_ids = raw_programs
        .iter()
        .map(|s| {
            Pubkey::from_str(s.trim())
                .with_context(|| format!("Invalid program ID: {}", s.trim()))
        })
        .collect::<Result<Vec<_>>>()?;
    if program_ids.is_empty() {
        return Err(anyhow::anyhow!("No program IDs provided"));
    }
    Ok(program_ids)
}

fn fetch_and_write_idls(
    program_ids: Vec<Pubkey>,
    rpc_url: String,
    output_dir: PathBuf,
) -> Result<()> {
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("Failed to create output directory: {}", output_dir.display()))?;

    let progress = Progress::new();
    let loader = account_loader::AccountLoader::new(rpc_url, None, false, Some(progress.clone()))?;
    let results = loader.fetch_idls(&program_ids);
    progress.finish();

    let mut not_found = Vec::new();
    let mut errors = Vec::new();

    for (program_id, result) in &results {
        match result {
            Ok(Some(idl_json)) => {
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

fn resolve_output_dir(explicit: Option<PathBuf>, sync_source_dir: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }

    if let Some(path) = sync_source_dir {
        return path.to_path_buf();
    }

    if let Some(path) = default_idl_dir_from_env() {
        return path;
    }

    PathBuf::from(".")
}

fn default_idl_dir_from_env() -> Option<PathBuf> {
    let raw = std::env::var("SONAR_IDL_DIR").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

fn collect_program_ids_from_sync_path(path: &Path) -> Result<(Vec<Pubkey>, Option<PathBuf>)> {
    if path.is_dir() {
        let program_ids = scan_idl_directory(path)?;
        eprintln!("syncing {} IDLs from {}", program_ids.len(), path.display());
        return Ok((program_ids, Some(path.to_path_buf())));
    }

    if path.is_file() {
        let program_id = parse_program_id_from_idl_file(path)?;
        let output_dir = path.parent().map(|p| p.to_path_buf());
        eprintln!("syncing 1 IDL from {}", path.display());
        return Ok((vec![program_id], output_dir));
    }

    Err(anyhow::anyhow!(
        "Sync path does not exist or is not readable: {}",
        path.display()
    ))
}

/// Scans a directory for existing IDL files and extracts program IDs from filenames.
/// Files are expected to be named `<PROGRAM_ID>.json`.
fn scan_idl_directory(dir: &Path) -> Result<Vec<Pubkey>> {
    let mut program_ids = Vec::new();

    let entries =
        fs::read_dir(dir).with_context(|| format!("Failed to read directory: {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                match Pubkey::from_str(stem) {
                    Ok(pubkey) => program_ids.push(pubkey),
                    Err(_) => log::debug!("Skipping file with invalid program ID name: {}", stem),
                }
            }
        }
    }

    if program_ids.is_empty() {
        return Err(anyhow::anyhow!(
            "No valid IDL files found in directory: {}",
            dir.display()
        ));
    }

    Ok(program_ids)
}

fn parse_program_id_from_idl_file(path: &Path) -> Result<Pubkey> {
    if !path.extension().is_some_and(|ext| ext == "json") {
        return Err(anyhow::anyhow!(
            "Sync file must be a .json file named <PUBKEY>.json: {}",
            path.display()
        ));
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid sync file name: {}", path.display()))?;
    Pubkey::from_str(stem).with_context(|| {
        format!(
            "Sync file name must be <PUBKEY>.json, got invalid pubkey stem: {}",
            stem
        )
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{collect_program_ids_from_sync_path, parse_program_id_from_idl_file};

    fn unique_temp_dir(suffix: &str) -> std::path::PathBuf {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!(
            "sonar-idl-tests-{}-{}-{suffix}",
            std::process::id(),
            now
        ))
    }

    #[test]
    fn parse_program_id_from_valid_idl_file() {
        let path = std::path::Path::new("11111111111111111111111111111111.json");
        let pubkey = parse_program_id_from_idl_file(path).unwrap();
        assert_eq!(pubkey.to_string(), "11111111111111111111111111111111");
    }

    #[test]
    fn collect_sync_program_ids_from_directory() {
        let dir = unique_temp_dir("sync-dir");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("11111111111111111111111111111111.json"), "{}").unwrap();
        fs::write(dir.join("invalid.json"), "{}").unwrap();

        let (program_ids, output_dir) = collect_program_ids_from_sync_path(&dir).unwrap();
        assert_eq!(program_ids.len(), 1);
        assert_eq!(program_ids[0].to_string(), "11111111111111111111111111111111");
        assert_eq!(output_dir, Some(dir.clone()));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_sync_program_id_from_single_file() {
        let dir = unique_temp_dir("sync-file");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("11111111111111111111111111111111.json");
        fs::write(&file, "{}").unwrap();

        let (program_ids, output_dir) = collect_program_ids_from_sync_path(&file).unwrap();
        assert_eq!(program_ids.len(), 1);
        assert_eq!(program_ids[0].to_string(), "11111111111111111111111111111111");
        assert_eq!(output_dir, Some(dir.clone()));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_sync_program_id_rejects_invalid_file_stem() {
        let dir = unique_temp_dir("sync-invalid-file");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("not-a-pubkey.json");
        fs::write(&file, "{}").unwrap();

        let err = collect_program_ids_from_sync_path(&file).unwrap_err();
        assert!(err.to_string().contains("must be <PUBKEY>.json"));

        let _ = fs::remove_dir_all(&dir);
    }
}

