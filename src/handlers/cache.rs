use anyhow::{Context, Result};
use colored::Colorize;

use crate::cli::{CacheArgs, CacheCommands};
use crate::core::cache;

pub(crate) fn handle(args: CacheArgs) -> Result<()> {
    match args.command {
        CacheCommands::List => handle_list(),
        CacheCommands::Clean(args) => handle_clean(args),
        CacheCommands::Info(args) => handle_info(args),
    }
}

fn handle_list() -> Result<()> {
    let cache_root = cache::resolve_cache_dir(&None);
    if !cache_root.exists() {
        eprintln!("No cache directory found at {}", cache_root.display());
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&cache_root)
        .with_context(|| format!("Failed to read cache directory: {}", cache_root.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    if entries.is_empty() {
        eprintln!("Cache is empty");
        return Ok(());
    }

    entries.sort_by_key(|e| e.file_name());

    eprintln!("{} ({}):\n", "Cached entries".bold(), cache_root.display());

    for entry in &entries {
        let dir = entry.path();
        let key = entry.file_name().to_string_lossy().to_string();
        match cache::read_meta_json(&dir) {
            Ok(meta) => {
                let type_label = if meta.cache_type == "bundle" {
                    format!("bundle, {} txs", meta.inputs.len())
                } else {
                    "single".to_string()
                };
                eprintln!(
                    "  {} — {} accounts, {} ({})",
                    key.cyan(),
                    meta.account_count,
                    type_label,
                    meta.created_at,
                );
            }
            Err(_) => {
                let file_count = std::fs::read_dir(&dir)
                    .map(|rd| rd.filter_map(|e| e.ok()).count())
                    .unwrap_or(0);
                eprintln!("  {} — {} files (no metadata)", key.yellow(), file_count,);
            }
        }
    }

    eprintln!("\n{} entries total", entries.len());
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

fn handle_clean(args: crate::cli::CacheCleanArgs) -> Result<()> {
    let cache_root = cache::resolve_cache_dir(&None);
    if !cache_root.exists() {
        eprintln!("No cache directory found at {}", cache_root.display());
        return Ok(());
    }

    let min_age_days = match &args.older_than {
        Some(duration) => Some(parse_duration_days(duration)?),
        None => None,
    };

    let entries: Vec<_> = std::fs::read_dir(&cache_root)
        .with_context(|| format!("Failed to read cache directory: {}", cache_root.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    let mut removed = 0usize;
    for entry in &entries {
        let dir = entry.path();

        if let Some(max_age_days) = min_age_days {
            if let Ok(meta) = cache::read_meta_json(&dir) {
                if let Ok(created) = chrono::DateTime::parse_from_rfc3339(&meta.created_at) {
                    let age = chrono::Utc::now().signed_duration_since(created);
                    if age.num_days() < max_age_days as i64 {
                        continue;
                    }
                }
            }
        }

        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("Failed to remove cache entry: {}", dir.display()))?;
        removed += 1;
    }

    eprintln!("Removed {} cache entries", removed);
    Ok(())
}

fn handle_info(args: crate::cli::CacheInfoArgs) -> Result<()> {
    let cache_root = cache::resolve_cache_dir(&args.cache_dir);
    let dir = cache_root.join(&args.key);

    if !dir.exists() {
        anyhow::bail!("Cache entry not found: {}", args.key);
    }

    match cache::read_meta_json(&dir) {
        Ok(meta) => {
            eprintln!("{}: {}", "Key".bold(), args.key);
            eprintln!("{}: {}", "Type".bold(), meta.cache_type);
            eprintln!("{}: {}", "Created".bold(), meta.created_at);
            eprintln!("{}: {}", "RPC".bold(), meta.rpc_url);
            eprintln!("{}: {}", "Accounts".bold(), meta.account_count);
            eprintln!("{}: {}", "Sonar version".bold(), meta.sonar_version);
            if meta.inputs.len() > 1 || meta.cache_type == "bundle" {
                eprintln!("{}:", "Inputs".bold());
                for (i, input) in meta.inputs.iter().enumerate() {
                    let display = if input.len() > 44 {
                        format!("{}...", &input[..44])
                    } else {
                        input.clone()
                    };
                    eprintln!("  {}: {}", i + 1, display);
                }
            }
        }
        Err(_) => {
            eprintln!("{}: {} (no _meta.json found)", "Key".bold(), args.key);
        }
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
    eprintln!("{}: {} account files on disk", "Files".bold(), file_count);

    Ok(())
}
