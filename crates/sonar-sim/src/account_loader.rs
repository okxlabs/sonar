use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use solana_account::{AccountSharedData, ReadableAccount};
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_clock::Clock;
use solana_pubkey::Pubkey;
use solana_slot_hashes::SlotHashes;
use solana_sysvar_id::SysvarId;
use solana_transaction::versioned::VersionedTransaction;

use crate::account_dependencies::{
    collect_bpf_upgradeable_programdata_dependencies, collect_token_mint_dependencies,
};
use crate::account_fetcher::{AccountFetcher, format_pubkeys};
use crate::error::{Result, SonarSimError};
use crate::rpc_provider::RpcAccountProvider;
use crate::transaction::{AddressLookupPlan, MessageAccountPlan};
use crate::types::{
    AccountAppender, AccountSource, FetchObserver, FetchPolicy, ResolvedAccounts, ResolvedLookup,
};

/// High-level account loading orchestrator.
///
/// `AccountLoader` coordinates the full pipeline:
/// plan → fetch initial → resolve dependencies (loop) → resolve lookups
/// → fetch lookup accounts → resolve dependencies (loop).
///
/// The actual data-access work (dedup, caching, middleware, RPC calls) is
/// delegated to the inner [`AccountFetcher`].
pub struct AccountLoader {
    fetcher: AccountFetcher,
}

impl AccountLoader {
    pub fn new(rpc_url: String) -> Result<Self> {
        Ok(Self { fetcher: AccountFetcher::new(rpc_url)? })
    }

    pub fn with_provider(provider: Arc<dyn RpcAccountProvider>) -> Self {
        Self { fetcher: AccountFetcher::with_provider(provider) }
    }

    /// Attach an account source used before RPC.
    pub fn with_source(mut self, source: Arc<dyn AccountSource>) -> Self {
        self.fetcher = self.fetcher.with_source(source);
        self
    }

    /// Attach a policy that decides whether unresolved accounts may use RPC.
    pub fn with_policy(mut self, policy: Arc<dyn FetchPolicy>) -> Self {
        self.fetcher = self.fetcher.with_policy(policy);
        self
    }

    /// Attach an observer for fetch lifecycle events.
    pub fn with_observer(mut self, observer: Arc<dyn FetchObserver>) -> Self {
        self.fetcher = self.fetcher.with_observer(observer);
        self
    }

    /// Expose the underlying provider so callers can reuse the RPC connection.
    pub fn provider(&self) -> Arc<dyn RpcAccountProvider> {
        self.fetcher.provider()
    }

    /// Expose the inner fetcher for direct low-level account access.
    pub fn fetcher(&self) -> &AccountFetcher {
        &self.fetcher
    }

    pub fn load_for_transaction(&mut self, tx: &VersionedTransaction) -> Result<ResolvedAccounts> {
        self.load_from_plans(&[MessageAccountPlan::from_transaction(tx)])
    }

    /// Load accounts for multiple transactions (bundle simulation).
    /// Merges all required accounts from all transactions and fetches them in a single batch.
    pub fn load_for_transactions(
        &mut self,
        txs: &[&VersionedTransaction],
    ) -> Result<ResolvedAccounts> {
        if txs.is_empty() {
            return Ok(ResolvedAccounts { accounts: HashMap::new(), lookups: Vec::new() });
        }
        let plans: Vec<_> = txs.iter().map(|tx| MessageAccountPlan::from_transaction(tx)).collect();
        self.load_from_plans(&plans)
    }

