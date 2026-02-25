use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use log::{debug, trace};
use solana_account::{AccountSharedData, ReadableAccount};
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_clock::Clock;
use solana_pubkey::Pubkey;
use solana_slot_hashes::SlotHashes;
use solana_sysvar_id::SysvarId;
use solana_transaction::versioned::VersionedTransaction;
use std::sync::Mutex;

use crate::error::{Result, SonarSimError};
use crate::resolvers::{AccountDependencyResolver, default_resolvers};
use crate::rpc_provider::{RpcAccountProvider, SolanaRpcProvider};
use crate::transaction::{AddressLookupPlan, collect_account_plan};
use crate::types::{AccountAppender, AccountFetchMiddleware, ResolvedAccounts, ResolvedLookup};

const MAX_ACCOUNTS_PER_REQUEST: usize = 100;

fn lock_cache(
    cache: &Mutex<HashMap<Pubkey, AccountSharedData>>,
) -> Result<std::sync::MutexGuard<'_, HashMap<Pubkey, AccountSharedData>>> {
    cache.lock().map_err(|_| SonarSimError::Internal("account cache lock poisoned".into()))
}

pub struct AccountLoader {
    provider: Arc<dyn RpcAccountProvider>,
    cache: Mutex<HashMap<Pubkey, AccountSharedData>>,
    middleware: Option<Arc<dyn AccountFetchMiddleware>>,
    resolvers: Vec<Box<dyn AccountDependencyResolver>>,
}

impl AccountLoader {
    pub fn new(rpc_url: String) -> Result<Self> {
        if rpc_url.is_empty() {
            return Err(SonarSimError::Validation("RPC URL cannot be empty".into()));
        }
        Ok(Self {
            provider: Arc::new(SolanaRpcProvider::new(rpc_url)),
            cache: Mutex::new(HashMap::new()),
            middleware: None,
            resolvers: default_resolvers(),
        })
    }

    pub fn with_provider(provider: Arc<dyn RpcAccountProvider>) -> Self {
        Self {
            provider,
            cache: Mutex::new(HashMap::new()),
            middleware: None,
            resolvers: default_resolvers(),
        }
    }

    /// Attach an optional middleware for local account resolution,
    /// offline mode, and progress reporting. Returns `self` for chaining.
    pub fn with_middleware(mut self, middleware: Arc<dyn AccountFetchMiddleware>) -> Self {
        self.middleware = Some(middleware);
        self
    }

    /// Replace the default dependency resolvers with a custom set.
    pub fn with_resolvers(mut self, resolvers: Vec<Box<dyn AccountDependencyResolver>>) -> Self {
        self.resolvers = resolvers;
        self
    }

    /// Expose the underlying provider so callers can reuse the RPC connection.
    pub fn provider(&self) -> Arc<dyn RpcAccountProvider> {
        Arc::clone(&self.provider)
    }

    pub fn load_for_transaction(&self, tx: &VersionedTransaction) -> Result<ResolvedAccounts> {
        self.load_from_plans(&[collect_account_plan(tx)])
    }

    /// Load accounts for multiple transactions (bundle simulation).
    /// Merges all required accounts from all transactions and fetches them in a single batch.
    pub fn load_for_transactions(&self, txs: &[&VersionedTransaction]) -> Result<ResolvedAccounts> {
        if txs.is_empty() {
            return Ok(ResolvedAccounts { accounts: HashMap::new(), lookups: Vec::new() });
        }
        let plans: Vec<_> = txs.iter().map(|tx| collect_account_plan(tx)).collect();
        self.load_from_plans(&plans)
    }

    fn load_from_plans(
        &self,
        plans: &[crate::transaction::MessageAccountPlan],
    ) -> Result<ResolvedAccounts> {
        let initial = collect_initial_accounts(plans);
        let mut accounts = HashMap::new();

        self.fetch_accounts(&initial, &mut accounts).map_err(|e| {
            SonarSimError::AccountData(format!(
                "Failed to fetch initial accounts (static + sysvars + lookups): {e}"
            ))
        })?;
        self.resolve_all_dependencies(&mut accounts)?;

        let (lookups, lookup_pubkeys) = self.resolve_all_lookups(plans, &mut accounts)?;

        if !lookup_pubkeys.is_empty() {
            self.fetch_accounts(&lookup_pubkeys, &mut accounts).map_err(|e| {
                SonarSimError::AccountData(format!(
                    "Failed to load accounts from address lookup tables: [{}]: {e}",
                    format_pubkeys(&lookup_pubkeys)
                ))
            })?;
            self.resolve_all_dependencies(&mut accounts)?;
        }

        Ok(ResolvedAccounts { accounts, lookups })
    }

