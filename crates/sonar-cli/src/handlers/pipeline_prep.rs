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

/// Where the transactions to set up come from. Folds the two input modes
/// (raw transactions/signatures vs. CLI-synthesized instructions) behind a
/// single setup entry point.
pub(crate) enum TxSource {
    /// Raw positional inputs: base64/base58 transactions or signatures to fetch.
    /// A single empty input defers to stdin; more than one input is a bundle.
    Raw(Vec<String>),
    /// Instruction inputs synthesized into one transaction with the given payer.
    Instructions { payer: Pubkey, inputs: Vec<transaction::InstructionInput> },
}

/// Transaction provenance retained for cache-metadata writing after the heavy
/// parsed form has been mutated.
pub(crate) struct TxOrigin {
    pub original_input: String,
    pub raw_tx_base64: String,
    pub resolved_from: String,
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
    pub rpc_batch_size: usize,
}

/// Everything a command needs after the shared setup pipeline has run: the
/// post-mutation parsed transactions, their provenance, the loaded accounts,
/// and the resolved cache state.
pub(crate) struct PreparedPipeline {
    pub origins: Vec<TxOrigin>,
    pub parsed_txs: Vec<transaction::ParsedTransaction>,
    pub account_loader: AccountLoader,
    pub resolved_accounts: ResolvedAccounts,
    pub cache_dir: Option<PathBuf>,
    pub offline: bool,
}