    fn load_from_plans(&mut self, plans: &[MessageAccountPlan]) -> Result<ResolvedAccounts> {
        let initial = collect_initial_accounts(plans);
        let mut accounts = HashMap::new();

        self.fetcher.fetch_accounts(&initial, &mut accounts).map_err(|e| {
            SonarSimError::AccountData {
                pubkey: None,
                reason: format!(
                    "Failed to fetch initial accounts (static + sysvars + lookups): {e}"
                ),
            }
        })?;
        self.resolve_all_dependencies(&mut accounts)?;

        let (lookups, lookup_pubkeys) = self.resolve_all_lookups(plans, &mut accounts)?;

        if !lookup_pubkeys.is_empty() {
            self.fetcher.fetch_accounts(&lookup_pubkeys, &mut accounts).map_err(|e| {
                SonarSimError::AccountData {
                    pubkey: None,
                    reason: format!(
                        "Failed to load accounts from address lookup tables: [{}]: {e}",
                        format_pubkeys(&lookup_pubkeys)
                    ),
                }
            })?;
            self.resolve_all_dependencies(&mut accounts)?;
        }

        Ok(ResolvedAccounts { accounts, lookups })
    }

    fn resolve_all_lookups(
        &mut self,
        plans: &[MessageAccountPlan],
        accounts: &mut HashMap<Pubkey, AccountSharedData>,
    ) -> Result<(Vec<ResolvedLookup>, Vec<Pubkey>)> {
        let mut lookups = Vec::new();
        let mut lookup_pubkeys = Vec::new();
        let mut seen = HashSet::new();

        for plan in plans {
            for lookup_plan in &plan.address_lookups {
                let resolved = self.load_lookup_table(lookup_plan, accounts)?;
                for addr in
                    resolved.writable_addresses.iter().chain(resolved.readonly_addresses.iter())
                {
                    if seen.insert(*addr) {
                        lookup_pubkeys.push(*addr);
                    }
                }
                lookups.push(resolved);
            }
        }

        Ok((lookups, lookup_pubkeys))
    }

    fn append_accounts_inner(
        &mut self,
        resolved: &mut ResolvedAccounts,
        pubkeys: &[Pubkey],
    ) -> Result<()> {
        if pubkeys.is_empty() {
            return Ok(());
        }
        self.fetcher.fetch_accounts(pubkeys, &mut resolved.accounts).map_err(|e| {
            SonarSimError::AccountData {
                pubkey: None,
                reason: format!(
                    "Failed to fetch appended accounts: [{}]: {e}",
                    format_pubkeys(pubkeys)
                ),
            }
        })?;
        self.resolve_all_dependencies(&mut resolved.accounts)?;
        Ok(())
    }

    /// Runs built-in dependency rules in a loop until no new dependencies emerge.
    fn resolve_all_dependencies(
        &mut self,
        accounts: &mut HashMap<Pubkey, AccountSharedData>,
    ) -> Result<()> {
        let mut attempted_missing = HashSet::new();
        loop {
            let mut all_missing = Vec::new();
            let mut seen = HashSet::new();

            for key in collect_bpf_upgradeable_programdata_dependencies(accounts) {
                if seen.insert(key) {
                    all_missing.push(key);
                }
            }
            for key in collect_token_mint_dependencies(accounts) {
                if seen.insert(key) {
                    all_missing.push(key);
                }
            }

            if all_missing.is_empty() {
                break;
            }

            let mut newly_attemptable = Vec::new();
            for key in all_missing {
                if attempted_missing.insert(key) {
                    newly_attemptable.push(key);
                }
            }

            if newly_attemptable.is_empty() {
                log::warn!(
                    "Stopping dependency resolution: no newly-attemptable keys remain; unresolved dependencies: [{}]",
                    format_pubkeys(&attempted_missing.iter().copied().collect::<Vec<_>>())
                );
                break;
            }

            self.fetcher.fetch_accounts(&newly_attemptable, accounts).map_err(|e| {
                SonarSimError::AccountData {
                    pubkey: None,
                    reason: format!(
                        "Failed to fetch dependency accounts: [{}]: {e}",
                        format_pubkeys(&newly_attemptable)
                    ),
                }
            })?;
        }

        Ok(())
    }

