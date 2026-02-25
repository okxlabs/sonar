pub(crate) mod account;
pub(crate) mod cache;
pub(crate) mod completions;
pub(crate) mod config;
pub(crate) mod convert;
pub(crate) mod decode;
pub(crate) mod idl;
pub(crate) mod pda;
pub(crate) mod program_elf;
pub(crate) mod send;
pub(crate) mod simulate;

use crate::parsers::instruction::ParserRegistry;
use crate::utils::progress::Progress;
use crate::{cli, core::account_loader, core::idl_fetcher, core::transaction};
use anyhow::{Context, Result};
use solana_pubkey::Pubkey;

/// Collects executable program IDs from resolved accounts for IDL loading.
pub(crate) fn collect_program_ids(
    resolved_accounts: &account_loader::ResolvedAccounts,
) -> Vec<Pubkey> {
    let mut program_ids: Vec<_> = resolved_accounts
        .accounts
        .iter()
        .filter(|(_, account)| account.executable)
        .map(|(pubkey, _)| *pubkey)
        .collect();

    program_ids.sort();
    program_ids.dedup();

    if program_ids.is_empty() {
        log::error!("No executable accounts found; IDL parsers will not be loaded");
    }

    program_ids
}

/// Auto-fetch missing IDLs for fetchable upgradeable programs and persist them to local cache.
pub(crate) fn auto_fetch_missing_idls(
    idl_fetcher: &idl_fetcher::IdlFetcher,
    parser_registry: &ParserRegistry,
    program_ids: &[Pubkey],
    resolved_accounts: &account_loader::ResolvedAccounts,
    progress: Option<&Progress>,
) -> Result<usize> {
    let missing = parser_registry.find_fetchable_programs(program_ids, resolved_accounts);
    if missing.is_empty() {
        return Ok(0);
    }

    let Some(idl_dir) = parser_registry.idl_directory() else {
        return Ok(0);
    };

    std::fs::create_dir_all(idl_dir)
        .with_context(|| format!("Failed to create IDL cache directory: {}", idl_dir.display()))?;

    if let Some(progress) = progress {
        progress.set_message(format!("Fetching missing IDLs... ({})", missing.len()));
    }

    let mut fetched = 0usize;
    for (program_id, result) in idl_fetcher.fetch_idls(&missing) {
        match result {
            Ok(Some(idl_json)) => {
                let formatted = match serde_json::from_str::<serde_json::Value>(&idl_json) {
                    Ok(value) => {
                        serde_json::to_string_pretty(&value).unwrap_or_else(|_| idl_json.clone())
                    }
                    Err(_) => idl_json,
                };
                let idl_path = idl_dir.join(format!("{}.json", program_id));
                std::fs::write(&idl_path, formatted).with_context(|| {
                    format!("Failed to write auto-fetched IDL: {}", idl_path.display())
                })?;
                fetched += 1;
            }
            Ok(None) => {
                log::debug!("No on-chain IDL found for program {}", program_id);
            }
            Err(err) => {
                log::warn!("Failed to auto-fetch IDL for {}: {:#}", program_id, err);
            }
        }
    }

    Ok(fetched)
}

/// Builds a set of all account keys referenced by the parsed transactions and their
/// resolved address lookup tables.
fn collect_transaction_account_keys(
    parsed_txs: &[&transaction::ParsedTransaction],
    resolved_accounts: &account_loader::ResolvedAccounts,
) -> std::collections::HashSet<Pubkey> {
    use std::collections::HashSet;

    let mut tx_keys: HashSet<Pubkey> = HashSet::new();
    for parsed_tx in parsed_txs {
        tx_keys.extend(parsed_tx.account_plan.static_accounts.iter());
    }
    for lookup in &resolved_accounts.lookups {
        tx_keys.extend(lookup.writable_addresses.iter());
        tx_keys.extend(lookup.readonly_addresses.iter());
    }
    tx_keys
}

/// Finds --replace pubkeys that are not present in the given transaction account key set.
fn find_unmatched_replacements(
    replacements: &[cli::Replacement],
    tx_keys: &std::collections::HashSet<Pubkey>,
) -> Vec<Pubkey> {
    replacements.iter().filter(|r| !tx_keys.contains(&r.pubkey())).map(|r| r.pubkey()).collect()
}

