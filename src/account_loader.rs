use std::{
    collections::{HashMap, HashSet},
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
use std::sync::Mutex;

use crate::{
    cli::ProgramReplacement,
    transaction::{AddressLookupPlan, collect_account_plan},
};

const MAX_ACCOUNTS_PER_REQUEST: usize = 100;

pub struct AccountLoader {
    client: Arc<RpcClient>,
    cache: Mutex<HashMap<Pubkey, Account>>,
}

impl AccountLoader {
    pub fn new(rpc_url: String) -> Result<Self> {
        if rpc_url.is_empty() {
            return Err(anyhow!("RPC URL cannot be empty"));
        }
        Ok(Self { client: Arc::new(RpcClient::new(rpc_url)), cache: Mutex::new(HashMap::new()) })
    }

    pub fn load_for_transaction(
        &self,
        tx: &VersionedTransaction,
        _replacements: &[ProgramReplacement],
    ) -> Result<ResolvedAccounts> {
        let plan = collect_account_plan(tx);
        let mut accounts = HashMap::new();

        // Pre-load static accounts
        self.fetch_accounts(&plan.static_accounts, &mut accounts)
            .context("Failed to fetch static account info")?;

        self.fetch_accounts(&[Clock::id(), SlotHashes::id()], &mut accounts)
            .context("Failed to fetch sysvar accounts")?;

        self.ensure_upgradeable_dependencies(&mut accounts)
            .context("Failed to load upgradeable program metadata")?;

        let mut lookups = Vec::new();
        let mut writable_lookup_accounts = Vec::new();
        let mut readonly_lookup_accounts = Vec::new();

        // Process address lookup tables
        // Maintain consistent order with `build_lookup_locations`: aggregate all writable indexes first, then all readonly indexes
        for lookup_plan in &plan.address_lookups {
            let resolved = self.load_lookup_table(lookup_plan, &mut accounts)?;
            writable_lookup_accounts.extend(resolved.writable_addresses.iter().copied());
            readonly_lookup_accounts.extend(resolved.readonly_addresses.iter().copied());
            lookups.push(resolved);
        }

        if !writable_lookup_accounts.is_empty() {
            self.fetch_accounts(&writable_lookup_accounts, &mut accounts).with_context(|| {
                format!(
                    "Failed to load writable accounts from address lookup table: [{}]",
                    format_pubkeys(&writable_lookup_accounts)
                )
            })?;
            self.ensure_upgradeable_dependencies(&mut accounts)
                .context("Failed to load upgradeable program dependencies when processing writable accounts from address lookup table")?;
        }

        if !readonly_lookup_accounts.is_empty() {
            self.fetch_accounts(&readonly_lookup_accounts, &mut accounts).with_context(|| {
                format!(
                    "Failed to load readonly accounts from address lookup table: [{}]",
                    format_pubkeys(&readonly_lookup_accounts)
                )
            })?;
            self.ensure_upgradeable_dependencies(&mut accounts)
                .context("Failed to load upgradeable program dependencies when processing readonly accounts from address lookup table")?;
        }

        Ok(ResolvedAccounts { accounts, lookups })
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
                if let Ok(state) =
                    bincode::deserialize::<UpgradeableLoaderState>(account.data.as_slice())
                {
                    if let UpgradeableLoaderState::Program { programdata_address } = state {
                        let programdata_key =
                            Pubkey::new_from_array(programdata_address.to_bytes());
                        if !accounts.contains_key(&programdata_key) {
                            missing.push(programdata_key);
                        }
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
            ..Default::default()
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