    fn load_lookup_table(
        &mut self,
        plan: &AddressLookupPlan,
        accounts: &mut HashMap<Pubkey, AccountSharedData>,
    ) -> Result<ResolvedLookup> {
        self.fetcher.fetch_accounts(&[plan.account_key], accounts).map_err(|e| {
            SonarSimError::LookupTable {
                table: Some(plan.account_key),
                reason: format!(
                    "Failed to fetch address lookup table account `{}`: {e}",
                    plan.account_key
                ),
            }
        })?;

        let table_account =
            accounts.get(&plan.account_key).ok_or_else(|| SonarSimError::LookupTable {
                table: Some(plan.account_key),
                reason: format!(
                    "Address lookup table account `{}` missing from cache",
                    plan.account_key
                ),
            })?;

        expand_lookup_table(table_account, plan)
    }
}

impl AccountAppender for AccountLoader {
    fn append_accounts(
        &mut self,
        resolved: &mut ResolvedAccounts,
        pubkeys: &[Pubkey],
    ) -> Result<()> {
        self.append_accounts_inner(resolved, pubkeys)
    }
}

fn collect_initial_accounts(plans: &[MessageAccountPlan]) -> Vec<Pubkey> {
    let mut keys = Vec::new();
    let mut seen = HashSet::new();

    for plan in plans {
        for key in &plan.static_accounts {
            if seen.insert(*key) {
                keys.push(*key);
            }
        }
        for lookup in &plan.address_lookups {
            if seen.insert(lookup.account_key) {
                keys.push(lookup.account_key);
            }
        }
    }

    for sysvar in [Clock::id(), SlotHashes::id()] {
        if seen.insert(sysvar) {
            keys.push(sysvar);
        }
    }

    keys
}

/// Expand a fetched address lookup table account into resolved addresses.
///
/// Pure function: takes the already-fetched ALT account data and the lookup plan,
/// deserializes the table, and resolves writable/readonly addresses by index.
fn expand_lookup_table(
    table_account: &AccountSharedData,
    plan: &AddressLookupPlan,
) -> Result<ResolvedLookup> {
    let lookup_table = AddressLookupTable::deserialize(table_account.data()).map_err(|err| {
        SonarSimError::LookupTable {
            table: Some(plan.account_key),
            reason: format!("Failed to parse address lookup table `{}`: {err}", plan.account_key),
        }
    })?;
    let all_addresses = lookup_table.addresses.to_vec();

    let writable_addresses =
        resolve_lookup_indexes(&all_addresses, &plan.writable_indexes, plan.account_key).map_err(
            |e| SonarSimError::LookupTable {
                table: Some(plan.account_key),
                reason: format!(
                    "Failed to parse writable indexes for address lookup table `{}`: {e}",
                    plan.account_key
                ),
            },
        )?;
    let readonly_addresses =
        resolve_lookup_indexes(&all_addresses, &plan.readonly_indexes, plan.account_key).map_err(
            |e| SonarSimError::LookupTable {
                table: Some(plan.account_key),
                reason: format!(
                    "Failed to parse readonly indexes for address lookup table `{}`: {e}",
                    plan.account_key
                ),
            },
        )?;

    Ok(ResolvedLookup {
        account_key: plan.account_key,
        writable_indexes: plan.writable_indexes.clone(),
        readonly_indexes: plan.readonly_indexes.clone(),
        writable_addresses,
        readonly_addresses,
    })
}

