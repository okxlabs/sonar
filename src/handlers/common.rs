//! Shared handler utilities: account loading, IDL pipeline, cache helpers,
//! and unmatched-address validation used by `decode`, `simulate`, and others.

use std::path::PathBuf;

use anyhow::{Context, Result};
use solana_account::ReadableAccount;
use solana_pubkey::Pubkey;
use sonar_sim::internals::{AccountLoader, ResolvedAccounts};

use crate::cli;
use crate::core::cache::CacheLocation;
use crate::core::{account_loader, idl_fetcher, transaction};
use crate::parsers::instruction::ParserRegistry;
use crate::utils::progress::Progress;

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

pub(crate) struct ResolvedInputTransactions {
    pub resolved_txs: Vec<transaction::ResolvedTxInput>,
}

pub(crate) struct PreparedPipelineContext {
    pub account_loader: AccountLoader,
    pub resolved_accounts: ResolvedAccounts,
}

/// Result of resolving transaction inputs and deriving a cache key.
/// This is the first half of the shared setup pipeline, before any
/// simulate-specific mutations.
pub(crate) struct ResolvedWithCacheKey {
    pub resolved_txs: Vec<transaction::ResolvedTxInput>,
    pub cache_key: String,
}

/// Common cache and prepare arguments shared by simulate and decode handlers.
/// Simulate computes `cache_enabled` as `(cache || cache_dir.is_some() || refresh_cache)` (opt-in);
/// decode computes it as `(!no_cache)` (opt-out).
pub(crate) struct CachePrepareArgs<'a> {
    pub rpc_url: &'a str,
    pub cache_enabled: bool,
    pub cache_dir: &'a Option<PathBuf>,
    pub refresh_cache: bool,
    pub no_idl_fetch: bool,
}

/// Result of resolving cache state and preparing accounts/IDLs.
/// This is the second half of the shared setup pipeline.
pub(crate) struct CachePreparedContext {
    pub cache_dir: Option<PathBuf>,
    pub offline: bool,
    pub prepared: PreparedPipelineContext,
}

// ---------------------------------------------------------------------------
// Cache helpers
// ---------------------------------------------------------------------------

/// Returns `None` when `refresh_cache` is true (forcing re-fetch), otherwise
/// passes through the directory for reading cached accounts.
pub(crate) fn cache_read_dir(cache_dir: Option<PathBuf>, refresh_cache: bool) -> Option<PathBuf> {
    if refresh_cache { None } else { cache_dir }
}

/// Build a `CacheLocation` from CLI args.
pub(crate) fn build_cache_location(cache_dir: &Option<PathBuf>) -> CacheLocation {
    let resolved = crate::core::cache::resolve_cache_dir(cache_dir);
    if cache_dir.is_some() {
        CacheLocation::Explicit(resolved)
    } else {
        CacheLocation::Auto(resolved)
    }
}

// ---------------------------------------------------------------------------
// Transaction input resolution
// ---------------------------------------------------------------------------

/// Parses transaction inputs into a normalized transaction list.
///
/// - bundle mode: parse all positional inputs as transactions/signatures
/// - single mode: parse first positional input, fallback to stdin when missing
pub(crate) fn resolve_inputs_to_txs(
    tx_inputs: Vec<String>,
    rpc_url: &str,
    cache_location: Option<CacheLocation>,
    progress: &Progress,
    bundle_mode: bool,
) -> Result<ResolvedInputTransactions> {
    let resolver = transaction::TxInputResolver::new(rpc_url, cache_location);
    if bundle_mode {
        let resolved_txs = resolver.resolve_many(&tx_inputs, Some(progress))?;
        return Ok(ResolvedInputTransactions { resolved_txs });
    }

    let tx_single = tx_inputs.into_iter().next();
    let raw_input = transaction::read_raw_transaction(tx_single)?;
    let resolved_txs = resolver.resolve_many(&[raw_input], Some(progress))?;
    Ok(ResolvedInputTransactions { resolved_txs })
}

// ---------------------------------------------------------------------------
// Shared setup pipeline
// ---------------------------------------------------------------------------