    fn resolve_all_lookups(
        &self,
        plans: &[crate::transaction::MessageAccountPlan],
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
        &self,
        resolved: &mut ResolvedAccounts,
        pubkeys: &[Pubkey],
    ) -> Result<()> {
        if pubkeys.is_empty() {
            return Ok(());
        }
        self.fetch_accounts(pubkeys, &mut resolved.accounts).map_err(|e| {
            SonarSimError::AccountData(format!(
                "Failed to fetch appended accounts: [{}]: {e}",
                format_pubkeys(pubkeys)
            ))
        })?;
        self.resolve_all_dependencies(&mut resolved.accounts)?;
        Ok(())
    }

    /// Runs all registered resolvers in a loop until no new dependencies emerge.
    fn resolve_all_dependencies(
        &self,
        accounts: &mut HashMap<Pubkey, AccountSharedData>,
    ) -> Result<()> {
        loop {
            let mut all_missing = Vec::new();
            let mut seen = HashSet::new();

            for resolver in &self.resolvers {
                for key in resolver.resolve_dependencies(accounts) {
                    if seen.insert(key) {
                        all_missing.push(key);
                    }
                }
            }

            if all_missing.is_empty() {
                break;
            }

            self.fetch_accounts(&all_missing, accounts).map_err(|e| {
                SonarSimError::AccountData(format!(
                    "Failed to fetch dependency accounts: [{}]: {e}",
                    format_pubkeys(&all_missing)
                ))
            })?;
        }

        Ok(())
    }

    fn load_lookup_table(
        &self,
        plan: &AddressLookupPlan,
        accounts: &mut HashMap<Pubkey, AccountSharedData>,
    ) -> Result<ResolvedLookup> {
        self.fetch_accounts(&[plan.account_key], accounts).map_err(|e| {
            SonarSimError::LookupTable(format!(
                "Failed to fetch address lookup table account `{}`: {e}",
                plan.account_key
            ))
        })?;

        let table_account = accounts.get(&plan.account_key).ok_or_else(|| {
            SonarSimError::LookupTable(format!(
                "Address lookup table account `{}` missing from cache",
                plan.account_key
            ))
        })?;

        let lookup_table =
            AddressLookupTable::deserialize(table_account.data()).map_err(|err| {
                SonarSimError::LookupTable(format!(
                    "Failed to parse address lookup table `{}`: {err}",
                    plan.account_key
                ))
            })?;
        let all_addresses = lookup_table.addresses.to_vec();

        let writable_addresses = resolve_lookup_indexes(&all_addresses, &plan.writable_indexes)
            .map_err(|e| {
                SonarSimError::LookupTable(format!(
                    "Failed to parse writable indexes for address lookup table `{}`: {e}",
                    plan.account_key
                ))
            })?;
        let readonly_addresses = resolve_lookup_indexes(&all_addresses, &plan.readonly_indexes)
            .map_err(|e| {
                SonarSimError::LookupTable(format!(
                    "Failed to parse readonly indexes for address lookup table `{}`: {e}",
                    plan.account_key
                ))
            })?;

        Ok(ResolvedLookup {
            account_key: plan.account_key,
            writable_indexes: plan.writable_indexes.clone(),
            readonly_indexes: plan.readonly_indexes.clone(),
            writable_addresses,
            readonly_addresses,
        })
    }

    fn fetch_accounts(
        &self,
        pubkeys: &[Pubkey],
        destination: &mut HashMap<Pubkey, AccountSharedData>,
    ) -> Result<()> {
        let mut unique = Vec::new();
        let mut seen = HashSet::new();
        for key in pubkeys {
            if destination.contains_key(key) {
                continue;
            }
            if seen.insert(*key) {
                unique.push(*key);
            }
        }

        if unique.is_empty() {
            return Ok(());
        }

        trace!("Preparing to fetch {} accounts: [{}]", unique.len(), format_pubkeys(&unique));

        let mut to_fetch = Vec::new();
        {
            let cache = lock_cache(&self.cache)?;
            for key in unique {
                if let Some(account) = cache.get(&key) {
                    destination.insert(key, account.clone());
                } else {
                    to_fetch.push(key);
                }
            }
        }

        if to_fetch.is_empty() {
            return Ok(());
        }

        if let Some(ref middleware) = self.middleware {
            let resolved = middleware.try_resolve_local(&to_fetch)?;
            if !resolved.is_empty() {
                let mut still_missing = Vec::new();
                for key in to_fetch {
                    if let Some(account) = resolved.get(&key) {
                        destination.insert(key, account.clone());
                        lock_cache(&self.cache)?.insert(key, account.clone());
                    } else {
                        still_missing.push(key);
                    }
                }
                to_fetch = still_missing;
            }

            if to_fetch.is_empty() {
                return Ok(());
            }

            if middleware.is_offline() {
                middleware.on_offline_missing(&to_fetch);
                return Ok(());
            }
        }

        let total_count = to_fetch.len();
        let mut requested_count = 0usize;
        for chunk in to_fetch.chunks(MAX_ACCOUNTS_PER_REQUEST) {
            let response = self.provider.get_multiple_accounts(chunk).map_err(|e| {
                SonarSimError::Rpc(
                    format!(
                        "getMultipleAccounts call failed, account list: [{}]: {e}",
                        format_pubkeys(chunk)
                    )
                    .into(),
                )
            })?;

            if response.len() != chunk.len() {
                return Err(SonarSimError::Rpc(
                    format!(
                        "RPC returned count mismatch with request ({} != {})",
                        response.len(),
                        chunk.len()
                    )
                    .into(),
                ));
            }

            for (pubkey, maybe_account) in chunk.iter().zip(response.into_iter()) {
                requested_count += 1;
                if let Some(ref middleware) = self.middleware {
                    middleware.on_fetch_progress(pubkey, requested_count, total_count);
                }
                if let Some(account) = maybe_account {
                    destination.insert(*pubkey, account.clone());
                    let mut cache = lock_cache(&self.cache)?;
                    cache.insert(*pubkey, account);
                }
            }
        }

        debug!("Successfully fetched {} accounts from RPC", total_count);
        Ok(())
    }
}