/// Finds --fund-sol pubkeys that are not present in the given transaction account key set.
fn find_unmatched_sol_fundings(
    fundings: &[cli::Funding],
    tx_keys: &std::collections::HashSet<Pubkey>,
) -> Vec<Pubkey> {
    fundings.iter().filter(|f| !tx_keys.contains(&f.pubkey)).map(|f| f.pubkey).collect()
}

/// Finds --fund-token pubkeys (account and mint) that are not present in the given
/// transaction account key set.
fn find_unmatched_token_fundings(
    token_fundings: &[cli::TokenFunding],
    tx_keys: &std::collections::HashSet<Pubkey>,
) -> Vec<Pubkey> {
    let mut unmatched = Vec::new();
    for tf in token_fundings {
        if !tx_keys.contains(&tf.account) {
            unmatched.push(tf.account);
        }
        if let Some(mint) = tf.mint {
            if !tx_keys.contains(&mint) {
                unmatched.push(mint);
            }
        }
    }
    unmatched
}

/// Warns the user when --replace, --fund-sol, or --fund-token addresses are not found
/// in the transaction's account keys, which likely indicates a typo.
pub(crate) fn warn_unmatched_addresses(
    replacements: &[cli::Replacement],
    fundings: &[cli::Funding],
    token_fundings: &[cli::TokenFunding],
    parsed_txs: &[&transaction::ParsedTransaction],
    resolved_accounts: &account_loader::ResolvedAccounts,
) {
    if replacements.is_empty() && fundings.is_empty() && token_fundings.is_empty() {
        return;
    }

    let tx_keys = collect_transaction_account_keys(parsed_txs, resolved_accounts);

    for pubkey in find_unmatched_replacements(replacements, &tx_keys) {
        log::warn!(
            "--replace target {} is not referenced in the transaction's account keys. Did you mean a different address?",
            pubkey,
        );
    }

    for pubkey in find_unmatched_sol_fundings(fundings, &tx_keys) {
        log::warn!(
            "--fund-sol address {} is not referenced in the transaction's account keys. Did you mean a different address?",
            pubkey,
        );
    }

    for pubkey in find_unmatched_token_fundings(token_fundings, &tx_keys) {
        log::warn!(
            "--fund-token address {} is not referenced in the transaction's account keys. Did you mean a different address?",
            pubkey,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        collect_program_ids, find_unmatched_replacements, find_unmatched_sol_fundings,
        find_unmatched_token_fundings,
    };
    use crate::cli;
    use crate::core::account_loader;
    use solana_account::Account;
    use solana_pubkey::Pubkey;
    use solana_sdk_ids::system_program;
    use std::collections::{HashMap, HashSet};

    fn executable_account() -> Account {
        Account {
            lamports: 0,
            data: Vec::new(),
            owner: system_program::id(),
            executable: true,
            rent_epoch: 0,
        }
    }

    fn non_executable_account() -> Account {
        Account {
            lamports: 0,
            data: Vec::new(),
            owner: system_program::id(),
            executable: false,
            rent_epoch: 0,
        }
    }

    #[test]
    fn collect_program_ids_only_includes_executable_accounts() {
        let exec_a = Pubkey::new_unique();
        let exec_b = Pubkey::new_unique();
        let non_exec = Pubkey::new_unique();
        let mut accounts = HashMap::new();
        accounts.insert(exec_a, executable_account());
        accounts.insert(exec_b, executable_account());
        accounts.insert(non_exec, non_executable_account());

        let resolved = account_loader::ResolvedAccounts { accounts, lookups: vec![] };

        let program_ids = collect_program_ids(&resolved);

        assert_eq!(program_ids.len(), 2);
        assert!(program_ids.contains(&exec_a));
        assert!(program_ids.contains(&exec_b));
        assert!(!program_ids.contains(&non_exec));
    }

    #[test]
    fn collect_program_ids_returns_empty_when_no_executable_accounts() {
        let mut accounts = HashMap::new();
        accounts.insert(Pubkey::new_unique(), non_executable_account());
        accounts.insert(Pubkey::new_unique(), non_executable_account());

        let resolved = account_loader::ResolvedAccounts { accounts, lookups: vec![] };

        let program_ids = collect_program_ids(&resolved);

        assert!(program_ids.is_empty());
    }

    #[test]
    fn find_unmatched_sol_fundings_returns_empty_when_all_match() {
        let key_a = Pubkey::new_unique();
        let key_b = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [key_a, key_b].into_iter().collect();

        let fundings = vec![
            cli::Funding { pubkey: key_a, amount_lamports: 1_000_000_000 },
            cli::Funding { pubkey: key_b, amount_lamports: 2_000_000_000 },
        ];

        let unmatched = find_unmatched_sol_fundings(&fundings, &tx_keys);
        assert!(unmatched.is_empty());
    }

    #[test]
    fn find_unmatched_sol_fundings_detects_missing_address() {
        let key_in_tx = Pubkey::new_unique();
        let key_not_in_tx = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [key_in_tx].into_iter().collect();

        let fundings = vec![
            cli::Funding { pubkey: key_in_tx, amount_lamports: 1_000_000_000 },
            cli::Funding { pubkey: key_not_in_tx, amount_lamports: 2_000_000_000 },
        ];

        let unmatched = find_unmatched_sol_fundings(&fundings, &tx_keys);
        assert_eq!(unmatched.len(), 1);
        assert_eq!(unmatched[0], key_not_in_tx);
    }

    #[test]
    fn find_unmatched_sol_fundings_returns_empty_for_no_fundings() {
        let tx_keys: HashSet<Pubkey> = [Pubkey::new_unique()].into_iter().collect();
        let unmatched = find_unmatched_sol_fundings(&[], &tx_keys);
        assert!(unmatched.is_empty());
    }

    #[test]
    fn find_unmatched_token_fundings_detects_missing_account_and_mint() {
        let account_in_tx = Pubkey::new_unique();
        let mint_in_tx = Pubkey::new_unique();
        let account_not_in_tx = Pubkey::new_unique();
        let mint_not_in_tx = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [account_in_tx, mint_in_tx].into_iter().collect();

        let token_fundings = vec![
            cli::TokenFunding {
                account: account_in_tx,
                mint: Some(mint_in_tx),
                amount: cli::TokenAmount::Raw(100),
            },
            cli::TokenFunding {
                account: account_not_in_tx,
                mint: Some(mint_not_in_tx),
                amount: cli::TokenAmount::Raw(200),
            },
        ];

        let unmatched = find_unmatched_token_fundings(&token_fundings, &tx_keys);
        assert_eq!(unmatched.len(), 2);
        assert!(unmatched.contains(&account_not_in_tx));
        assert!(unmatched.contains(&mint_not_in_tx));
    }

    #[test]
    fn find_unmatched_token_fundings_returns_empty_when_all_match() {
        let account = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [account, mint].into_iter().collect();

        let token_fundings = vec![cli::TokenFunding {
            account,
            mint: Some(mint),
            amount: cli::TokenAmount::Raw(100),
        }];

        let unmatched = find_unmatched_token_fundings(&token_fundings, &tx_keys);
        assert!(unmatched.is_empty());
    }

    #[test]
    fn find_unmatched_replacements_detects_missing_program_id() {
        let prog_in_tx = Pubkey::new_unique();
        let prog_not_in_tx = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [prog_in_tx].into_iter().collect();

        let replacements = vec![
            cli::Replacement::Program {
                program_id: prog_in_tx,
                so_path: std::path::PathBuf::from("/tmp/a.so"),
            },
            cli::Replacement::Program {
                program_id: prog_not_in_tx,
                so_path: std::path::PathBuf::from("/tmp/b.so"),
            },
        ];

        let unmatched = find_unmatched_replacements(&replacements, &tx_keys);
        assert_eq!(unmatched.len(), 1);
        assert_eq!(unmatched[0], prog_not_in_tx);
    }

    #[test]
    fn find_unmatched_replacements_returns_empty_when_all_match() {
        let prog_a = Pubkey::new_unique();
        let prog_b = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [prog_a, prog_b].into_iter().collect();

        let replacements = vec![
            cli::Replacement::Program {
                program_id: prog_a,
                so_path: std::path::PathBuf::from("/tmp/a.so"),
            },
            cli::Replacement::Program {
                program_id: prog_b,
                so_path: std::path::PathBuf::from("/tmp/b.so"),
            },
        ];

        let unmatched = find_unmatched_replacements(&replacements, &tx_keys);
        assert!(unmatched.is_empty());
    }
}
