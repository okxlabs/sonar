use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use log::{debug, trace};
use solana_account::AccountSharedData;
use solana_pubkey::Pubkey;

use crate::error::{Result, SonarSimError};
use crate::rpc_provider::{RpcAccountProvider, SolanaRpcProvider};
use crate::types::{AccountSource, FetchEvent, FetchObserver, FetchPolicy, RpcDecision};

/// Default maximum number of accounts per `getMultipleAccounts` RPC call.
/// Matches the Solana validator's built-in limit.
pub const DEFAULT_RPC_BATCH_SIZE: usize = 100;

/// Low-level account fetcher that handles dedup, caching, source resolution,
/// policy gating, observer events, and batched RPC calls.
///
/// `AccountFetcher` owns the data-access plumbing; higher-level
/// orchestration (dependency resolution, lookup-table expansion, etc.)
/// lives in [`AccountLoader`](crate::account_loader::AccountLoader).
pub struct AccountFetcher {
    provider: Arc<dyn RpcAccountProvider>,
    cache: HashMap<Pubkey, AccountSharedData>,
    missing_cache: HashSet<Pubkey>,
    sources: Vec<Arc<dyn AccountSource>>,
    policy: Option<Arc<dyn FetchPolicy>>,
    observers: Vec<Arc<dyn FetchObserver>>,
    rpc_batch_size: usize,
}

impl AccountFetcher {
    pub fn new(rpc_url: String) -> Result<Self> {
        if rpc_url.is_empty() {
            return Err(SonarSimError::Validation { reason: "RPC URL cannot be empty".into() });
        }
        Ok(Self::with_provider(Arc::new(SolanaRpcProvider::new(rpc_url))))
    }

    pub fn with_provider(provider: Arc<dyn RpcAccountProvider>) -> Self {
        Self {
            provider,
            cache: HashMap::new(),
            missing_cache: HashSet::new(),
            sources: Vec::new(),
            policy: None,
            observers: Vec::new(),
            rpc_batch_size: DEFAULT_RPC_BATCH_SIZE,
        }
    }

    /// Set the maximum number of accounts per `getMultipleAccounts` RPC call.
    pub fn with_rpc_batch_size(mut self, size: usize) -> Self {
        self.rpc_batch_size = size.max(1);
        self
    }

    pub fn with_source(mut self, source: Arc<dyn AccountSource>) -> Self {
        self.sources.push(source);
        self
    }

    pub fn with_policy(mut self, policy: Arc<dyn FetchPolicy>) -> Self {
        self.policy = Some(policy);
        self
    }

    pub fn with_observer(mut self, observer: Arc<dyn FetchObserver>) -> Self {
        self.observers.push(observer);
        self
    }

    pub fn provider(&self) -> Arc<dyn RpcAccountProvider> {
        Arc::clone(&self.provider)
    }

    pub fn rpc_batch_size(&self) -> usize {
        self.rpc_batch_size
    }

