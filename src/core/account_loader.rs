use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

use anyhow::{Context, Result, anyhow};
use log::{debug, trace};
use solana_account::{Account, ReadableAccount};
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_client::rpc_client::RpcClient;
use solana_commitment_config::CommitmentConfig;

use solana_clock::Clock;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;
use solana_slot_hashes::SlotHashes;
use solana_sysvar_id::SysvarId;
use solana_transaction::versioned::VersionedTransaction;
use spl_token::solana_program::program_pack::Pack;
use std::sync::Mutex;

use crate::{
    cli::Replacement,
    core::idl_fetcher::IdlFetcher,
    core::transaction::{AddressLookupPlan, collect_account_plan},
    utils::progress::Progress,
};

const MAX_ACCOUNTS_PER_REQUEST: usize = 100;

pub struct AccountLoader {
    client: Arc<RpcClient>,
    cache: Mutex<HashMap<Pubkey, Account>>,
    progress: Option<Progress>,
    /// Optional local directory to load account JSON files from.
    local_dir: Option<PathBuf>,
    #[allow(dead_code)]
    cache_write_dir: Option<PathBuf>,
    /// When true, never fetch from RPC; missing accounts are treated as non-existent.
    offline: bool,
}

impl AccountLoader {
    pub fn new(
        rpc_url: String,
        local_dir: Option<PathBuf>,
        cache_write_dir: Option<PathBuf>,
        offline: bool,
        progress: Option<Progress>,
    ) -> Result<Self> {
        if rpc_url.is_empty() && !offline {
            return Err(anyhow!(
                "RPC URL cannot be empty (use --cache for local-only mode when cache exists)"
            ));
        }
        let url = if rpc_url.is_empty() { "http://localhost:8899".to_string() } else { rpc_url };
        Ok(Self {
            client: Arc::new(RpcClient::new(url)),
            cache: Mutex::new(HashMap::new()),
            progress,
            local_dir,
            cache_write_dir,
            offline,
        })
    }

    pub fn idl_fetcher(&self, progress: Option<Progress>) -> IdlFetcher {
        IdlFetcher::with_client(
            Arc::clone(&self.client),
            progress.or_else(|| self.progress.clone()),
        )
    }

    pub fn load_for_transaction(
        &self,
        tx: &VersionedTransaction,
        _replacements: &[Replacement],
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

        // Process address lookup tables
        // Since we pre-fetched the lookup table accounts in Phase 1, `load_lookup_table`
        // will hit the cache/map and not trigger new network requests.
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
        _replacements: &[Replacement],
    ) -> Result<ResolvedAccounts> {
        if txs.is_empty() {
            return Ok(ResolvedAccounts { accounts: HashMap::new(), lookups: Vec::new() });
        }

        // Collect account plans from all transactions
        let plans: Vec<_> = txs.iter().map(|tx| collect_account_plan(tx)).collect();

        // Merge all static accounts and sysvars
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

        // Phase 1: Batch fetch all initial accounts
        self.fetch_accounts(&all_initial_accounts, &mut accounts)
            .context("Failed to fetch initial accounts for bundle")?;

        self.ensure_upgradeable_dependencies(&mut accounts)
            .context("Failed to load upgradeable program metadata for bundle")?;

        // Process all address lookup tables and collect lookup accounts
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

        // Phase 2: Batch fetch all resolved lookup accounts
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
        // Fetch lookup table account
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

        // Layer 2: Try loading from local directory (if configured)
        if let Some(ref dir) = self.local_dir {
            let mut still_missing = Vec::new();
            for key in to_fetch {
                let path = dir.join(format!("{key}.json"));
                if path.exists() {
                    let account = crate::cli::parse_account_json(&path).map_err(|e| anyhow!(e))?;
                    destination.insert(key, account.clone());
                    self.cache.lock().unwrap().insert(key, account);
                    debug!("Loaded account {} from local file: {}", key, path.display());
                } else {
                    still_missing.push(key);
                }
            }
            to_fetch = still_missing;
        }

        if to_fetch.is_empty() {
            return Ok(());
        }

        // Layer 3: Offline mode — never fetch from RPC.
        // Native programs/sysvars are built into LiteSVM and need not be loaded from disk.
        // Missing non-native accounts are treated as non-existent and simulation continues.
        if self.offline {
            let non_native_missing: Vec<_> =
                to_fetch.iter().filter(|k| !is_native_or_sysvar(k)).collect();
            if !non_native_missing.is_empty() {
                eprintln!(
                    "Warning: offline mode: {} account(s) not found in cache directory (treated as non-existent): [{}]",
                    non_native_missing.len(),
                    non_native_missing.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(", ")
                );
            }
            return Ok(());
        }

        // Layer 4: Fetch remaining accounts from RPC
        let total_count = to_fetch.len();
        let mut requested_count = 0usize;
        for chunk in to_fetch.chunks(MAX_ACCOUNTS_PER_REQUEST) {
            let response = self.client.get_multiple_accounts(chunk).with_context(|| {
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
                requested_count += 1;
                self.set_progress_message(format!(
                    "loading account {} ({}/{})",
                    pubkey, requested_count, total_count
                ));
                if let Some(account) = maybe_account {
                    destination.insert(*pubkey, account.clone());
                    let mut cache = self.cache.lock().unwrap();
                    cache.insert(*pubkey, account);
                }
            }
        }

        debug!("Successfully fetched accounts: [{}]", format_pubkeys(pubkeys));
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

    pub fn fetch_transaction_by_signature(&self, signature: &str) -> Result<VersionedTransaction> {
        use solana_client::rpc_config::RpcTransactionConfig;
        use solana_transaction_status_client_types::UiTransactionEncoding;

        let signature = signature
            .parse()
            .with_context(|| format!("Invalid signature format: {}", signature))?;

        let config = RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Base64),
            commitment: Some(CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
        };

        let response =
            self.client.get_transaction_with_config(&signature, config).map_err(|e| {
                log::error!("RPC get_transaction error: {:?}", e);
                anyhow!("Failed to fetch transaction for signature: {}. Error: {}", signature, e)
            })?;

        let transaction = response.transaction;

        match transaction.transaction.decode() {
            Some(tx) => Ok(tx),
            None => Err(anyhow!("Failed to decode transaction from RPC response")),
        }
    }

    fn set_progress_message(&self, message: impl Into<std::borrow::Cow<'static, str>>) {
        if let Some(progress) = &self.progress {
            progress.set_message(message);
        }
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

#[derive(Debug, Clone)]
pub struct ResolvedAccounts {
    pub accounts: HashMap<Pubkey, Account>,
    pub lookups: Vec<ResolvedLookup>,
}

#[derive(Debug, Clone)]
pub struct ResolvedLookup {
    pub account_key: Pubkey,
    pub writable_indexes: Vec<u8>,
    pub readonly_indexes: Vec<u8>,
    pub writable_addresses: Vec<Pubkey>,
    pub readonly_addresses: Vec<Pubkey>,
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

/// Returns `true` if the pubkey is a well-known native program or sysvar
/// that is built into LiteSVM and does not need to be loaded from disk.
fn is_native_or_sysvar(pubkey: &Pubkey) -> bool {
    crate::utils::native_ids::is_native_or_sysvar(pubkey)
}
