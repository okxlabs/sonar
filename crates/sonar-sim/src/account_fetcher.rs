use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use log::{debug, trace};
use solana_account::AccountSharedData;
use solana_pubkey::Pubkey;

use crate::error::{Result, SonarSimError};
use crate::rpc_provider::{RpcAccountProvider, SolanaRpcProvider};
use crate::types::AccountFetchMiddleware;

const MAX_ACCOUNTS_PER_REQUEST: usize = 100;

fn lock_cache(
    cache: &Mutex<HashMap<Pubkey, AccountSharedData>>,
) -> Result<std::sync::MutexGuard<'_, HashMap<Pubkey, AccountSharedData>>> {
    cache
        .lock()
        .map_err(|_| SonarSimError::Internal { reason: "account cache lock poisoned".into() })
}

/// Low-level account fetcher that handles dedup, caching, middleware
/// (local resolution / offline mode / progress), and batched RPC calls.
///
/// `AccountFetcher` owns the data-access plumbing; higher-level
/// orchestration (dependency resolution, lookup-table expansion, etc.)
/// lives in [`AccountLoader`](crate::account_loader::AccountLoader).
pub struct AccountFetcher {
    provider: Arc<dyn RpcAccountProvider>,
    cache: Mutex<HashMap<Pubkey, AccountSharedData>>,
    middleware: Option<Arc<dyn AccountFetchMiddleware>>,
}

impl AccountFetcher {
    pub fn new(rpc_url: String) -> Result<Self> {
        if rpc_url.is_empty() {
            return Err(SonarSimError::Validation { reason: "RPC URL cannot be empty".into() });
        }
        Ok(Self {
            provider: Arc::new(SolanaRpcProvider::new(rpc_url)),
            cache: Mutex::new(HashMap::new()),
            middleware: None,
        })
    }

    pub fn with_provider(provider: Arc<dyn RpcAccountProvider>) -> Self {
        Self { provider, cache: Mutex::new(HashMap::new()), middleware: None }
    }

    pub fn with_middleware(mut self, middleware: Arc<dyn AccountFetchMiddleware>) -> Self {
        self.middleware = Some(middleware);
        self
    }

    pub fn provider(&self) -> Arc<dyn RpcAccountProvider> {
        Arc::clone(&self.provider)
    }

    /// Fetch accounts by pubkey into `destination`.
    ///
    /// Handles the full pipeline:
    /// 1. Dedup against `destination` and the internal cache
    /// 2. Try middleware local resolution
    /// 3. Respect offline mode
    /// 4. Batch RPC requests (chunks of [`MAX_ACCOUNTS_PER_REQUEST`])
    /// 5. Report progress via middleware callback
    pub fn fetch_accounts(
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
            let response =
                self.provider.get_multiple_accounts(chunk).map_err(|e| SonarSimError::Rpc {
                    message: format!(
                        "getMultipleAccounts call failed, account list: [{}]: {e}",
                        format_pubkeys(chunk)
                    ),
                })?;

            if response.len() != chunk.len() {
                return Err(SonarSimError::Rpc {
                    message: format!(
                        "RPC returned count mismatch with request ({} != {})",
                        response.len(),
                        chunk.len()
                    ),
                });
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

    fn system_account(lamports: u64) -> Account {
        Account {
            lamports,
            data: vec![],
            owner: solana_sdk_ids::system_program::id(),
            executable: false,
            rent_epoch: 0,
        }
    }

    #[test]
    fn fetcher_deduplicates_against_destination() {
        let key = Pubkey::new_unique();
        let mut accounts = std::collections::HashMap::new();
        accounts.insert(key, system_account(100));

        let fetcher =
            AccountFetcher::with_provider(Arc::new(FakeAccountProvider::from_accounts(accounts)));

        let mut dest = HashMap::new();
        dest.insert(key, AccountSharedData::from(system_account(100)));

        fetcher.fetch_accounts(&[key], &mut dest).unwrap();
        assert_eq!(dest.len(), 1);
    }

    #[test]
    fn fetcher_caches_fetched_accounts() {
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

        let key = Pubkey::new_unique();
        let mut accounts = std::collections::HashMap::new();
        accounts.insert(key, AccountSharedData::from(system_account(42)));

        let call_count = Arc::new(AtomicUsize::new(0));
        let provider = CountingProvider { accounts, call_count: call_count.clone() };

        let fetcher = AccountFetcher::with_provider(Arc::new(provider));

        let mut dest1 = HashMap::new();
        fetcher.fetch_accounts(&[key], &mut dest1).unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        let mut dest2 = HashMap::new();
        fetcher.fetch_accounts(&[key], &mut dest2).unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 1, "second fetch should hit cache");
    }

    #[test]
    fn fetcher_returns_accounts_from_provider() {
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();

        let mut accounts = std::collections::HashMap::new();
        accounts.insert(key1, system_account(100));
        accounts.insert(key2, system_account(200));

        let fetcher =
            AccountFetcher::with_provider(Arc::new(FakeAccountProvider::from_accounts(accounts)));

        let mut dest = HashMap::new();
        fetcher.fetch_accounts(&[key1, key2], &mut dest).unwrap();
        assert!(dest.contains_key(&key1));
        assert!(dest.contains_key(&key2));
    }
}