    /// Fetch accounts by pubkey into `destination`.
    ///
    /// Handles the full pipeline:
    /// 1. Dedup against `destination` and the internal cache
    /// 2. Resolve accounts from configured sources
    /// 3. Gate RPC access via policy decision
    /// 4. Batch RPC requests (chunks of [`MAX_ACCOUNTS_PER_REQUEST`])
    /// 5. Emit observer events
    pub fn fetch_accounts(
        &mut self,
        pubkeys: &[Pubkey],
        destination: &mut HashMap<Pubkey, AccountSharedData>,
    ) -> Result<()> {
        let mut unique = Vec::with_capacity(pubkeys.len());
        let mut seen = HashSet::with_capacity(pubkeys.len());
        for key in pubkeys {
            if crate::known_programs::is_litesvm_builtin_program(key) {
                continue;
            }
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
        for key in unique {
            if let Some(account) = self.cache.get(&key) {
                destination.insert(key, account.clone());
            } else {
                to_fetch.push(key);
            }
        }

        if to_fetch.is_empty() {
            return Ok(());
        }

        for source in &self.sources {
            if to_fetch.is_empty() {
                break;
            }
            let resolved = source.resolve(&to_fetch)?;
            if resolved.is_empty() {
                continue;
            }

            let mut still_missing = Vec::new();
            for key in to_fetch {
                if let Some(account) = resolved.get(&key) {
                    destination.insert(key, account.clone());
                    self.cache.insert(key, account.clone());
                    self.missing_cache.remove(&key);
                    self.notify(FetchEvent::LocalResolved { pubkey: key });
                } else {
                    still_missing.push(key);
                }
            }
            to_fetch = still_missing;
        }

        if to_fetch.is_empty() {
            return Ok(());
        }

        if let Some(policy) = &self.policy {
            if policy.decide_rpc(&to_fetch) == RpcDecision::Deny {
                self.notify(FetchEvent::RpcSkippedByPolicy { missing: to_fetch });
                return Ok(());
            }
        }

        to_fetch.retain(|key| !self.missing_cache.contains(key));
        if to_fetch.is_empty() {
            return Ok(());
        }

        let total_count = to_fetch.len();
        let chunks: Vec<&[Pubkey]> = to_fetch.chunks(self.rpc_batch_size).collect();

        for (batch_index, chunk) in chunks.iter().enumerate() {
            self.notify(FetchEvent::RpcBatchStarted {
                batch_index,
                batch_size: chunk.len(),
                total_requested: total_count,
            });
        }

        // Fetch all batches in parallel when multiple chunks are needed.
        // Single-batch case avoids thread overhead.
        let batch_results: Vec<Result<Vec<Option<AccountSharedData>>>> = if chunks.len() <= 1 {
            chunks.iter().map(|&chunk| self.provider.get_multiple_accounts(chunk)).collect()
        } else {
            let provider = &self.provider;
            std::thread::scope(|s| {
                let handles: Vec<_> = chunks
                    .iter()
                    .map(|&chunk| {
                        let provider = Arc::clone(provider);
                        s.spawn(move || provider.get_multiple_accounts(chunk))
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().expect("RPC batch thread panicked")).collect()
            })
        };

        // Process results and update caches
        let mut requested_count = 0usize;
        let mut fetched_count = 0usize;
        for (&chunk, result) in chunks.iter().zip(batch_results) {
            let response = result.map_err(|e| SonarSimError::Rpc {
                reason: format!(
                    "getMultipleAccounts call failed, account list: [{}]: {e}",
                    format_pubkeys(chunk)
                ),
            })?;

            if response.len() != chunk.len() {
                return Err(SonarSimError::Rpc {
                    reason: format!(
                        "RPC returned count mismatch with request ({} != {})",
                        response.len(),
                        chunk.len()
                    ),
                });
            }

            for (pubkey, maybe_account) in chunk.iter().zip(response.into_iter()) {
                requested_count += 1;
                self.notify(FetchEvent::RpcProgress {
                    pubkey: *pubkey,
                    current: requested_count,
                    total: total_count,
                });
                if let Some(account) = maybe_account {
                    destination.insert(*pubkey, account.clone());
                    self.cache.insert(*pubkey, account);
                    self.missing_cache.remove(pubkey);
                    fetched_count += 1;
                } else {
                    self.missing_cache.insert(*pubkey);
                }
            }
        }

        self.notify(FetchEvent::RpcFinished {
            requested: total_count,
            fetched: fetched_count,
            missing: total_count.saturating_sub(fetched_count),
        });

        debug!("Successfully fetched {} accounts from RPC", total_count);
        Ok(())
    }

    fn notify(&self, event: FetchEvent) {
        for observer in &self.observers {
            observer.on_event(&event);
        }
    }
}

pub(crate) fn format_pubkeys(pubkeys: &[Pubkey]) -> String {
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
    use crate::test_utils::system_account;
    use crate::types::{AccountSource, FetchObserver, FetchPolicy};
    use std::str::FromStr;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn fetcher_deduplicates_against_destination() {
        let key = Pubkey::new_unique();
        let mut accounts = std::collections::HashMap::new();
        accounts.insert(key, system_account(100));

        let mut fetcher =
            AccountFetcher::with_provider(Arc::new(FakeAccountProvider::from_accounts(accounts)));

        let mut dest = HashMap::new();
        dest.insert(key, AccountSharedData::from(system_account(100)));

        fetcher.fetch_accounts(&[key], &mut dest).unwrap();
        assert_eq!(dest.len(), 1);
    }

    #[test]
    fn fetcher_caches_fetched_accounts() {
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

        let mut fetcher = AccountFetcher::with_provider(Arc::new(provider));

        let mut dest1 = HashMap::new();
        fetcher.fetch_accounts(&[key], &mut dest1).unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        let mut dest2 = HashMap::new();
        fetcher.fetch_accounts(&[key], &mut dest2).unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 1, "second fetch should hit cache");
    }

    #[test]
    fn fetcher_negative_cache_avoids_refetch_for_known_missing() {
        struct CountingProvider {
            call_count: Arc<AtomicUsize>,
        }

        impl RpcAccountProvider for CountingProvider {
            fn get_multiple_accounts(
                &self,
                pubkeys: &[Pubkey],
            ) -> Result<Vec<Option<AccountSharedData>>> {
                self.call_count.fetch_add(1, Ordering::SeqCst);
                Ok(pubkeys.iter().map(|_| None).collect())
            }
        }

        let key = Pubkey::new_unique();
        let call_count = Arc::new(AtomicUsize::new(0));
        let provider = CountingProvider { call_count: call_count.clone() };
        let mut fetcher = AccountFetcher::with_provider(Arc::new(provider));

        let mut dest1 = HashMap::new();
        fetcher.fetch_accounts(&[key], &mut dest1).unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
        assert!(fetcher.missing_cache.contains(&key));

        let mut dest2 = HashMap::new();
        fetcher.fetch_accounts(&[key], &mut dest2).unwrap();
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "known-missing key should not trigger repeated RPC calls"
        );
    }