/// Account loader plus the accounts it resolved — internal result of
/// [`prepare_accounts_and_idls`].
struct PreparedAccounts {
    account_loader: AccountLoader,
    resolved_accounts: ResolvedAccounts,
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
// Shared setup pipeline
// ---------------------------------------------------------------------------

/// Run the shared command setup as a single operation: resolve transaction
/// inputs, derive the cache key, apply a caller-supplied mutation to each parsed
/// transaction, then resolve cache state and load accounts/IDLs.
///
/// The mutation hook is the one stage that genuinely differs between commands:
/// `simulate` applies instruction account/data patches — which change the set of
/// accounts to load, so they must run before account resolution — while `decode`
/// passes a no-op. Threading it as a closure keeps the resolve → mutate → load
/// ordering an internal detail rather than a contract split across call sites.
pub(crate) fn resolve_mutate_prepare(
    source: TxSource,
    resolver_cache_location: Option<CacheLocation>,
    cache_args: &CachePrepareArgs,
    parser_registry: &mut ParserRegistry,
    progress: &Progress,
    mut mutate: impl FnMut(&mut transaction::ParsedTransaction) -> Result<()>,
) -> Result<PreparedPipeline> {
    // 1. Resolve inputs into transactions and derive the cache key.
    let (resolved_txs, cache_key) =
        resolve_inputs(source, resolver_cache_location, cache_args, progress)?;

    // 2. Separate provenance from the parsed form, then apply the caller's
    //    mutation. The account set to load depends on the post-mutation
    //    transactions, so this must happen before preparing accounts.
    let mut origins = Vec::with_capacity(resolved_txs.len());
    let mut parsed_txs = Vec::with_capacity(resolved_txs.len());
    for resolved in resolved_txs {
        origins.push(TxOrigin {
            original_input: resolved.original_input,
            raw_tx_base64: resolved.raw_tx_base64,
            resolved_from: resolved.source.as_str().to_string(),
        });
        parsed_txs.push(resolved.parsed_tx);
    }
    for parsed_tx in &mut parsed_txs {
        mutate(parsed_tx)?;
    }

    // 3. Resolve cache state and load accounts/IDLs for the final transactions.
    let (cache_dir, offline) = crate::core::cache::resolve_cache_state(
        cache_args.cache_enabled,
        cache_args.cache_dir,
        cache_args.refresh_cache,
        &cache_key,
    );
    let cache_read_dir_for_load = cache_read_dir(cache_dir.clone(), cache_args.refresh_cache);
    let prepared = prepare_accounts_and_idls(
        cache_args.rpc_url,
        cache_read_dir_for_load,
        offline,
        &parsed_txs,
        parser_registry,
        cache_args.no_idl_fetch,
        progress,
        cache_args.rpc_batch_size,
    )?;

    Ok(PreparedPipeline {
        origins,
        parsed_txs,
        account_loader: prepared.account_loader,
        resolved_accounts: prepared.resolved_accounts,
        cache_dir,
        offline,
    })
}

/// Resolve a [`TxSource`] into concrete transactions and the cache key derived
/// from them. Raw inputs are fetched/parsed (single input falls back to stdin);
/// instruction inputs are synthesized into one transaction at the CLI.
fn resolve_inputs(
    source: TxSource,
    resolver_cache_location: Option<CacheLocation>,
    cache_args: &CachePrepareArgs,
    progress: &Progress,
) -> Result<(Vec<transaction::ResolvedTxInput>, String)> {
    match source {
        TxSource::Raw(tx_inputs) => {
            let resolver =
                transaction::TxInputResolver::new(cache_args.rpc_url, resolver_cache_location);
            let resolved_txs = if tx_inputs.len() > 1 {
                resolver.resolve_many(&tx_inputs, Some(progress))?
            } else {
                let raw_input = transaction::read_raw_transaction(tx_inputs.into_iter().next())?;
                resolver.resolve_many(&[raw_input], Some(progress))?
            };
            let cache_key = derive_cache_key(&resolved_txs);
            Ok((resolved_txs, cache_key))
        }
        TxSource::Instructions { payer, inputs } => {
            let source = transaction::TxResolveSource::Instructions;
            let parsed_tx = transaction::build_transaction_from_instructions(payer, &inputs)
                .context("Failed to build transaction from instruction inputs")?;
            let raw_tx_base64 = transaction::encode_transaction_to_base64(&parsed_tx.transaction)?;
            let cache_key = crate::core::cache::derive_cache_key_single(
                source.as_str(),
                &parsed_tx.transaction,
            );
            let resolved_txs = vec![transaction::ResolvedTxInput {
                original_input: source.as_str().to_string(),
                raw_tx_base64,
                parsed_tx,
                source,
            }];
            Ok((resolved_txs, cache_key))
        }
    }
}

/// Derive a cache key from resolved transactions: single-key for one input,
/// bundle-key otherwise.
fn derive_cache_key(resolved_txs: &[transaction::ResolvedTxInput]) -> String {
    if resolved_txs.len() == 1 {
        crate::core::cache::derive_cache_key_single(
            &resolved_txs[0].original_input,
            &resolved_txs[0].parsed_tx.transaction,
        )
    } else {
        let inputs: Vec<_> = resolved_txs.iter().map(|tx| tx.original_input.clone()).collect();
        let parsed_txs: Vec<_> = resolved_txs.iter().map(|tx| tx.parsed_tx.clone()).collect();
        crate::core::cache::derive_cache_key_bundle(&inputs, &parsed_txs)
    }
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
        log::debug!("No executable program accounts found; skipping optional IDL parser loading");
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
#[allow(clippy::too_many_arguments)]
fn prepare_accounts_and_idls(
    rpc_url: &str,
    cache_dir: Option<PathBuf>,
    offline: bool,
    parsed_txs: &[transaction::ParsedTransaction],
    parser_registry: &mut ParserRegistry,
    no_idl_fetch: bool,
    progress: &Progress,
    rpc_batch_size: usize,
) -> Result<PreparedAccounts> {
    let mut account_loader = account_loader::create_loader(
        rpc_url.to_string(),
        cache_dir,
        offline,
        Some(progress.clone()),
        rpc_batch_size,
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

    Ok(PreparedAccounts { account_loader, resolved_accounts })
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

    // Single source of truth for the typo warning; each flag only supplies its
    // label and the addresses it referenced that the transaction never mentions.
    let groups = [
        ("--override target", find_unmatched_overrides(overrides, &tx_keys)),
        ("--fund-sol address", find_unmatched_sol_fundings(fundings, &tx_keys)),
        ("--fund-token address", find_unmatched_token_fundings(token_fundings, &tx_keys)),
        ("--close-account target", find_unmatched_closures(account_closures, &tx_keys)),
    ];

    for (label, unmatched) in groups {
        for pubkey in unmatched {
            log::warn!(
                "{label} {pubkey} is not referenced in the transaction's account keys. \
                 Did you mean a different address?",
            );
        }
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
    use log::{Level, LevelFilter, Metadata, Record};
    use solana_account::{Account, AccountSharedData};
    use solana_pubkey::Pubkey;
    use solana_sdk_ids::system_program;
    use sonar_sim::internals::ResolvedAccounts;
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use std::sync::Once;

    thread_local! {
        static CAPTURED: RefCell<Option<Vec<(Level, String)>>> = const { RefCell::new(None) };
    }

    struct TestLogger;

    impl log::Log for TestLogger {
        fn enabled(&self, _metadata: &Metadata) -> bool {
            true
        }

        fn log(&self, record: &Record) {
            CAPTURED.with(|cell| {
                if let Some(buf) = cell.borrow_mut().as_mut() {
                    buf.push((record.level(), record.args().to_string()));
                }
            });
        }

        fn flush(&self) {}
    }

    static TEST_LOGGER: TestLogger = TestLogger;
    static LOGGER_INIT: Once = Once::new();

    fn capture_logs(run: impl FnOnce()) -> Vec<(Level, String)> {
        LOGGER_INIT.call_once(|| {
            log::set_logger(&TEST_LOGGER).expect("test logger should initialize once");
            log::set_max_level(LevelFilter::Trace);
        });

        CAPTURED.with(|cell| *cell.borrow_mut() = Some(Vec::new()));
        run();
        CAPTURED.with(|cell| cell.borrow_mut().take().unwrap_or_default())
    }

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
    fn collect_program_ids_does_not_log_error_when_no_executable_accounts() {
        let mut accounts = HashMap::new();
        accounts.insert(Pubkey::new_unique(), non_executable_account());

        let resolved = ResolvedAccounts { accounts, lookups: vec![] };

        let records = capture_logs(|| {
            let program_ids = collect_program_ids(&resolved);
            assert!(program_ids.is_empty());
        });

        assert!(
            records.iter().all(|(level, message)| *level != Level::Error
                || !message.contains("No executable accounts found")),
            "empty executable-account set should not be logged as an error: {records:?}"
        );
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
