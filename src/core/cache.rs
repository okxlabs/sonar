use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use solana_transaction::versioned::VersionedTransaction;

use crate::core::transaction;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct CacheTransaction {
    pub input: String,
    pub raw_tx: String,
    pub resolved_from: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct CacheMeta {
    pub created_at: String,
    pub sonar_version: String,
    #[serde(rename = "type")]
    pub cache_type: String,
    pub transactions: Vec<CacheTransaction>,
    pub rpc_url: String,
    pub account_count: usize,
}

pub(crate) fn resolve_cache_dir(cache_dir: &Option<PathBuf>) -> PathBuf {
    if let Some(dir) = cache_dir {
        return dir.clone();
    }
    if let Ok(dir) = std::env::var("SONAR_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".sonar").join("cache")
}

pub(crate) fn derive_cache_key_single(input: &str, tx: &VersionedTransaction) -> String {
    if transaction::is_transaction_signature(input) {
        return input.trim().to_string();
    }
    let sig = tx.signatures.first().map(|s| s.to_string()).unwrap_or_default();
    if sig.chars().all(|c| c == '1') {
        let msg_bytes = tx.message.serialize();
        let hash = Sha256::digest(&msg_bytes);
        hex::encode(&hash[..16])
    } else {
        sig
    }
}

pub(crate) fn derive_cache_key_bundle(
    inputs: &[String],
    txs: &[transaction::ParsedTransaction],
) -> String {
    let mut hasher = Sha256::new();
    for (input, tx) in inputs.iter().zip(txs.iter()) {
        let single_key = derive_cache_key_single(input, &tx.transaction);
        hasher.update(single_key.as_bytes());
        hasher.update(b":");
    }
    let hash = hasher.finalize();
    format!("bundle-{}", hex::encode(&hash[..16]))
}

/// Returns (cache_dir, offline).
/// offline is true when _meta.json exists and refresh_cache is false.
pub(crate) fn resolve_cache_state(
    cache: bool,
    cache_dir: &Option<PathBuf>,
    refresh_cache: bool,
    cache_key: &str,
) -> (Option<PathBuf>, bool) {
    if !cache {
        return (None, false);
    }
    let root = resolve_cache_dir(cache_dir);
    let dir = root.join(cache_key);
    let cache_complete = dir.join("_meta.json").exists();
    let offline = cache_complete && !refresh_cache;
    (Some(dir), offline)
}

pub(crate) fn write_meta_json(dir: &Path, meta: &CacheMeta) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("Failed to create cache directory: {}", dir.display()))?;
    let path = dir.join("_meta.json");
    let json = serde_json::to_string_pretty(meta).context("Failed to serialize cache metadata")?;
    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write cache metadata: {}", path.display()))?;
    Ok(())
}