    #[test]
    fn fetcher_local_resolution_clears_missing_cache() {
        struct EmptyProvider;

        impl RpcAccountProvider for EmptyProvider {
            fn get_multiple_accounts(
                &self,
                pubkeys: &[Pubkey],
            ) -> Result<Vec<Option<AccountSharedData>>> {
                Ok(pubkeys.iter().map(|_| None).collect())
            }
        }

        struct ToggleSource {
            key: Pubkey,
            account: AccountSharedData,
            enabled: Arc<AtomicBool>,
        }

        impl AccountSource for ToggleSource {
            fn resolve(&self, pubkeys: &[Pubkey]) -> Result<HashMap<Pubkey, AccountSharedData>> {
                if !self.enabled.load(Ordering::SeqCst) {
                    return Ok(HashMap::new());
                }
                let mut found = HashMap::new();
                if pubkeys.iter().any(|k| *k == self.key) {
                    found.insert(self.key, self.account.clone());
                }
                Ok(found)
            }
        }

        let key = Pubkey::new_unique();
        let enabled = Arc::new(AtomicBool::new(false));
        let source = Arc::new(ToggleSource {
            key,
            account: AccountSharedData::from(system_account(99)),
            enabled: enabled.clone(),
        });

        let mut fetcher =
            AccountFetcher::with_provider(Arc::new(EmptyProvider)).with_source(source);

        let mut dest1 = HashMap::new();
        fetcher.fetch_accounts(&[key], &mut dest1).unwrap();
        assert!(fetcher.missing_cache.contains(&key));
        assert!(!dest1.contains_key(&key));

        enabled.store(true, Ordering::SeqCst);
        let mut dest2 = HashMap::new();
        fetcher.fetch_accounts(&[key], &mut dest2).unwrap();

        assert!(dest2.contains_key(&key));
        assert!(!fetcher.missing_cache.contains(&key));
    }

    #[test]
    fn fetcher_returns_accounts_from_provider() {
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();

        let mut accounts = std::collections::HashMap::new();
        accounts.insert(key1, system_account(100));
        accounts.insert(key2, system_account(200));

        let mut fetcher =
            AccountFetcher::with_provider(Arc::new(FakeAccountProvider::from_accounts(accounts)));

        let mut dest = HashMap::new();
        fetcher.fetch_accounts(&[key1, key2], &mut dest).unwrap();
        assert!(dest.contains_key(&key1));
        assert!(dest.contains_key(&key2));
    }

