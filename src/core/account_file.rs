use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use solana_account::{Account, ReadableAccount};
use solana_clock::Clock;
use solana_pubkey::Pubkey;
use solana_sdk_ids::system_program;
use solana_slot_hashes::SlotHashes;
use solana_sysvar_id::SysvarId;

use sonar_sim::ResolvedAccounts;

/// JSON structure for deserializing an account file (flat format).
/// Supports the simple `{ "lamports": ..., "data": ... }` format.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountJsonFlat {
    lamports: u64,
    data: AccountDataJson,
    owner: String,
    #[serde(default)]
    executable: bool,
    #[serde(default)]
    rent_epoch: u64,
}

/// JSON structure for deserializing a Solana CLI style account file (nested format).
/// Supports `{ "pubkey": "...", "account": { "lamports": ..., "data": ... } }`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountJsonNested {
    #[allow(dead_code)]
    pubkey: String,
    account: AccountJsonFlat,
}

/// Account data can be either a plain base64 string or a tuple `["base64data", "base64"]`.
#[derive(Deserialize)]
#[serde(untagged)]
enum AccountDataJson {
    Plain(String),
    Tuple(String, String),
}

pub fn parse_account_json(path: &Path) -> Result<Account, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|err| format!("Failed to read account file `{}`: {err}", path.display()))?;

    let json: AccountJsonFlat = if let Ok(nested) =
        serde_json::from_str::<AccountJsonNested>(&contents)
    {
        nested.account
    } else {
        serde_json::from_str(&contents)
            .map_err(|err| format!("Failed to parse account JSON `{}`: {err}", path.display()))?
    };

    let data_b64 = match &json.data {
        AccountDataJson::Plain(s) => s.clone(),
        AccountDataJson::Tuple(data, _encoding) => data.clone(),
    };

    let data = base64::engine::general_purpose::STANDARD
        .decode(&data_b64)
        .map_err(|err| format!("Failed to decode base64 data in `{}`: {err}", path.display()))?;

    let owner = Pubkey::from_str(&json.owner)
        .map_err(|err| format!("Failed to parse owner `{}`: {err}", json.owner))?;

    Ok(Account {
        lamports: json.lamports,
        data,
        owner,
        executable: json.executable,
        rent_epoch: json.rent_epoch,
    })
}

// ---------------------------------------------------------------------------
// Account dump (Solana CLI compatible JSON format)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DumpAccount {
    pubkey: String,
    account: DumpAccountInner,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DumpAccountInner {
    lamports: u64,
    data: (String, String),
    owner: String,
    executable: bool,
    rent_epoch: u64,
    space: usize,
}

static NATIVE_LOADER_ID: LazyLock<Pubkey> = LazyLock::new(|| {
    "NativeLoader1111111111111111111111111111111".parse().expect("invalid NativeLoader pubkey")
});

fn is_native_owner(account: &impl ReadableAccount) -> bool {
    *account.owner() == *NATIVE_LOADER_ID
}

