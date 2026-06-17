//! Resolve Anchor IDL files inside a directory by program ID.
//!
//! Historically an IDL file had to be named `<PROGRAM_ID>.json` to be picked
//! up. We still recognise that canonical layout (produced by auto-fetch and
//! `sonar idl sync`), but also match IDL files with arbitrary names (e.g.
//! `my_program.json`) by reading the `address` field defined in the Anchor IDL
//! spec, so users no longer have to rename their IDLs.
//!
//! Precedence is deterministic and does not depend on filesystem timestamps:
//! the canonical `<PROGRAM_ID>.json` always wins when present, and
//! arbitrarily-named files only fill the gap for programs without a canonical
//! file. When several arbitrarily-named files declare the same address, the
//! first in sorted filename order wins.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;
use solana_pubkey::Pubkey;

/// Resolve the IDL file for `program_id` inside `dir`.
///
/// Prefers the canonical `<program_id>.json`; otherwise matches any `*.json`
/// whose Anchor `address` field equals `program_id`.
///
/// Returns `None` if no matching IDL file exists.
pub fn resolve_idl_path(dir: &Path, program_id: &Pubkey) -> Option<PathBuf> {
    resolve_with_index(dir, program_id, &build_address_index(dir))
}

/// Resolve the IDL file for `program_id` against a prebuilt address `index`, so
/// callers that resolve many programs can share a single directory scan.
///
/// The canonical `<program_id>.json` takes precedence; the address-matched file
/// from `index` is used only when no canonical file exists.
pub fn resolve_with_index(
    dir: &Path,
    program_id: &Pubkey,
    index: &HashMap<Pubkey, PathBuf>,
) -> Option<PathBuf> {
    canonical_path(dir, program_id).or_else(|| index.get(program_id).cloned())
}

/// The canonical `<program_id>.json` path, if it exists as a file.
fn canonical_path(dir: &Path, program_id: &Pubkey) -> Option<PathBuf> {
    let path = dir.join(format!("{program_id}.json"));
    path.is_file().then_some(path)
}

/// Scan `dir` for arbitrarily-named IDL files and index them by the program ID
/// declared in their Anchor `address` field.
///
/// Canonical `<PROGRAM_ID>.json` files are skipped here — they are resolved by
/// name in [`resolve_idl_path`] / [`canonical_path`] — so a directory full of
/// cached IDLs (the common case for the default `~/.sonar/idls` cache) stays
/// cheap to index.
///
/// When two files declare the same address, the first in sorted filename order
/// wins, so the result is always deterministic.
pub fn build_address_index(dir: &Path) -> HashMap<Pubkey, PathBuf> {
    let mut paths: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
            .filter(|path| !has_pubkey_stem(path))
            .collect(),
        Err(_) => return HashMap::new(),
    };
    paths.sort();

    let entries = paths.into_iter().filter_map(|path| {
        let address = read_idl_address(&path)?;
        Some((address, path))
    });

    index_first_wins(entries)
}

/// Index entries by address, keeping the first file seen for each address.
/// Entries are expected in sorted filename order, which makes the outcome
/// deterministic on collisions.
fn index_first_wins(
    entries: impl IntoIterator<Item = (Pubkey, PathBuf)>,
) -> HashMap<Pubkey, PathBuf> {
    let mut index: HashMap<Pubkey, PathBuf> = HashMap::new();

    for (address, path) in entries {
        match index.get(&address) {
            Some(kept) => {
                log::debug!(
                    "IDL address {address} also declared by {} (shadowed by {})",
                    path.display(),
                    kept.display()
                );
            }
            None => {
                index.insert(address, path);
            }
        }
    }

    index
}

/// Whether the file stem is itself a valid base58 pubkey (the canonical
/// `<PROGRAM_ID>.json` layout handled by the fast path).
fn has_pubkey_stem(path: &Path) -> bool {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.parse::<Pubkey>().is_ok())
}

/// Read the program address an IDL file declares.
///
/// Supports both the Anchor 0.30+ top-level `address` field and the legacy
/// `metadata.address` location.
fn read_idl_address(path: &Path) -> Option<Pubkey> {
    let content = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&content).ok()?;

    let address = value
        .get("address")
        .and_then(Value::as_str)
        .or_else(|| value.get("metadata").and_then(|m| m.get("address")).and_then(Value::as_str))?;

    address.parse::<Pubkey>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sonar-idl-dir-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be valid")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn resolves_canonical_named_file() {
        let dir = temp_dir("canonical");
        let program_id = Pubkey::new_unique();
        let path = dir.join(format!("{program_id}.json"));
        std::fs::write(&path, "{}").unwrap();

        assert_eq!(resolve_idl_path(&dir, &program_id), Some(path));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolves_arbitrary_named_file_by_address_field() {
        let dir = temp_dir("by-address");
        let program_id = Pubkey::new_unique();
        let path = dir.join("my_program.json");
        std::fs::write(&path, format!(r#"{{ "address": "{program_id}" }}"#)).unwrap();

        assert_eq!(resolve_idl_path(&dir, &program_id), Some(path));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolves_legacy_metadata_address() {
        let dir = temp_dir("legacy");
        let program_id = Pubkey::new_unique();
        let path = dir.join("legacy.json");
        std::fs::write(&path, format!(r#"{{ "metadata": {{ "address": "{program_id}" }} }}"#))
            .unwrap();

        assert_eq!(resolve_idl_path(&dir, &program_id), Some(path));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn returns_none_when_no_match() {
        let dir = temp_dir("no-match");
        let path = dir.join("unrelated.json");
        std::fs::write(&path, format!(r#"{{ "address": "{}" }}"#, Pubkey::new_unique())).unwrap();

        assert_eq!(resolve_idl_path(&dir, &Pubkey::new_unique()), None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn canonical_name_wins_over_address_match() {
        let dir = temp_dir("precedence");
        let program_id = Pubkey::new_unique();

        // Canonical file (empty) and an arbitrarily-named file both target the
        // same program; the canonical name must win regardless of contents.
        let canonical = dir.join(format!("{program_id}.json"));
        std::fs::write(&canonical, "{}").unwrap();
        std::fs::write(dir.join("my_program.json"), format!(r#"{{ "address": "{program_id}" }}"#))
            .unwrap();

        assert_eq!(resolve_idl_path(&dir, &program_id), Some(canonical));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn address_collision_is_deterministic_by_filename() {
        let address = Pubkey::new_unique();

        // Entries arrive in sorted filename order; the first one wins.
        let index = index_first_wins([
            (address, PathBuf::from("a.json")),
            (address, PathBuf::from("b.json")),
        ]);
        assert_eq!(index.get(&address), Some(&PathBuf::from("a.json")));
    }

    #[test]
    fn index_skips_canonical_and_invalid_files() {
        let dir = temp_dir("index");
        let canonical_id = Pubkey::new_unique();
        let arbitrary_id = Pubkey::new_unique();

        std::fs::write(dir.join(format!("{canonical_id}.json")), "{}").unwrap();
        std::fs::write(dir.join("arbitrary.json"), format!(r#"{{ "address": "{arbitrary_id}" }}"#))
            .unwrap();
        std::fs::write(dir.join("not-json.txt"), "ignored").unwrap();
        std::fs::write(dir.join("no-address.json"), r#"{ "foo": "bar" }"#).unwrap();

        let index = build_address_index(&dir);
        assert_eq!(index.len(), 1);
        assert!(index.contains_key(&arbitrary_id));
        assert!(!index.contains_key(&canonical_id));

        std::fs::remove_dir_all(&dir).ok();
    }
}