    #[test]
    fn fetcher_policy_can_skip_rpc_and_emit_event() {
        struct NeverCalledProvider;

        impl RpcAccountProvider for NeverCalledProvider {
            fn get_multiple_accounts(
                &self,
                _pubkeys: &[Pubkey],
            ) -> Result<Vec<Option<AccountSharedData>>> {
                panic!("RPC provider should not be called when policy denies");
            }
        }

        struct DenyAllPolicy;

        impl FetchPolicy for DenyAllPolicy {
            fn decide_rpc(&self, _unresolved: &[Pubkey]) -> RpcDecision {
                RpcDecision::Deny
            }
        }

        struct RecordingObserver {
            events: Arc<Mutex<Vec<FetchEvent>>>,
        }

        impl FetchObserver for RecordingObserver {
            fn on_event(&self, event: &FetchEvent) {
                self.events.lock().unwrap().push(event.clone());
            }
        }

        let key = Pubkey::new_unique();
        let events = Arc::new(Mutex::new(Vec::new()));
        let observer = Arc::new(RecordingObserver { events: events.clone() });

        let mut fetcher = AccountFetcher::with_provider(Arc::new(NeverCalledProvider))
            .with_policy(Arc::new(DenyAllPolicy))
            .with_observer(observer);

        let mut dest = HashMap::new();
        fetcher.fetch_accounts(&[key], &mut dest).unwrap();

        let recorded = events.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], FetchEvent::RpcSkippedByPolicy { missing: vec![key] });
    }

    #[test]
    fn fetcher_emits_rpc_progress_sequence() {
        struct RecordingObserver {
            events: Arc<Mutex<Vec<FetchEvent>>>,
        }

        impl FetchObserver for RecordingObserver {
            fn on_event(&self, event: &FetchEvent) {
                self.events.lock().unwrap().push(event.clone());
            }
        }

        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let mut accounts = std::collections::HashMap::new();
        accounts.insert(key1, system_account(100));
        accounts.insert(key2, system_account(200));

        let events = Arc::new(Mutex::new(Vec::new()));
        let observer = Arc::new(RecordingObserver { events: events.clone() });
        let mut fetcher =
            AccountFetcher::with_provider(Arc::new(FakeAccountProvider::from_accounts(accounts)))
                .with_observer(observer);

        let mut dest = HashMap::new();
        fetcher.fetch_accounts(&[key1, key2], &mut dest).unwrap();

        let recorded = events.lock().unwrap();
        assert!(!recorded.is_empty());

        assert!(matches!(
            recorded.first(),
            Some(FetchEvent::RpcBatchStarted { batch_index: 0, batch_size: 2, total_requested: 2 })
        ));

        let progress_events: Vec<_> = recorded
            .iter()
            .filter_map(|event| match event {
                FetchEvent::RpcProgress { pubkey, current, total } => {
                    Some((*pubkey, *current, *total))
                }
                _ => None,
            })
            .collect();
        assert_eq!(progress_events.len(), 2);
        assert_eq!(progress_events[0].1, 1);
        assert_eq!(progress_events[1].1, 2);
        assert_eq!(progress_events[0].2, 2);
        assert_eq!(progress_events[1].2, 2);

        assert!(matches!(
            recorded.last(),
            Some(FetchEvent::RpcFinished { requested: 2, fetched: 2, missing: 0 })
        ));
    }

    #[test]
    fn fetcher_skips_litesvm_builtin_programs_without_rpc() {
        struct CountingProvider {
            call_count: Arc<AtomicUsize>,
        }

        impl RpcAccountProvider for CountingProvider {
            fn get_multiple_accounts(
                &self,
                pubkeys: &[Pubkey],
            ) -> Result<Vec<Option<AccountSharedData>>> {
                self.call_count.fetch_add(1, Ordering::SeqCst);
                Ok(pubkeys.iter().map(|_| None).collect())
            }
        }

        let call_count = Arc::new(AtomicUsize::new(0));
        let provider = CountingProvider { call_count: call_count.clone() };
        let mut fetcher = AccountFetcher::with_provider(Arc::new(provider));
        let mut dest = HashMap::new();

        let spl_token_program = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
            .expect("hard-coded pubkey should parse");
        assert!(crate::known_programs::is_litesvm_builtin_program(&spl_token_program));

        fetcher.fetch_accounts(&[spl_token_program], &mut dest).unwrap();

        assert_eq!(
            call_count.load(Ordering::SeqCst),
            0,
            "builtin program should be filtered before RPC"
        );
        assert!(dest.is_empty(), "builtin program should not be inserted into destination");
    }
}