pub(crate) fn read_meta_json(dir: &Path) -> Result<CacheMeta> {
    let path = dir.join("_meta.json");
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read cache metadata: {}", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse cache metadata: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_cache_dir_uses_provided_dir() {
        let dir = Some(PathBuf::from("/custom/cache"));
        assert_eq!(resolve_cache_dir(&dir), PathBuf::from("/custom/cache"));
    }

    #[test]
    fn resolve_cache_dir_uses_default_when_none() {
        let saved = std::env::var("SONAR_CACHE_DIR").ok();
        std::env::remove_var("SONAR_CACHE_DIR");

        let dir = resolve_cache_dir(&None);
        assert!(dir.to_string_lossy().ends_with(".sonar/cache"));

        if let Some(val) = saved {
            std::env::set_var("SONAR_CACHE_DIR", val);
        }
    }

    #[test]
    fn resolve_cache_state_returns_none_when_cache_disabled() {
        let (dir, offline) = resolve_cache_state(false, &None, false, "test-key");
        assert!(dir.is_none());
        assert!(!offline);
    }

    #[test]
    fn write_and_read_meta_json_roundtrip() {
        let dir = std::env::temp_dir().join(format!("sonar-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let meta = CacheMeta {
            created_at: "2026-02-22T10:00:00Z".to_string(),
            sonar_version: "0.2.0".to_string(),
            cache_type: "single".to_string(),
            transactions: vec![CacheTransaction {
                input: "test-sig".to_string(),
                raw_tx: "AQID".to_string(),
                resolved_from: "rpc".to_string(),
            }],
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            account_count: 5,
        };
        write_meta_json(&dir, &meta).unwrap();

        let read_back = read_meta_json(&dir).unwrap();
        assert_eq!(read_back.cache_type, "single");
        assert_eq!(read_back.account_count, 5);
        assert_eq!(read_back.transactions.len(), 1);
        assert_eq!(read_back.transactions[0].input, "test-sig");
        assert_eq!(read_back.transactions[0].resolved_from, "rpc");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_meta_json_rejects_legacy_inputs_schema() {
        let dir = std::env::temp_dir()
            .join(format!("sonar-cache-legacy-schema-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let legacy = serde_json::json!({
            "created_at": "2026-02-22T10:00:00Z",
            "sonar_version": "0.2.0",
            "type": "single",
            "inputs": ["legacy-input"],
            "rpc_url": "https://api.mainnet-beta.solana.com",
            "account_count": 1
        });
        std::fs::write(dir.join("_meta.json"), legacy.to_string()).unwrap();

        let err = read_meta_json(&dir).expect_err("legacy schema should fail to parse");
        assert!(err.to_string().contains("Failed to parse cache metadata"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn derive_cache_key_for_e2e_offline_tx() {
        let raw = "GPdrKqMbYtzsysuEJhYG4bpUB9xQFpdQ8ps9s8XorbfD5SA5FrFfMAL2oznLNP9Ah4wXPe6Y9BVkAXM7Gw47whMxuK5TCKvpKtkyDEiuYfaRZCmv1mk5u16HvPzQqXGHzmf3iFUraHA2yEghbqaJsUW27PmXWvs2xPhK1WtFBvF4PNxtFBNa7sGwHZPmT88zk5pwpVnseAu48HhDvY6Nj7qjTzRAAFczubznScT4aT1m5CNyYjVwYjR5iqc7PrpTzyAxevb1Zk1ndXgHfwnQAhZfKfV712i5z352Jbf96WdQFGva3f22NGSVWtSFjp6agEBDTvWVUa3Db4WvArURczERDymqEEhX5EfMSZTUYenfgRXL2kgjWoXkuFaDyumgapdyqzQFixL4aJZCEDp6yfq7V5g2WYqwqNXHBsKbfpTfqKsqCV1niunXSZfGTTRgXjFWXuQNbtLrbd9TTJmUhsTMJuPzhohT89yX292vmUDGvHv3YqkJGynKcEGT6cPrB6ayWiBGybsJ2fUiax7QFPKT1hscSm5HDJPV3HmrC3DyHQAWq6hrPzGMeUcEBfSEtkvPFtNe9kpw4N9x2bJwuiGRgHbVmzDnRGMdXpu9KWigYb3uebLTFLUDeDq1CfD377AzdxBkBJQkdQ3peTjAz1kW3pEQQLiE57p16Wf8oUgxCveHpGr73RCoveDsjeF7puENGkM2aFkmKLBRvW3yJHL9mCP6ZkuScMy9VCWkh554yEs72DEZU25Upj7RAAhc7zG7iWyP6m2gZg6gGZ5hqjrCasQXUjJkaPT4LgBeLS9W5scn7QA51QMZi95DgAvD9mQeSFixnFofpqNDTNWVnisoQQ2eAEPVwKC1sfKjBdMgrMJKG1JjzDM7DWRzR7xPYSyjPfHXHx5aZJ8LdyYjYXjS6dViihtH3sNebZzqLdDzERDEe6bAFAkB5tGcRdqF1kcdPN3HNeRgeA7xvJg5r3kaDkAQQ9yjZV2stexZ1eDa4KiRwBY3MsgujEntBT992CrU4uAtHKkSXUbusXHMjbx9Dn57HD9GEdzAnDq53Gmmz7xU2qKZ3hKhLZg3ZtYVTeAysqUSfZTRPp87VjdyrG2Msk32ufPdQbAZ2x9FYbUCPpRvLMPsNhDe5D4fMt9X63bQTsk9VQNx39j5Mo6NkYvcKmKiz3pb5J19bHnnSKixEKa89typqcbNFunudMMmAAT3egyok3WRCqwC83EwNBWwrdm5zs1mRefEDu77arj7E";
        let parsed = crate::core::transaction::parse_raw_transaction(raw).unwrap();
        let key = derive_cache_key_single(raw, &parsed.transaction);
        assert_eq!(key, "9afd5d16ef71a846c79292a284ab625a");
    }

    #[test]
    fn derive_cache_key_single_uses_signature_input() {
        let sig = "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQN";
        let raw = "GPdrKqMbYtzsysuEJhYG4bpUB9xQFpdQ8ps9s8XorbfD5SA5FrFfMAL2oznLNP9Ah4wXPe6Y9BVkAXM7Gw47whMxuK5TCKvpKtkyDEiuYfaRZCmv1mk5u16HvPzQqXGHzmf3iFUraHA2yEghbqaJsUW27PmXWvs2xPhK1WtFBvF4PNxtFBNa7sGwHZPmT88zk5pwpVnseAu48HhDvY6Nj7qjTzRAAFczubznScT4aT1m5CNyYjVwYjR5iqc7PrpTzyAxevb1Zk1ndXgHfwnQAhZfKfV712i5z352Jbf96WdQFGva3f22NGSVWtSFjp6agEBDTvWVUa3Db4WvArURczERDymqEEhX5EfMSZTUYenfgRXL2kgjWoXkuFaDyumgapdyqzQFixL4aJZCEDp6yfq7V5g2WYqwqNXHBsKbfpTfqKsqCV1niunXSZfGTTRgXjFWXuQNbtLrbd9TTJmUhsTMJuPzhohT89yX292vmUDGvHv3YqkJGynKcEGT6cPrB6ayWiBGybsJ2fUiax7QFPKT1hscSm5HDJPV3HmrC3DyHQAWq6hrPzGMeUcEBfSEtkvPFtNe9kpw4N9x2bJwuiGRgHbVmzDnRGMdXpu9KWigYb3uebLTFLUDeDq1CfD377AzdxBkBJQkdQ3peTjAz1kW3pEQQLiE57p16Wf8oUgxCveHpGr73RCoveDsjeF7puENGkM2aFkmKLBRvW3yJHL9mCP6ZkuScMy9VCWkh554yEs72DEZU25Upj7RAAhc7zG7iWyP6m2gZg6gGZ5hqjrCasQXUjJkaPT4LgBeLS9W5scn7QA51QMZi95DgAvD9mQeSFixnFofpqNDTNWVnisoQQ2eAEPVwKC1sfKjBdMgrMJKG1JjzDM7DWRzR7xPYSyjPfHXHx5aZJ8LdyYjYXjS6dViihtH3sNebZzqLdDzERDEe6bAFAkB5tGcRdqF1kcdPN3HNeRgeA7xvJg5r3kaDkAQQ9yjZV2stexZ1eDa4KiRwBY3MsgujEntBT992CrU4uAtHKkSXUbusXHMjbx9Dn57HD9GEdzAnDq53Gmmz7xU2qKZ3hKhLZg3ZtYVTeAysqUSfZTRPp87VjdyrG2Msk32ufPdQbAZ2x9FYbUCPpRvLMPsNhDe5D4fMt9X63bQTsk9VQNx39j5Mo6NkYvcKmKiz3pb5J19bHnnSKixEKa89typqcbNFunudMMmAAT3egyok3WRCqwC83EwNBWwrdm5zs1mRefEDu77arj7E";
        let parsed = crate::core::transaction::parse_raw_transaction(raw).unwrap();
        let key = derive_cache_key_single(sig, &parsed.transaction);
        assert_eq!(key, sig, "Valid signature input should be returned as-is");
    }

    #[test]
    fn derive_cache_key_bundle_is_order_sensitive() {
        use sha2::{Digest, Sha256};

        let mut hasher1 = Sha256::new();
        hasher1.update(b"key_a:");
        hasher1.update(b"key_b:");
        let hash1 = hasher1.finalize();
        let key1 = format!("bundle-{}", hex::encode(&hash1[..16]));

        let mut hasher2 = Sha256::new();
        hasher2.update(b"key_b:");
        hasher2.update(b"key_a:");
        let hash2 = hasher2.finalize();
        let key2 = format!("bundle-{}", hex::encode(&hash2[..16]));

        assert_ne!(key1, key2, "Different orderings should produce different cache keys");
    }
}
