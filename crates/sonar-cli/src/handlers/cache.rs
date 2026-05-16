use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use colored::Colorize;
use serde::Serialize;

use crate::cli::{CacheArgs, CacheCommands};
use crate::core::cache;
use crate::core::cache::CacheMeta;

#[derive(Serialize)]
struct CacheListEntry {
    key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rpc_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sonar_version: Option<String>,
    /// Only present for entries without metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    file_count: Option<usize>,
}

#[derive(Serialize)]
struct CacheInfoTransaction {
    input: String,
    raw_tx: String,
    resolved_from: String,
}

#[derive(Serialize)]
struct CacheInfoOutput {
    key: String,
    cache_type: String,
    created_at: String,
    rpc_url: String,
    account_count: usize,
    sonar_version: String,
    transactions: Vec<CacheInfoTransaction>,
    account_files: usize,
}

#[derive(Serialize)]
struct CacheCleanOutput {
    removed: usize,
}

struct CacheEntry {
    path: PathBuf,
    key: String,
    meta: Option<CacheMeta>,
}

/// Reads the cache root directory, filters to directories, sorts by name,
/// and reads `_meta.json` for each entry (storing `None` on failure).
fn list_cache_entries(cache_root: &Path) -> Result<Vec<CacheEntry>> {
    let mut dir_entries: Vec<_> = std::fs::read_dir(cache_root)
        .with_context(|| format!("Failed to read cache directory: {}", cache_root.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    dir_entries.sort_by_key(|e| e.file_name());

    let entries = dir_entries
        .into_iter()
        .map(|e| {
            let path = e.path();
            let key = e.file_name().to_string_lossy().to_string();
            let meta = cache::read_meta_json(&path).ok();
            CacheEntry { path, key, meta }
        })
        .collect();

    Ok(entries)
}

pub(crate) fn handle(args: CacheArgs, json: bool) -> Result<()> {
    match args.command {
        CacheCommands::List => handle_list(json),
        CacheCommands::Clean(args) => handle_clean(args, json),
        CacheCommands::Info(args) => handle_info(args, json),
    }
}

fn handle_list(json: bool) -> Result<()> {
    let cache_root = cache::resolve_cache_dir(&None);
    if !cache_root.exists() {
        if json {
            println!("[]");
        } else {
            eprintln!("No cache directory found at {}", cache_root.display());
        }
        return Ok(());
    }

    let entries = list_cache_entries(&cache_root)?;

    if entries.is_empty() {
        if json {
            println!("[]");
        } else {
            eprintln!("Cache is empty");
        }
        return Ok(());
    }

    if json {
        let list: Vec<CacheListEntry> = entries
            .iter()
            .map(|entry| match &entry.meta {
                Some(meta) => CacheListEntry {
                    key: entry.key.clone(),
                    cache_type: Some(meta.cache_type.clone()),
                    account_count: Some(meta.account_count),
                    created_at: Some(meta.created_at.clone()),
                    rpc_url: Some(meta.rpc_url.clone()),
                    sonar_version: Some(meta.sonar_version.clone()),
                    file_count: None,
                },
                None => {
                    let fc = std::fs::read_dir(&entry.path)
                        .map(|rd| rd.filter_map(|e| e.ok()).count())
                        .unwrap_or(0);
                    CacheListEntry {
                        key: entry.key.clone(),
                        cache_type: None,
                        account_count: None,
                        created_at: None,
                        rpc_url: None,
                        sonar_version: None,
                        file_count: Some(fc),
                    }
                }
            })
            .collect();
        crate::output::print_json(&list)?;
    } else {
        println!("{} ({}):\n", "Cached entries".bold(), cache_root.display());

        for entry in &entries {
            match &entry.meta {
                Some(meta) => {
                    let type_label = if meta.cache_type == "bundle" {
                        format!("bundle, {} txs", meta.transactions.len())
                    } else {
                        "single".to_string()
                    };
                    println!(
                        "  {} — {} accounts, {} ({})",
                        entry.key.cyan(),
                        meta.account_count,
                        type_label,
                        meta.created_at,
                    );
                }
                None => {
                    let file_count = std::fs::read_dir(&entry.path)
                        .map(|rd| rd.filter_map(|e| e.ok()).count())
                        .unwrap_or(0);
                    println!("  {} — {} files (no metadata)", entry.key.yellow(), file_count,);
                }
            }
        }

        println!("\n{} entries total", entries.len());
    }
    Ok(())
}

fn parse_duration_days(s: &str) -> Result<u64> {
    let s = s.trim();
    if let Some(days) = s.strip_suffix('d') {
        days.parse::<u64>().with_context(|| format!("Invalid duration: {s}"))
    } else if let Some(hours) = s.strip_suffix('h') {
        let h = hours.parse::<u64>().with_context(|| format!("Invalid duration: {s}"))?;
        Ok(h / 24)
    } else {
        anyhow::bail!("Unsupported duration format: {s}. Use <N>d (days) or <N>h (hours)")
    }
}

fn handle_clean(args: crate::cli::CacheCleanArgs, json: bool) -> Result<()> {
    let cache_root = cache::resolve_cache_dir(&None);
    if !cache_root.exists() {
        if json {
            crate::output::print_json(&CacheCleanOutput { removed: 0 })?;
        } else {
            eprintln!("No cache directory found at {}", cache_root.display());
        }
        return Ok(());
    }

    let min_age_days = match &args.older_than {
        Some(duration) => Some(parse_duration_days(duration)?),
        None => None,
    };

    let entries = list_cache_entries(&cache_root)?;

    let mut removed = 0usize;
    for entry in &entries {
        if let Some(max_age_days) = min_age_days {
            if let Some(meta) = &entry.meta {
                if let Ok(created) = chrono::DateTime::parse_from_rfc3339(&meta.created_at) {
                    let age = chrono::Utc::now().signed_duration_since(created);
                    if age.num_days() < max_age_days as i64 {
                        continue;
                    }
                }
            }
        }

        std::fs::remove_dir_all(&entry.path)
            .with_context(|| format!("Failed to remove cache entry: {}", entry.path.display()))?;
        removed += 1;
    }

    if json {
        crate::output::print_json(&CacheCleanOutput { removed })?;
    } else {
        println!("Removed {} cache entries", removed);
    }
    Ok(())
}

fn handle_info(args: crate::cli::CacheInfoArgs, json: bool) -> Result<()> {
    let cache_root = cache::resolve_cache_dir(&args.cache_dir);
    let dir = cache_root.join(&args.key);

    if !dir.exists() {
        anyhow::bail!("Cache entry not found: {}", args.key);
    }

    let file_count = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    e.path().extension().map(|ext| ext == "json").unwrap_or(false)
                        && e.file_name() != "_meta.json"
                })
                .count()
        })
        .unwrap_or(0);

    let meta_result = cache::read_meta_json(&dir);

    if json {
        match meta_result {
            Ok(meta) => {
                let output = CacheInfoOutput {
                    key: args.key,
                    cache_type: meta.cache_type,
                    created_at: meta.created_at,
                    rpc_url: meta.rpc_url,
                    account_count: meta.account_count,
                    sonar_version: meta.sonar_version,
                    transactions: meta
                        .transactions
                        .into_iter()
                        .map(|tx| CacheInfoTransaction {
                            input: tx.input,
                            raw_tx: tx.raw_tx,
                            resolved_from: tx.resolved_from,
                        })
                        .collect(),
                    account_files: file_count,
                };
                crate::output::print_json(&output)?;
            }
            Err(_) => {
                let output = serde_json::json!({
                    "key": args.key,
                    "account_files": file_count,
                    "error": "no _meta.json found"
                });
                crate::output::print_json(&output)?;
            }
        }
    } else {
        match meta_result {
            Ok(meta) => {
                println!("{}: {}", "Key".bold(), args.key);
                println!("{}: {}", "Type".bold(), meta.cache_type);
                println!("{}: {}", "Created".bold(), meta.created_at);
                println!("{}: {}", "RPC".bold(), meta.rpc_url);
                println!("{}: {}", "Accounts".bold(), meta.account_count);
                println!("{}: {}", "Sonar version".bold(), meta.sonar_version);
                if meta.transactions.len() > 1 || meta.cache_type == "bundle" {
                    println!("{}:", "Transactions".bold());
                    for (i, tx) in meta.transactions.iter().enumerate() {
                        let display = if tx.input.len() > 44 {
                            format!("{}...", &tx.input[..44])
                        } else {
                            tx.input.clone()
                        };
                        println!("  {}: {} ({})", i + 1, display, tx.resolved_from);
                    }
                }
            }
            Err(_) => {
                println!("{}: {} (no _meta.json found)", "Key".bold(), args.key);
            }
        }
        println!("{}: {} account files on disk", "Files".bold(), file_count);
    }

    Ok(())
}