fn resolve_lookup_indexes(
    addresses: &[Pubkey],
    indexes: &[u8],
    table_key: Pubkey,
) -> Result<Vec<Pubkey>> {
    indexes
        .iter()
        .map(|idx| {
            addresses.get(*idx as usize).copied().ok_or_else(|| SonarSimError::LookupTable {
                table: Some(table_key),
                reason: format!("Index {idx} out of address lookup table range"),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::rpc_provider::FakeAccountProvider;
    use crate::test_utils::{create_transfer_tx, make_token_account_shared, system_account};
    use solana_keypair::Keypair;
    use solana_signer::Signer;

    #[test]
    fn loader_returns_accounts_from_provider() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let mut accounts = std::collections::HashMap::new();
        accounts.insert(payer.pubkey(), system_account(10_000_000_000));
        accounts.insert(recipient, system_account(0));

        let mut loader =
            AccountLoader::with_provider(Arc::new(FakeAccountProvider::from_accounts(accounts)));

        let tx = create_transfer_tx(&payer, &recipient, 1000);
        let resolved = loader.load_for_transaction(&tx).expect("should load from fake provider");

        assert!(resolved.accounts.contains_key(&payer.pubkey()));
        assert!(resolved.accounts.contains_key(&recipient));
    }

    #[test]
    fn in_memory_cache_avoids_duplicate_provider_calls() {
        struct CountingProvider {
            accounts: std::collections::HashMap<Pubkey, AccountSharedData>,
            call_count: Arc<AtomicUsize>,
        }

        impl RpcAccountProvider for CountingProvider {
            fn get_multiple_accounts(
                &self,
                pubkeys: &[Pubkey],
            ) -> Result<Vec<Option<AccountSharedData>>> {
                self.call_count.fetch_add(1, Ordering::SeqCst);
                Ok(pubkeys.iter().map(|k| self.accounts.get(k).cloned()).collect())
            }
        }

        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let mut accounts = std::collections::HashMap::new();
        for (key, lamports) in [
            (payer.pubkey(), 10_000_000_000u64),
            (recipient, 0),
            (Clock::id(), 0),
            (SlotHashes::id(), 0),
            (solana_sdk_ids::system_program::id(), 0),
        ] {
            accounts.insert(key, AccountSharedData::from(system_account(lamports)));
        }

        let call_count = Arc::new(AtomicUsize::new(0));
        let provider = CountingProvider { accounts, call_count: call_count.clone() };

        let mut loader = AccountLoader::with_provider(Arc::new(provider));

        let tx = create_transfer_tx(&payer, &recipient, 1000);

        let _ = loader.load_for_transaction(&tx).unwrap();
        let first_call_count = call_count.load(Ordering::SeqCst);
        assert!(first_call_count > 0);

        let _ = loader.load_for_transaction(&tx).unwrap();
        let second_call_count = call_count.load(Ordering::SeqCst);
        assert_eq!(
            first_call_count, second_call_count,
            "second load should use cache, not call provider again"
        );
    }

    #[test]
    fn repeated_append_accounts_does_not_refetch_known_missing_token_mint() {
        struct CountingProvider {
            accounts: std::collections::HashMap<Pubkey, AccountSharedData>,
            call_count: Arc<AtomicUsize>,
        }

        impl RpcAccountProvider for CountingProvider {
            fn get_multiple_accounts(
                &self,
                pubkeys: &[Pubkey],
            ) -> Result<Vec<Option<AccountSharedData>>> {
                self.call_count.fetch_add(1, Ordering::SeqCst);
                Ok(pubkeys.iter().map(|k| self.accounts.get(k).cloned()).collect())
            }
        }

        let token_account = Pubkey::new_unique();
        let missing_mint = Pubkey::new_unique();
        let call_count = Arc::new(AtomicUsize::new(0));
        let provider = CountingProvider {
            accounts: HashMap::from([(token_account, make_token_account_shared(&missing_mint))]),
            call_count: call_count.clone(),
        };
        let mut loader = AccountLoader::with_provider(Arc::new(provider));
        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        loader.append_accounts(&mut resolved, &[token_account]).unwrap();
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            2,
            "first append should fetch token account and then attempt its missing mint once"
        );

        loader.append_accounts(&mut resolved, &[token_account]).unwrap();
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            2,
            "second append should not refetch known-missing mint dependency"
        );
    }

    #[test]
    fn append_accounts_fetches_additional_accounts() {
        let payer = Keypair::new();
        let extra = Pubkey::new_unique();

        let mut accounts = std::collections::HashMap::new();
        accounts.insert(payer.pubkey(), system_account(10_000_000_000));
        accounts.insert(extra, system_account(42));

        let mut loader =
            AccountLoader::with_provider(Arc::new(FakeAccountProvider::from_accounts(accounts)));

        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        loader.append_accounts(&mut resolved, &[extra]).unwrap();
        assert!(resolved.accounts.contains_key(&extra));
        assert_eq!(resolved.accounts[&extra].lamports(), 42);
    }

    #[test]
    fn load_for_transactions_merges_accounts_from_multiple_txs() {
        let payer = Keypair::new();
        let recipient1 = Pubkey::new_unique();
        let recipient2 = Pubkey::new_unique();

        let mut accounts = std::collections::HashMap::new();
        accounts.insert(payer.pubkey(), system_account(10_000_000_000));
        accounts.insert(recipient1, system_account(100));
        accounts.insert(recipient2, system_account(200));

        let mut loader =
            AccountLoader::with_provider(Arc::new(FakeAccountProvider::from_accounts(accounts)));

        let tx1 = create_transfer_tx(&payer, &recipient1, 50);
        let tx2 = create_transfer_tx(&payer, &recipient2, 100);
        let txs: Vec<&VersionedTransaction> = vec![&tx1, &tx2];

        let resolved = loader.load_for_transactions(&txs).expect("should merge accounts");

        assert!(resolved.accounts.contains_key(&payer.pubkey()));
        assert!(resolved.accounts.contains_key(&recipient1));
        assert!(resolved.accounts.contains_key(&recipient2));
    }

    #[test]
    fn load_for_transactions_empty_bundle() {
        let mut loader = AccountLoader::with_provider(Arc::new(FakeAccountProvider::empty()));

        let txs: Vec<&VersionedTransaction> = vec![];
        let resolved = loader.load_for_transactions(&txs).unwrap();
        assert!(resolved.accounts.is_empty());
        assert!(resolved.lookups.is_empty());
    }

    #[test]
    fn fetcher_accessor_returns_inner_fetcher() {
        let loader = AccountLoader::with_provider(Arc::new(FakeAccountProvider::empty()));
        let _fetcher = loader.fetcher();
    }

    #[test]
    fn dependency_resolution_stops_when_missing_keys_repeat() {
        struct MissingOnlyProvider {
            call_count: Arc<AtomicUsize>,
        }

        impl RpcAccountProvider for MissingOnlyProvider {
            fn get_multiple_accounts(
                &self,
                pubkeys: &[Pubkey],
            ) -> Result<Vec<Option<AccountSharedData>>> {
                self.call_count.fetch_add(1, Ordering::SeqCst);
                Ok(vec![None; pubkeys.len()])
            }
        }

        let token_account = Pubkey::new_unique();
        let missing_mint = Pubkey::new_unique();
        let call_count = Arc::new(AtomicUsize::new(0));

        let provider = MissingOnlyProvider { call_count: call_count.clone() };
        let mut loader = AccountLoader::with_provider(Arc::new(provider));

        let mut resolved = ResolvedAccounts {
            accounts: HashMap::from([(token_account, make_token_account_shared(&missing_mint))]),
            lookups: vec![],
        };

        loader.append_accounts(&mut resolved, &[token_account]).unwrap();

        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "known-missing dependency should only be fetched once"
        );
        assert!(!resolved.accounts.contains_key(&missing_mint));
    }

    #[test]
    fn dependency_resolution_offline_mode_stops_on_repeated_missing_keys() {
        struct NeverCalledProvider;

        impl RpcAccountProvider for NeverCalledProvider {
            fn get_multiple_accounts(
                &self,
                _pubkeys: &[Pubkey],
            ) -> Result<Vec<Option<AccountSharedData>>> {
                panic!("RPC provider should never be called in offline mode");
            }
        }

        struct OfflinePolicy;

        impl crate::types::FetchPolicy for OfflinePolicy {
            fn decide_rpc(&self, _unresolved: &[Pubkey]) -> crate::types::RpcDecision {
                crate::types::RpcDecision::Deny
            }
        }

        let token_account = Pubkey::new_unique();
        let missing_mint = Pubkey::new_unique();
        let policy = Arc::new(OfflinePolicy);
        let mut loader =
            AccountLoader::with_provider(Arc::new(NeverCalledProvider)).with_policy(policy);

        let mut resolved = ResolvedAccounts {
            accounts: HashMap::from([(token_account, make_token_account_shared(&missing_mint))]),
            lookups: vec![],
        };

        loader
            .append_accounts(&mut resolved, &[token_account])
            .expect("offline mode should not hang on repeated missing dependencies");

        assert!(!resolved.accounts.contains_key(&missing_mint));
    }

    // ── Pure function tests: collect_initial_accounts ──

    #[test]
    fn collect_initial_accounts_single_plan_no_lookups() {
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let plan =
            MessageAccountPlan { static_accounts: vec![key1, key2], address_lookups: vec![] };
        let keys = collect_initial_accounts(&[plan]);
        assert!(keys.contains(&key1));
        assert!(keys.contains(&key2));
        assert!(keys.contains(&Clock::id()));
        assert!(keys.contains(&SlotHashes::id()));
        assert_eq!(keys.len(), 4);
    }

    #[test]
    fn collect_initial_accounts_deduplicates_across_plans() {
        let shared = Pubkey::new_unique();
        let unique1 = Pubkey::new_unique();
        let unique2 = Pubkey::new_unique();
        let plan1 =
            MessageAccountPlan { static_accounts: vec![shared, unique1], address_lookups: vec![] };
        let plan2 =
            MessageAccountPlan { static_accounts: vec![shared, unique2], address_lookups: vec![] };
        let keys = collect_initial_accounts(&[plan1, plan2]);
        let shared_count = keys.iter().filter(|k| **k == shared).count();
        assert_eq!(shared_count, 1);
        assert_eq!(keys.len(), 5); // shared + unique1 + unique2 + Clock + SlotHashes
    }

    #[test]
    fn collect_initial_accounts_includes_lookup_table_keys() {
        let static_key = Pubkey::new_unique();
        let lookup_table_key = Pubkey::new_unique();
        let plan = MessageAccountPlan {
            static_accounts: vec![static_key],
            address_lookups: vec![AddressLookupPlan {
                account_key: lookup_table_key,
                writable_indexes: vec![0],
                readonly_indexes: vec![1],
            }],
        };
        let keys = collect_initial_accounts(&[plan]);
        assert!(keys.contains(&static_key));
        assert!(keys.contains(&lookup_table_key));
    }

    #[test]
    fn collect_initial_accounts_empty_plans() {
        let keys = collect_initial_accounts(&[]);
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&Clock::id()));
        assert!(keys.contains(&SlotHashes::id()));
    }

    // ── Pure function tests: resolve_lookup_indexes ──

    #[test]
    fn resolve_lookup_indexes_valid() {
        let addr0 = Pubkey::new_unique();
        let addr1 = Pubkey::new_unique();
        let addr2 = Pubkey::new_unique();
        let addresses = vec![addr0, addr1, addr2];
        let table_key = Pubkey::new_unique();
        let result = resolve_lookup_indexes(&addresses, &[2, 0], table_key).unwrap();
        assert_eq!(result, vec![addr2, addr0]);
    }

    #[test]
    fn resolve_lookup_indexes_out_of_bounds() {
        let addresses = vec![Pubkey::new_unique()];
        let table_key = Pubkey::new_unique();
        let err = resolve_lookup_indexes(&addresses, &[5], table_key).unwrap_err();
        assert!(err.to_string().contains("out of"));
        assert!(
            matches!(err, SonarSimError::LookupTable { table: Some(t), .. } if t == table_key),
            "error should include the table pubkey"
        );
    }

    #[test]
    fn resolve_lookup_indexes_empty() {
        let addresses = vec![Pubkey::new_unique()];
        let table_key = Pubkey::new_unique();
        let result = resolve_lookup_indexes(&addresses, &[], table_key).unwrap();
        assert!(result.is_empty());
    }

    // ── Pure function tests: expand_lookup_table ──

    /// Helper: build an `AccountSharedData` containing a valid serialized ALT
    /// with the given addresses.
    fn make_alt_account(addresses: &[Pubkey]) -> AccountSharedData {
        use solana_address_lookup_table_interface::state::{AddressLookupTable, LookupTableMeta};
        use std::borrow::Cow;

        let table = AddressLookupTable {
            meta: LookupTableMeta::default(), // active table, no authority
            addresses: Cow::Borrowed(addresses),
        };
        let data = table.serialize_for_tests().expect("serialize ALT for test");
        let mut account = AccountSharedData::default();
        account.set_data_from_slice(&data);
        account
    }

    #[test]
    fn expand_lookup_table_valid() {
        let addr0 = Pubkey::new_unique();
        let addr1 = Pubkey::new_unique();
        let addr2 = Pubkey::new_unique();
        let table_key = Pubkey::new_unique();

        let account = make_alt_account(&[addr0, addr1, addr2]);
        let plan = AddressLookupPlan {
            account_key: table_key,
            writable_indexes: vec![0, 2],
            readonly_indexes: vec![1],
        };

        let resolved = expand_lookup_table(&account, &plan).unwrap();
        assert_eq!(resolved.account_key, table_key);
        assert_eq!(resolved.writable_addresses, vec![addr0, addr2]);
        assert_eq!(resolved.readonly_addresses, vec![addr1]);
        assert_eq!(resolved.writable_indexes, vec![0, 2]);
        assert_eq!(resolved.readonly_indexes, vec![1]);
    }

    #[test]
    fn expand_lookup_table_malformed_data() {
        let plan = AddressLookupPlan {
            account_key: Pubkey::new_unique(),
            writable_indexes: vec![0],
            readonly_indexes: vec![],
        };

        let mut account = AccountSharedData::default();
        account.set_data_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

        let err = expand_lookup_table(&account, &plan).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Failed to parse address lookup table"), "unexpected error: {msg}");
    }

    #[test]
    fn bpf_dependency_resolution_discovers_programdata_transitively() {
        use solana_account::Account;
        use solana_loader_v3_interface::state::UpgradeableLoaderState;
        use solana_sdk_ids::bpf_loader_upgradeable;

        fn make_bpf_program(programdata_address: &Pubkey) -> AccountSharedData {
            let state =
                UpgradeableLoaderState::Program { programdata_address: *programdata_address };
            let data = bincode::serialize(&state).unwrap();
            AccountSharedData::from(Account {
                lamports: 1,
                data,
                owner: bpf_loader_upgradeable::id(),
                executable: true,
                rent_epoch: 0,
            })
        }

        let program_key = Pubkey::new_unique();
        let programdata_key = Pubkey::new_unique();

        // The programdata account itself (minimal, just needs to exist)
        let programdata_account = AccountSharedData::from(Account {
            lamports: 1,
            data: vec![0; 64],
            owner: bpf_loader_upgradeable::id(),
            executable: false,
            rent_epoch: 0,
        });

        let provider = FakeAccountProvider::new(HashMap::from([
            (program_key, make_bpf_program(&programdata_key)),
            (programdata_key, programdata_account),
        ]));

        let mut loader = AccountLoader::with_provider(Arc::new(provider));
        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        // Fetch only the program key; dependency resolution should discover programdata.
        loader.append_accounts(&mut resolved, &[program_key]).unwrap();

        assert!(
            resolved.accounts.contains_key(&program_key),
            "resolved accounts should contain the BPF program"
        );
        assert!(
            resolved.accounts.contains_key(&programdata_key),
            "resolved accounts should contain the programdata dependency discovered transitively"
        );
    }

    #[test]
    fn expand_lookup_table_index_out_of_range() {
        let addr0 = Pubkey::new_unique();
        let addr1 = Pubkey::new_unique();
        let table_key = Pubkey::new_unique();

        let account = make_alt_account(&[addr0, addr1]);
        let plan = AddressLookupPlan {
            account_key: table_key,
            writable_indexes: vec![5], // out of range
            readonly_indexes: vec![],
        };

        let err = expand_lookup_table(&account, &plan).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Failed to parse writable indexes"), "unexpected error: {msg}");
    }
}