/// Resolves transaction inputs and derives a cache key. This is the first phase
/// of the shared handler setup — call this before any simulate-specific mutations,
/// then call [`resolve_cache_and_prepare`] after mutations are applied.
pub(crate) fn resolve_and_derive_cache_key(
    tx_inputs: Vec<String>,
    rpc_url: &str,
    resolver_cache_location: Option<CacheLocation>,
    progress: &Progress,
) -> Result<ResolvedWithCacheKey> {
    let is_bundle = tx_inputs.len() > 1;
    let parsed_inputs =
        resolve_inputs_to_txs(tx_inputs, rpc_url, resolver_cache_location, progress, is_bundle)?;
    let resolved_txs = parsed_inputs.resolved_txs;

    let cache_key = if resolved_txs.len() == 1 {
        crate::core::cache::derive_cache_key_single(
            &resolved_txs[0].original_input,
            &resolved_txs[0].parsed_tx.transaction,
        )
    } else {
        let inputs: Vec<_> = resolved_txs.iter().map(|tx| tx.original_input.clone()).collect();
        let parsed_txs: Vec<_> = resolved_txs.iter().map(|tx| tx.parsed_tx.clone()).collect();
        crate::core::cache::derive_cache_key_bundle(&inputs, &parsed_txs)
    };

    Ok(ResolvedWithCacheKey { resolved_txs, cache_key })
}

/// Resolves cache state and prepares accounts/IDLs. This is the second phase of
/// the shared handler setup — called after any simulate-specific mutations have
/// been applied to the parsed transactions.
pub(crate) fn resolve_cache_and_prepare(
    args: &CachePrepareArgs,
    cache_key: &str,
    parsed_txs: &[transaction::ParsedTransaction],
    parser_registry: &mut ParserRegistry,
    progress: &Progress,
) -> Result<CachePreparedContext> {
    let (resolved_cache_dir, offline) = crate::core::cache::resolve_cache_state(
        args.cache_enabled,
        args.cache_dir,
        args.refresh_cache,
        cache_key,
    );
    let cache_read_dir_for_load = cache_read_dir(resolved_cache_dir.clone(), args.refresh_cache);

    let prepared = prepare_accounts_and_idls(
        args.rpc_url,
        cache_read_dir_for_load,
        offline,
        parsed_txs,
        parser_registry,
        args.no_idl_fetch,
        progress,
    )?;

    Ok(CachePreparedContext { cache_dir: resolved_cache_dir, offline, prepared })
}

// ---------------------------------------------------------------------------
// IDL pipeline
// ---------------------------------------------------------------------------

