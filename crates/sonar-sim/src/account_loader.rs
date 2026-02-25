use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow};
use log::{debug, trace};
use solana_account::{Account, ReadableAccount};
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_clock::Clock;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;
use solana_slot_hashes::SlotHashes;
use solana_sysvar_id::SysvarId;
use solana_transaction::versioned::VersionedTransaction;
use spl_token::solana_program::program_pack::Pack;
use std::sync::Mutex;

use crate::rpc_provider::{RpcAccountProvider, SolanaRpcProvider};
use crate::transaction::{AddressLookupPlan, collect_account_plan};
use crate::types::{ResolvedAccounts, ResolvedLookup};

const MAX_ACCOUNTS_PER_REQUEST: usize = 100;

pub struct AccountLoader {
    provider: Arc<dyn RpcAccountProvider>,
    cache: Mutex<HashMap<Pubkey, Account>>,
}

impl AccountLoader {
    pub fn new(rpc_url: String) -> Result<Self> {
        if rpc_url.is_empty() {
            return Err(anyhow!("RPC URL cannot be empty"));
        }
        Ok(Self {
            provider: Arc::new(SolanaRpcProvider::new(rpc_url)),
            cache: Mutex::new(HashMap::new()),
        })
    }

    pub fn with_provider(provider: Arc<dyn RpcAccountProvider>) -> Self {
        Self {
            provider,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Expose the underlying provider so callers can reuse the RPC connection.
    pub fn provider(&self) -> Arc<dyn RpcAccountProvider> {
        Arc::clone(&self.provider)
    }

    pub fn load_for_transaction(
        &self,
        tx: &VersionedTransaction,
    ) -> Result<ResolvedAccounts> {
        let plan = collect_account_plan(tx);
        let mut accounts = HashMap::new();

        // Phase 1: Batch fetch static accounts, sysvars, and address lookup table accounts
        let mut initial_accounts = plan.static_accounts.clone();
        initial_accounts.push(Clock::id());
        initial_accounts.push(SlotHashes::id());
        for lookup in &plan.address_lookups {
            initial_accounts.push(lookup.account_key);
        }

        self.fetch_accounts(&initial_accounts, &mut accounts)
            .context("Failed to fetch initial accounts (static + sysvars + lookups)")?;

        self.ensure_upgradeable_dependencies(&mut accounts)
            .context("Failed to load upgradeable program metadata")?;

        let mut lookups = Vec::new();
        let mut writable_lookup_accounts = Vec::new();
        let mut readonly_lookup_accounts = Vec::new();

        for lookup_plan in &plan.address_lookups {
            let resolved = self.load_lookup_table(lookup_plan, &mut accounts)?;
            writable_lookup_accounts.extend(resolved.writable_addresses.iter().copied());
            readonly_lookup_accounts.extend(resolved.readonly_addresses.iter().copied());
            lookups.push(resolved);
        }

        // Phase 2: Batch fetch all resolved lookup accounts
        let mut lookup_accounts_to_fetch = writable_lookup_accounts.clone();
        lookup_accounts_to_fetch.extend_from_slice(&readonly_lookup_accounts);

        if !lookup_accounts_to_fetch.is_empty() {
            self.fetch_accounts(&lookup_accounts_to_fetch, &mut accounts).with_context(|| {
                format!(
                    "Failed to load accounts from address lookup tables: [{}]",
                    format_pubkeys(&lookup_accounts_to_fetch)
                )
            })?;

            self.ensure_upgradeable_dependencies(&mut accounts).context(
                "Failed to load upgradeable program dependencies for lookup table accounts",
            )?;
        }

        self.ensure_token_mint_accounts(&mut accounts)
            .context("Failed to load token mint accounts for transaction")?;

        Ok(ResolvedAccounts { accounts, lookups })
    }

    /// Load accounts for multiple transactions (bundle simulation).
    /// Merges all required accounts from all transactions and fetches them in a single batch.
    pub fn load_for_transactions(
        &self,
        txs: &[&VersionedTransaction],
    ) -> Result<ResolvedAccounts> {
        if txs.is_empty() {
            return Ok(ResolvedAccounts { accounts: HashMap::new(), lookups: Vec::new() });
        }

        let plans: Vec<_> = txs.iter().map(|tx| collect_account_plan(tx)).collect();

        let mut all_initial_accounts: Vec<Pubkey> = Vec::new();
        for plan in &plans {
            all_initial_accounts.extend(plan.static_accounts.iter().copied());
            for lookup in &plan.address_lookups {
                all_initial_accounts.push(lookup.account_key);
            }
        }
        all_initial_accounts.push(Clock::id());
        all_initial_accounts.push(SlotHashes::id());

        // Deduplicate
        let mut seen = HashSet::new();
        all_initial_accounts.retain(|key| seen.insert(*key));

        let mut accounts = HashMap::new();

        self.fetch_accounts(&all_initial_accounts, &mut accounts)
            .context("Failed to fetch initial accounts for bundle")?;

        self.ensure_upgradeable_dependencies(&mut accounts)
            .context("Failed to load upgradeable program metadata for bundle")?;

        let mut all_lookups = Vec::new();
        let mut all_lookup_accounts: Vec<Pubkey> = Vec::new();

        for plan in &plans {
            for lookup_plan in &plan.address_lookups {
                let resolved = self.load_lookup_table(lookup_plan, &mut accounts)?;
                all_lookup_accounts.extend(resolved.writable_addresses.iter().copied());
                all_lookup_accounts.extend(resolved.readonly_addresses.iter().copied());
                all_lookups.push(resolved);
            }
        }

        // Deduplicate lookup accounts
        let mut seen = HashSet::new();
        all_lookup_accounts.retain(|key| seen.insert(*key));

        if !all_lookup_accounts.is_empty() {
            self.fetch_accounts(&all_lookup_accounts, &mut accounts).with_context(|| {
                format!(
                    "Failed to load accounts from address lookup tables: [{}]",
                    format_pubkeys(&all_lookup_accounts)
                )
            })?;

            self.ensure_upgradeable_dependencies(&mut accounts)
                .context("Failed to load upgradeable program dependencies for lookup accounts")?;
        }

        self.ensure_token_mint_accounts(&mut accounts)
            .context("Failed to load token mint accounts for bundle")?;

        Ok(ResolvedAccounts { accounts, lookups: all_lookups })
    }

    pub fn append_accounts(
        &self,
        resolved: &mut ResolvedAccounts,
        pubkeys: &[Pubkey],
    ) -> Result<()> {
        if pubkeys.is_empty() {
            return Ok(());
        }
        self.fetch_accounts(pubkeys, &mut resolved.accounts).with_context(|| {
            format!("Failed to fetch appended accounts: [{}]", format_pubkeys(pubkeys))
        })?;
        self.ensure_upgradeable_dependencies(&mut resolved.accounts)
            .context("Failed to load upgradeable program dependencies for appended accounts")?;
        Ok(())
    }

    fn ensure_upgradeable_dependencies(
        &self,
        accounts: &mut HashMap<Pubkey, Account>,
    ) -> Result<()> {
        loop {
            let mut missing = Vec::new();
            for account in accounts.values() {
                if account.owner != bpf_loader_upgradeable::id() {
                    continue;
                }
                if let Ok(UpgradeableLoaderState::Program { programdata_address }) =
                    bincode::deserialize::<UpgradeableLoaderState>(account.data.as_slice())
                {
                    let programdata_key = Pubkey::new_from_array(programdata_address.to_bytes());
                    if !accounts.contains_key(&programdata_key) {
                        missing.push(programdata_key);
                    }
                }
            }

            if missing.is_empty() {
                break;
            }

            self.fetch_accounts(&missing, accounts).with_context(|| {
                format!("Failed to fetch ProgramData accounts: [{}]", format_pubkeys(&missing))
            })?;
        }

        Ok(())
    }

    fn load_lookup_table(
        &self,
        plan: &AddressLookupPlan,
        accounts: &mut HashMap<Pubkey, Account>,
    ) -> Result<ResolvedLookup> {
        self.fetch_accounts(&[plan.account_key], accounts).with_context(|| {
            format!("Failed to fetch address lookup table account `{}`", plan.account_key)
        })?;

        let table_account = accounts.get(&plan.account_key).ok_or_else(|| {
            anyhow!("Address lookup table account `{}` missing from cache", plan.account_key)
        })?;

        let lookup_table =
            AddressLookupTable::deserialize(table_account.data()).map_err(|err| {
                anyhow!("Failed to parse address lookup table `{}`: {err}", plan.account_key)
            })?;
        let all_addresses = lookup_table.addresses.to_vec();

        let writable_addresses = resolve_lookup_indexes(&all_addresses, &plan.writable_indexes)
            .with_context(|| {
                format!(
                    "Failed to parse writable indexes for address lookup table `{}`",
                    plan.account_key
                )
            })?;
        let readonly_addresses = resolve_lookup_indexes(&all_addresses, &plan.readonly_indexes)
            .with_context(|| {
                format!(
                    "Failed to parse readonly indexes for address lookup table `{}`",
                    plan.account_key
                )
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
        destination: &mut HashMap<Pubkey, Account>,
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

        // Layer 1: Check in-memory cache
        let mut to_fetch = Vec::new();
        {
            let cache = self.cache.lock().unwrap();
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

        // Layer 2: Fetch from RPC
        let total_count = to_fetch.len();
        for chunk in to_fetch.chunks(MAX_ACCOUNTS_PER_REQUEST) {
            let response = self.provider.get_multiple_accounts(chunk).with_context(|| {
                format!(
                    "getMultipleAccounts call failed, account list: [{}]",
                    format_pubkeys(chunk)
                )
            })?;

            if response.len() != chunk.len() {
                return Err(anyhow!(
                    "RPC returned count mismatch with request ({} != {})",
                    response.len(),
                    chunk.len()
                ));
            }

            for (pubkey, maybe_account) in chunk.iter().zip(response.into_iter()) {
                if let Some(account) = maybe_account {
                    destination.insert(*pubkey, account.clone());
                    let mut cache = self.cache.lock().unwrap();
                    cache.insert(*pubkey, account);
                }
            }
        }

        debug!("Successfully fetched {} accounts from RPC", total_count);
        Ok(())
    }

    fn ensure_token_mint_accounts(&self, accounts: &mut HashMap<Pubkey, Account>) -> Result<()> {
        let mut missing_mints = HashSet::new();
        for account in accounts.values() {
            if let Some(mint) = token_account_mint(account) {
                if !accounts.contains_key(&mint) {
                    missing_mints.insert(mint);
                }
            }
        }

        if missing_mints.is_empty() {
            return Ok(());
        }

        let mint_pubkeys: Vec<Pubkey> = missing_mints.into_iter().collect();
        self.fetch_accounts(&mint_pubkeys, accounts).with_context(|| {
            format!("Failed to fetch token mint accounts: [{}]", format_pubkeys(&mint_pubkeys))
        })?;
        Ok(())
    }
}

fn token_account_mint(account: &Account) -> Option<Pubkey> {
    let owner = account.owner;
    if owner == spl_token::ID {
        let token_account = spl_token::state::Account::unpack(account.data()).ok()?;
        return Some(Pubkey::new_from_array(token_account.mint.to_bytes()));
    }
    if owner == spl_token_2022::ID {
        use spl_token_2022::extension::StateWithExtensions;
        use spl_token_2022::state::Account as Token2022Account;
        let token_account = StateWithExtensions::<Token2022Account>::unpack(account.data()).ok()?;
        return Some(Pubkey::new_from_array(token_account.base.mint.to_bytes()));
    }
    None
}

fn resolve_lookup_indexes(addresses: &[Pubkey], indexes: &[u8]) -> Result<Vec<Pubkey>> {
    indexes
        .iter()
        .map(|idx| {
            addresses
                .get(*idx as usize)
                .copied()
                .ok_or_else(|| anyhow!("Index {idx} out of address lookup table range"))
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

        let loader = AccountLoader::with_provider(
            Arc::new(FakeAccountProvider::new(accounts)),
        );

        let tx = create_transfer_tx(&payer, &recipient, 1000);
        let resolved =
            loader.load_for_transaction(&tx).expect("should load from fake provider");

        assert!(resolved.accounts.contains_key(&payer.pubkey()));
        assert!(resolved.accounts.contains_key(&recipient));
    }

    #[test]
    fn in_memory_cache_avoids_duplicate_provider_calls() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct CountingProvider {
            accounts: std::collections::HashMap<Pubkey, Account>,
            call_count: Arc<AtomicUsize>,
        }

        impl RpcAccountProvider for CountingProvider {
            fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<Account>>> {
                self.call_count.fetch_add(1, Ordering::SeqCst);
                Ok(pubkeys.iter().map(|k| self.accounts.get(k).cloned()).collect())
            }
        }

        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let mut accounts = std::collections::HashMap::new();
        accounts.insert(payer.pubkey(), system_account(10_000_000_000));
        accounts.insert(recipient, system_account(0));
        accounts.insert(Clock::id(), system_account(0));
        accounts.insert(SlotHashes::id(), system_account(0));
        accounts.insert(solana_sdk_ids::system_program::id(), system_account(0));

        let call_count = Arc::new(AtomicUsize::new(0));
        let provider = CountingProvider { accounts, call_count: call_count.clone() };

        let loader = AccountLoader::with_provider(Arc::new(provider));

        let tx = create_transfer_tx(&payer, &recipient, 1000);

        // First load
        let _ = loader.load_for_transaction(&tx).unwrap();
        let first_call_count = call_count.load(Ordering::SeqCst);
        assert!(first_call_count > 0);

        // Second load of the same tx — should hit in-memory cache
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

        let loader = AccountLoader::with_provider(
            Arc::new(FakeAccountProvider::new(accounts)),
        );

        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        loader.append_accounts(&mut resolved, &[extra]).unwrap();
        assert!(resolved.accounts.contains_key(&extra));
        assert_eq!(resolved.accounts[&extra].lamports, 42);
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

        let loader = AccountLoader::with_provider(
            Arc::new(FakeAccountProvider::new(accounts)),
        );

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
