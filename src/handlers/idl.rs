use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result};
use serde::Serialize;
use solana_pubkey::Pubkey;

use crate::cli::{IdlAddressArgs, IdlArgs, IdlFetchArgs, IdlSubcommands, IdlSyncArgs};
use crate::core::idl_fetcher;
use crate::utils::progress::Progress;

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum IdlFetchStatus {
    Ok,
    NotFound,
    Error,
}

#[derive(Serialize)]
struct IdlFetchResult {
    program: String,
    path: Option<String>,
    status: IdlFetchStatus,
}

#[derive(Serialize)]
struct IdlAddressOutput {
    program: String,
    idl_address: String,
}

pub(crate) fn handle(args: IdlArgs, json: bool) -> Result<()> {
    match args.command {
        IdlSubcommands::Fetch(args) => handle_fetch(args, json),
        IdlSubcommands::Sync(args) => handle_sync(args, json),
        IdlSubcommands::Address(args) => handle_address(args, json),
    }
}

fn handle_fetch(args: IdlFetchArgs, json: bool) -> Result<()> {
    let program_ids = parse_program_ids(&args.programs)?;
    let output_dir = resolve_output_dir(args.output_dir, None);
    fetch_and_write_idls(program_ids, args.rpc.rpc_url, output_dir, args.allow_partial, json)
}

fn handle_sync(args: IdlSyncArgs, json: bool) -> Result<()> {
    let (program_ids, default_output_dir) = collect_program_ids_from_sync_path(&args.path)?;
    let output_dir = resolve_output_dir(args.output_dir, default_output_dir.as_deref());
    fetch_and_write_idls(program_ids, args.rpc.rpc_url, output_dir, args.allow_partial, json)
}

fn handle_address(args: IdlAddressArgs, json: bool) -> Result<()> {
    let program_id = Pubkey::from_str(args.program.trim())
        .with_context(|| format!("Invalid program ID: {}", args.program.trim()))?;
    let idl_address = idl_fetcher::get_idl_address(&program_id)?;

    if json {
        let output = IdlAddressOutput {
            program: program_id.to_string(),
            idl_address: idl_address.to_string(),
        };
        crate::output::print_json(&output)?;
    } else {
        println!("{idl_address}");
    }

    Ok(())
}

fn parse_program_ids(raw_programs: &[String]) -> Result<Vec<Pubkey>> {
    let program_ids = raw_programs
        .iter()
        .map(|s| {
            Pubkey::from_str(s.trim()).with_context(|| format!("Invalid program ID: {}", s.trim()))
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
    allow_partial: bool,
    json: bool,
) -> Result<()> {
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("Failed to create output directory: {}", output_dir.display()))?;

    let progress = Progress::new();
    let fetcher = idl_fetcher::IdlFetcher::new(rpc_url, Some(progress.clone()))?;
    let results = fetcher.fetch_idls(&program_ids);
    progress.finish();

    let mut fetched = 0usize;
    let mut not_found = Vec::new();
    let mut errors = Vec::new();
    let mut json_results: Vec<IdlFetchResult> = Vec::new();

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
                if json {
                    json_results.push(IdlFetchResult {
                        program: program_id.to_string(),
                        path: Some(path.display().to_string()),
                        status: IdlFetchStatus::Ok,
                    });
                } else {
                    println!("{}", path.display());
                }
                fetched += 1;
            }
            Ok(None) => {
                if json {
                    json_results.push(IdlFetchResult {
                        program: program_id.to_string(),
                        path: None,
                        status: IdlFetchStatus::NotFound,
                    });
                }
                not_found.push(program_id);
            }
            Err(e) => {
                if json {
                    json_results.push(IdlFetchResult {
                        program: program_id.to_string(),
                        path: None,
                        status: IdlFetchStatus::Error,
                    });
                }
                log::error!("IDL fetch error for {}: {:#}", program_id, e);
                errors.push(program_id);
            }
        }
    }

    if json {
        crate::output::print_json(&json_results)?;
    }

    for id in &not_found {
        log::warn!("no IDL found: {}", id);
    }

    let has_failures = !not_found.is_empty() || !errors.is_empty();
    if has_failures {
        log::warn!(
            "Summary: {} fetched, {} not found, {} error(s)",
            fetched,
            not_found.len(),
            errors.len()
        );

        if !allow_partial {
            return Err(anyhow::anyhow!(
                "IDL fetch/sync had failures ({} fetched, {} not found, {} error). Use --allow-partial to exit 0 when some programs fail.",
                fetched,
                not_found.len(),
                errors.len()
            ));
        }
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
        log::info!("syncing {} IDLs from {}", program_ids.len(), path.display());
        return Ok((program_ids, Some(path.to_path_buf())));
    }

    if path.is_file() {
        let program_id = parse_program_id_from_idl_file(path)?;
        let output_dir = path.parent().map(|p| p.to_path_buf());
        log::info!("syncing 1 IDL from {}", path.display());
        return Ok((vec![program_id], output_dir));
    }

    Err(anyhow::anyhow!("Sync path does not exist or is not readable: {}", path.display()))
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
        return Err(anyhow::anyhow!("No valid IDL files found in directory: {}", dir.display()));
    }

    Ok(program_ids)
}

fn parse_program_id_from_idl_file(path: &Path) -> Result<Pubkey> {
    if path.extension().is_none_or(|ext| ext != "json") {
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
        format!("Sync file name must be <PUBKEY>.json, got invalid pubkey stem: {}", stem)
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