/// Collects executable program IDs from resolved accounts for IDL loading.
pub(crate) fn collect_program_ids(resolved_accounts: &ResolvedAccounts) -> Vec<Pubkey> {
    let mut program_ids: Vec<_> = resolved_accounts
        .accounts
        .iter()
        .filter(|(_, account)| account.executable())
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
    resolved_accounts: &ResolvedAccounts,
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

/// Runs the shared IDL stage for a resolved account set.
pub(crate) fn run_idl_pipeline(
    account_loader: &AccountLoader,
    parser_registry: &mut ParserRegistry,
    resolved_accounts: &ResolvedAccounts,
    no_idl_fetch: bool,
    offline: bool,
    progress: Option<&Progress>,
) {
    let program_ids = collect_program_ids(resolved_accounts);
    if program_ids.is_empty() {
        log::error!("No executable accounts found after RPC load; skipping IDL parsing");
        return;
    }

    if !no_idl_fetch && !offline {
        let idl_fetcher = account_loader::create_idl_fetcher(account_loader, progress.cloned());
        match auto_fetch_missing_idls(
            &idl_fetcher,
            parser_registry,
            &program_ids,
            resolved_accounts,
            progress,
        ) {
            Ok(count) if count > 0 => log::info!("Auto-fetched {} missing IDLs", count),
            Ok(_) => {}
            Err(err) => log::warn!("Failed to auto-fetch missing IDLs: {:#}", err),
        }
    }

    match parser_registry.load_idl_parsers_for_programs(program_ids) {
        Ok(count) if count > 0 => log::info!("Lazy-loaded {} IDL parsers", count),
        Ok(_) => {}
        Err(err) => log::warn!("Failed to load IDL parsers: {:?}", err),
    }
}

/// Loads accounts for parsed transactions and runs the shared IDL pipeline.
pub(crate) fn prepare_accounts_and_idls(
    rpc_url: &str,
    cache_dir: Option<PathBuf>,
    offline: bool,
    parsed_txs: &[transaction::ParsedTransaction],
    parser_registry: &mut ParserRegistry,
    no_idl_fetch: bool,
    progress: &Progress,
) -> Result<PreparedPipelineContext> {
    let mut account_loader = account_loader::create_loader(
        rpc_url.to_string(),
        cache_dir,
        offline,
        Some(progress.clone()),
    )?;
    let resolved_accounts = if parsed_txs.len() == 1 {
        account_loader.load_for_transaction(&parsed_txs[0].transaction)?
    } else {
        let tx_refs: Vec<_> = parsed_txs.iter().map(|parsed| &parsed.transaction).collect();
        account_loader.load_for_transactions(&tx_refs)?
    };

    run_idl_pipeline(
        &account_loader,
        parser_registry,
        &resolved_accounts,
        no_idl_fetch,
        offline,
        Some(progress),
    );

    Ok(PreparedPipelineContext { account_loader, resolved_accounts })
}

// ---------------------------------------------------------------------------
// Unmatched-address validation
// ---------------------------------------------------------------------------

/// Builds a set of all account keys referenced by the parsed transactions and their
/// resolved address lookup tables.
fn collect_transaction_account_keys(
    parsed_txs: &[&transaction::ParsedTransaction],
    resolved_accounts: &ResolvedAccounts,
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

/// Finds --override pubkeys that are not present in the given transaction account key set.
fn find_unmatched_overrides(
    overrides: &[cli::AccountOverride],
    tx_keys: &std::collections::HashSet<Pubkey>,
) -> Vec<Pubkey> {
    overrides.iter().filter(|r| !tx_keys.contains(&r.pubkey())).map(|r| r.pubkey()).collect()
}

/// Finds --fund-sol pubkeys that are not present in the given transaction account key set.
fn find_unmatched_sol_fundings(
    fundings: &[cli::SolFunding],
    tx_keys: &std::collections::HashSet<Pubkey>,
) -> Vec<Pubkey> {
    fundings.iter().filter(|f| !tx_keys.contains(&f.pubkey)).map(|f| f.pubkey).collect()
}

/// Finds --close-account pubkeys that are not present in the given transaction account key set.
fn find_unmatched_closures(
    closures: &[Pubkey],
    tx_keys: &std::collections::HashSet<Pubkey>,
) -> Vec<Pubkey> {
    closures.iter().filter(|pk| !tx_keys.contains(pk)).copied().collect()
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

/// Warns the user when --override, --fund-sol, --fund-token, or --close-account addresses
/// are not found in the transaction's account keys, which likely indicates a typo.
pub(crate) fn warn_unmatched_addresses(
    overrides: &[cli::AccountOverride],
    fundings: &[cli::SolFunding],
    token_fundings: &[cli::TokenFunding],
    account_closures: &[Pubkey],
    parsed_txs: &[&transaction::ParsedTransaction],
    resolved_accounts: &ResolvedAccounts,
) {
    if overrides.is_empty()
        && fundings.is_empty()
        && token_fundings.is_empty()
        && account_closures.is_empty()
    {
        return;
    }

    let tx_keys = collect_transaction_account_keys(parsed_txs, resolved_accounts);

    for pubkey in find_unmatched_overrides(overrides, &tx_keys) {
        log::warn!(
            "--override target {} is not referenced in the transaction's account keys. Did you mean a different address?",
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

    for pubkey in find_unmatched_closures(account_closures, &tx_keys) {
        log::warn!(
            "--close-account target {} is not referenced in the transaction's account keys. Did you mean a different address?",
            pubkey,
        );
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{
        collect_program_ids, find_unmatched_closures, find_unmatched_overrides,
        find_unmatched_sol_fundings, find_unmatched_token_fundings,
    };
    use crate::cli;
    use solana_account::{Account, AccountSharedData};
    use solana_pubkey::Pubkey;
    use solana_sdk_ids::system_program;
    use sonar_sim::internals::ResolvedAccounts;
    use std::collections::{HashMap, HashSet};

    fn executable_account() -> AccountSharedData {
        AccountSharedData::from(Account {
            lamports: 0,
            data: Vec::new(),
            owner: system_program::id(),
            executable: true,
            rent_epoch: 0,
        })
    }

    fn non_executable_account() -> AccountSharedData {
        AccountSharedData::from(Account {
            lamports: 0,
            data: Vec::new(),
            owner: system_program::id(),
            executable: false,
            rent_epoch: 0,
        })
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

        let resolved = ResolvedAccounts { accounts, lookups: vec![] };

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

        let resolved = ResolvedAccounts { accounts, lookups: vec![] };

        let program_ids = collect_program_ids(&resolved);

        assert!(program_ids.is_empty());
    }

    #[test]
    fn find_unmatched_sol_fundings_returns_empty_when_all_match() {
        let key_a = Pubkey::new_unique();
        let key_b = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [key_a, key_b].into_iter().collect();

        let fundings = vec![
            cli::SolFunding { pubkey: key_a, amount_lamports: 1_000_000_000 },
            cli::SolFunding { pubkey: key_b, amount_lamports: 2_000_000_000 },
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
            cli::SolFunding { pubkey: key_in_tx, amount_lamports: 1_000_000_000 },
            cli::SolFunding { pubkey: key_not_in_tx, amount_lamports: 2_000_000_000 },
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
                owner: None,
                amount: cli::TokenAmount::Raw(100),
            },
            cli::TokenFunding {
                account: account_not_in_tx,
                mint: Some(mint_not_in_tx),
                owner: None,
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
            owner: None,
            amount: cli::TokenAmount::Raw(100),
        }];

        let unmatched = find_unmatched_token_fundings(&token_fundings, &tx_keys);
        assert!(unmatched.is_empty());
    }

    #[test]
    fn find_unmatched_overrides_detects_missing_program_id() {
        let prog_in_tx = Pubkey::new_unique();
        let prog_not_in_tx = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [prog_in_tx].into_iter().collect();

        let overrides = vec![
            cli::AccountOverride::Program {
                program_id: prog_in_tx,
                so_path: std::path::PathBuf::from("/tmp/a.so"),
            },
            cli::AccountOverride::Program {
                program_id: prog_not_in_tx,
                so_path: std::path::PathBuf::from("/tmp/b.so"),
            },
        ];

        let unmatched = find_unmatched_overrides(&overrides, &tx_keys);
        assert_eq!(unmatched.len(), 1);
        assert_eq!(unmatched[0], prog_not_in_tx);
    }

    #[test]
    fn find_unmatched_overrides_returns_empty_when_all_match() {
        let prog_a = Pubkey::new_unique();
        let prog_b = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [prog_a, prog_b].into_iter().collect();

        let overrides = vec![
            cli::AccountOverride::Program {
                program_id: prog_a,
                so_path: std::path::PathBuf::from("/tmp/a.so"),
            },
            cli::AccountOverride::Program {
                program_id: prog_b,
                so_path: std::path::PathBuf::from("/tmp/b.so"),
            },
        ];

        let unmatched = find_unmatched_overrides(&overrides, &tx_keys);
        assert!(unmatched.is_empty());
    }

    #[test]
    fn find_unmatched_closures_detects_missing_address() {
        let key_in_tx = Pubkey::new_unique();
        let key_not_in_tx = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [key_in_tx].into_iter().collect();

        let closures = vec![key_in_tx, key_not_in_tx];
        let unmatched = find_unmatched_closures(&closures, &tx_keys);
        assert_eq!(unmatched.len(), 1);
        assert_eq!(unmatched[0], key_not_in_tx);
    }

    #[test]
    fn find_unmatched_closures_returns_empty_when_all_match() {
        let key_a = Pubkey::new_unique();
        let key_b = Pubkey::new_unique();
        let tx_keys: HashSet<Pubkey> = [key_a, key_b].into_iter().collect();

        let closures = vec![key_a, key_b];
        let unmatched = find_unmatched_closures(&closures, &tx_keys);
        assert!(unmatched.is_empty());
    }
}
