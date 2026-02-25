pub use sonar_sim::{ExecutionStatus, SimulationOptions, SimulationResult, TransactionExecutor};

// ---------------------------------------------------------------------------
// Account dump logic (CLI-only, Solana CLI compatible JSON format)
// ---------------------------------------------------------------------------

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use base64::Engine;
use serde::Serialize;
use solana_account::Account;
use solana_clock::Clock;
use solana_pubkey::Pubkey;
use solana_sdk_ids::system_program;
use solana_slot_hashes::SlotHashes;
use solana_sysvar_id::SysvarId;

use sonar_sim::ResolvedAccounts;

/// JSON structure matching `solana account <PUBKEY> --output json`.
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

/// The NativeLoader program ID (owner of all native programs).
static NATIVE_LOADER_ID: LazyLock<Pubkey> = LazyLock::new(|| {
    "NativeLoader1111111111111111111111111111111".parse().expect("invalid NativeLoader pubkey")
});

/// Returns `true` if the account is owned by the NativeLoader (native program executable marker).
fn is_native_owner(account: &Account) -> bool {
    account.owner == *NATIVE_LOADER_ID
}

/// Dump accounts to a directory in Solana CLI compatible JSON format.
///
/// Writes the original RPC-loaded account data (before --replace / --patch-account-data).
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
        // Keep Clock/SlotHashes sysvars for offline ALT resolution.
        if crate::utils::native_ids::is_native_or_sysvar(pubkey)
            && *pubkey != Clock::id()
            && *pubkey != SlotHashes::id()
        {
            continue;
        }

        if is_native_owner(account) {
            continue;
        }

        let path = dir.join(format!("{pubkey}.json"));
        write_dump_account(pubkey, account, &path)?;

        dumped_keys.insert(*pubkey);
    }

    // For transaction-required accounts not present in the loaded account map
    // (non-existent on-chain), write zero-lamport placeholder JSONs so that
    // --load-accounts can find them and the offline loader doesn't warn about
    // missing files.
    for pubkey in required_keys {
        if dumped_keys.contains(&pubkey) || crate::utils::native_ids::is_native_or_sysvar(&pubkey) {
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
            Account {
                lamports: 1,
                data: vec![1, 2, 3],
                owner: solana_sdk_ids::system_program::id(),
                executable: false,
                rent_epoch: 0,
            },
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
            crate::core::account_file::parse_account_json(&PathBuf::from(&placeholder_path))
                .expect("valid placeholder json");
        assert_eq!(parsed.lamports, 0);
        assert!(parsed.data.is_empty());
        assert_eq!(parsed.owner, solana_sdk_ids::system_program::id());
        assert!(!parsed.executable);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