impl AccountAppender for AccountLoader {
    fn append_accounts(&self, resolved: &mut ResolvedAccounts, pubkeys: &[Pubkey]) -> Result<()> {
        self.append_accounts_inner(resolved, pubkeys)
    }
}

fn collect_initial_accounts(plans: &[crate::transaction::MessageAccountPlan]) -> Vec<Pubkey> {
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

fn resolve_lookup_indexes(addresses: &[Pubkey], indexes: &[u8]) -> Result<Vec<Pubkey>> {
    indexes
        .iter()
        .map(|idx| {
            addresses.get(*idx as usize).copied().ok_or_else(|| {
                SonarSimError::LookupTable(format!("Index {idx} out of address lookup table range"))
            })
        })
        .collect()
}

fn format_pubkeys(pubkeys: &[Pubkey]) -> String {
    const MAX_DISPLAY: usize = 10;
    if pubkeys.len() <= MAX_DISPLAY {
        return pubkeys.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
    }
    let mut rendered =
        pubkeys.iter().take(MAX_DISPLAY).map(ToString::to_string).collect::<Vec<_>>();
    rendered.push(format!("... total {}", pubkeys.len()));
    rendered.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc_provider::FakeAccountProvider;
    use solana_account::Account;
    use solana_hash::Hash;
    use solana_keypair::Keypair;
    use solana_message::Message;
    use solana_signer::Signer;
    use solana_system_interface::instruction as system_instruction;
    use solana_transaction::Transaction;

    fn system_account(lamports: u64) -> Account {
        Account {
            lamports,
            data: vec![],
            owner: solana_sdk_ids::system_program::id(),
            executable: false,
            rent_epoch: 0,
        }
    }

    fn create_transfer_tx(
        payer: &Keypair,
        recipient: &Pubkey,
        lamports: u64,
    ) -> VersionedTransaction {
        let blockhash = Hash::new_unique();
        let ix = system_instruction::transfer(&payer.pubkey(), recipient, lamports);
        let message = Message::new(&[ix], Some(&payer.pubkey()));
        VersionedTransaction::from(Transaction::new(&[payer], message, blockhash))
    }

    #[test]
    fn loader_returns_accounts_from_provider() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let mut accounts = std::collections::HashMap::new();
        accounts.insert(payer.pubkey(), system_account(10_000_000_000));
        accounts.insert(recipient, system_account(0));

        let loader =
            AccountLoader::with_provider(Arc::new(FakeAccountProvider::from_accounts(accounts)));

        let tx = create_transfer_tx(&payer, &recipient, 1000);
        let resolved = loader.load_for_transaction(&tx).expect("should load from fake provider");

        assert!(resolved.accounts.contains_key(&payer.pubkey()));
        assert!(resolved.accounts.contains_key(&recipient));
    }

    #[test]
    fn in_memory_cache_avoids_duplicate_provider_calls() {
        use std::sync::atomic::{AtomicUsize, Ordering};

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

        let loader = AccountLoader::with_provider(Arc::new(provider));

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
    fn append_accounts_fetches_additional_accounts() {
        let payer = Keypair::new();
        let extra = Pubkey::new_unique();

        let mut accounts = std::collections::HashMap::new();
        accounts.insert(payer.pubkey(), system_account(10_000_000_000));
        accounts.insert(extra, system_account(42));

        let loader =
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

        let loader =
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
        let loader = AccountLoader::with_provider(Arc::new(FakeAccountProvider::empty()));

        let txs: Vec<&VersionedTransaction> = vec![];
        let resolved = loader.load_for_transactions(&txs).unwrap();
        assert!(resolved.accounts.is_empty());
        assert!(resolved.lookups.is_empty());
    }
}