/// Dump accounts to a directory in Solana CLI compatible JSON format.
///
/// Writes the original RPC-loaded account data (before --override / --patch-account-data).
/// Each account is written to `<pubkey>.json`. Native/system programs and
/// native-owned executable marker accounts are skipped.
pub(crate) fn dump_accounts_to_dir(
    resolved: &ResolvedAccounts,
    required_accounts: &[Pubkey],
    dir: &Path,
) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("Failed to create dump directory: {}", dir.display()))?;

    let accounts = &resolved.accounts;
    let lookup_related: HashSet<Pubkey> = resolved
        .lookups
        .iter()
        .flat_map(|lookup| {
            std::iter::once(lookup.account_key)
                .chain(lookup.writable_addresses.iter().copied())
                .chain(lookup.readonly_addresses.iter().copied())
        })
        .collect();
    let required_keys: HashSet<Pubkey> =
        required_accounts.iter().copied().chain(lookup_related.iter().copied()).collect();

    let mut dumped_keys = HashSet::new();

    for (pubkey, account) in accounts {
        if sonar_sim::is_native_or_sysvar(pubkey)
            && *pubkey != Clock::id()
            && *pubkey != SlotHashes::id()
        {
            continue;
        }
        if sonar_sim::is_litesvm_builtin_program(pubkey) {
            continue;
        }

        if is_native_owner(account) {
            continue;
        }

        let path = dir.join(format!("{pubkey}.json"));
        let plain = Account::from(account.clone());
        write_dump_account(pubkey, &plain, &path)?;

        dumped_keys.insert(*pubkey);
    }

    for pubkey in required_keys {
        if dumped_keys.contains(&pubkey)
            || sonar_sim::is_native_or_sysvar(&pubkey)
            || sonar_sim::is_litesvm_builtin_program(&pubkey)
        {
            continue;
        }

        let placeholder = Account {
            lamports: 0,
            data: Vec::new(),
            owner: system_program::id(),
            executable: false,
            rent_epoch: 0,
        };
        let path = dir.join(format!("{pubkey}.json"));
        write_dump_account(&pubkey, &placeholder, &path)?;
    }

    Ok(())
}

fn write_dump_account(pubkey: &Pubkey, account: &Account, path: &Path) -> Result<()> {
    let dump = DumpAccount {
        pubkey: pubkey.to_string(),
        account: DumpAccountInner {
            lamports: account.lamports,
            data: (
                base64::engine::general_purpose::STANDARD.encode(&account.data),
                "base64".to_string(),
            ),
            owner: account.owner.to_string(),
            executable: account.executable,
            rent_epoch: account.rent_epoch,
            space: account.data.len(),
        },
    };

    let json = serde_json::to_string_pretty(&dump)
        .with_context(|| format!("Failed to serialize account {pubkey}"))?;
    std::fs::write(path, json)
        .with_context(|| format!("Failed to write account file: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    use solana_account::AccountSharedData;
    use sonar_sim::ResolvedLookup;

    #[test]
    fn dump_accounts_writes_lookup_placeholders_for_missing_accounts() {
        let temp_dir = std::env::temp_dir().join(format!(
            "sonar_dump_lookup_placeholders_{}_{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let lookup_table_key = Pubkey::new_unique();
        let missing_lookup_account = Pubkey::new_unique();

        let mut accounts = HashMap::new();
        accounts.insert(
            lookup_table_key,
            AccountSharedData::from(Account {
                lamports: 1,
                data: vec![1, 2, 3],
                owner: solana_sdk_ids::system_program::id(),
                executable: false,
                rent_epoch: 0,
            }),
        );

        let resolved = ResolvedAccounts {
            accounts,
            lookups: vec![ResolvedLookup {
                account_key: lookup_table_key,
                writable_indexes: vec![0],
                readonly_indexes: vec![],
                writable_addresses: vec![missing_lookup_account],
                readonly_addresses: vec![],
            }],
        };

        dump_accounts_to_dir(&resolved, &[lookup_table_key], &temp_dir)
            .expect("dump should succeed");

        let placeholder_path = temp_dir.join(format!("{missing_lookup_account}.json"));
        assert!(placeholder_path.exists(), "missing lookup placeholder should be written");

        let parsed =
            parse_account_json(&PathBuf::from(&placeholder_path)).expect("valid placeholder json");
        assert_eq!(parsed.lamports, 0);
        assert!(parsed.data.is_empty());
        assert_eq!(parsed.owner, solana_sdk_ids::system_program::id());
        assert!(!parsed.executable);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn dump_accounts_skips_placeholder_for_litesvm_builtin_program() {
        let temp_dir = std::env::temp_dir().join(format!(
            "sonar_dump_builtin_placeholder_{}_{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let builtin_program = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
            .expect("hard-coded pubkey should parse");
        let resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        dump_accounts_to_dir(&resolved, &[builtin_program], &temp_dir)
            .expect("dump should succeed");

        let placeholder_path = temp_dir.join(format!("{builtin_program}.json"));
        assert!(
            !placeholder_path.exists(),
            "builtin program should not generate placeholder cache file"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
