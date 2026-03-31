use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::Result;
use log::debug;
use solana_account::AccountSharedData;
use solana_pubkey::Pubkey;

use sonar_sim::{AccountLoader, HistoricalRpcProvider};

use crate::core::idl_fetcher::IdlFetcher;
use crate::utils::progress::Progress;

/// Local source backed by account JSON files in a directory.
pub struct LocalDirSource {
    local_dir: PathBuf,
}

impl LocalDirSource {
    pub fn new(local_dir: PathBuf) -> Self {
        Self { local_dir }
    }
}

impl sonar_sim::AccountSource for LocalDirSource {
    fn resolve(&self, pubkeys: &[Pubkey]) -> sonar_sim::Result<HashMap<Pubkey, AccountSharedData>> {
        let mut found = HashMap::new();
        for key in pubkeys {
            let path = self.local_dir.join(format!("{key}.json"));
            if path.exists() {
                let account = crate::core::account_file::parse_account_json(&path)
                    .map_err(|e| sonar_sim::SonarSimError::Internal { reason: e.to_string() })?;
                if crate::core::account_file::is_missing_account_placeholder(&account) {
                    debug!(
                        "Ignoring placeholder account {} from local file: {}",
                        key,
                        path.display()
                    );
                    continue;
                }
                debug!("Loaded account {} from local file: {}", key, path.display());
                found.insert(*key, AccountSharedData::from(account));
            }
        }
        Ok(found)
    }
}

/// Policy that denies RPC for all unresolved accounts.
pub struct OfflinePolicy;

impl sonar_sim::FetchPolicy for OfflinePolicy {
    fn decide_rpc(&self, _unresolved: &[Pubkey]) -> sonar_sim::RpcDecision {
        sonar_sim::RpcDecision::Deny
    }
}

/// Observer that reports RPC progress in CLI progress UI.
pub struct CliProgressObserver {
    progress: Progress,
}

impl CliProgressObserver {
    pub fn new(progress: Progress) -> Self {
        Self { progress }
    }
}

impl sonar_sim::FetchObserver for CliProgressObserver {
    fn on_event(&self, event: &sonar_sim::FetchEvent) {
        if let sonar_sim::FetchEvent::RpcProgress { pubkey, current, total } = event {
            self.progress
                .set_message(format!("loading account {} ({}/{})", pubkey, current, total));
        }
    }
}

/// Observer that reports unresolved accounts when RPC is skipped by policy.
pub struct OfflineWarningObserver;

impl sonar_sim::FetchObserver for OfflineWarningObserver {
    fn on_event(&self, event: &sonar_sim::FetchEvent) {
        if let sonar_sim::FetchEvent::RpcSkippedByPolicy { missing } = event {
            let non_native: Vec<_> = missing
                .iter()
                .filter(|k| {
                    !sonar_sim::is_native_or_sysvar(k) && !sonar_sim::is_litesvm_builtin_program(k)
                })
                .collect();
            if !non_native.is_empty() {
                log::warn!(
                    "offline mode: {} account(s) not found in cache directory (treated as non-existent): [{}]",
                    non_native.len(),
                    non_native.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(", ")
                );
            }
        }
    }
}

/// Construct a unified `AccountLoader` with CLI fetch adapters.
///
/// In offline mode an empty RPC URL is accepted (a dummy URL is used since
/// the RPC layer will never be reached).
pub fn create_loader(
    rpc_url: String,
    local_dir: Option<PathBuf>,
    offline: bool,
    progress: Option<Progress>,
    history_slot: Option<u64>,
) -> Result<AccountLoader> {
    let url = if rpc_url.is_empty() && offline {
        "http://localhost:8899".to_string()
    } else if rpc_url.is_empty() {
        return Err(anyhow::anyhow!(
            "RPC URL cannot be empty (use --cache for local-only mode when cache exists)"
        ));
    } else {
        rpc_url
    };

    let mut loader = if let Some(slot) = history_slot {
        AccountLoader::with_provider(Arc::new(HistoricalRpcProvider::new(url, slot)))
    } else {
        AccountLoader::new(url)?
    };

    if let Some(dir) = local_dir {
        loader = loader.with_source(Arc::new(LocalDirSource::new(dir)));
    }

    if offline {
        loader = loader
            .with_policy(Arc::new(OfflinePolicy))
            .with_observer(Arc::new(OfflineWarningObserver));
    }

    if let Some(progress) = progress {
        loader = loader.with_observer(Arc::new(CliProgressObserver::new(progress)));
    }

    Ok(loader)
}

/// Create an `IdlFetcher` that shares the loader's RPC provider.
pub fn create_idl_fetcher(loader: &AccountLoader, progress: Option<Progress>) -> IdlFetcher {
    IdlFetcher::with_provider(loader.provider(), progress)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;
    use solana_account::{Account, ReadableAccount};
    use solana_clock::Clock;
    use solana_hash::Hash;
    use solana_keypair::Keypair;
    use solana_message::Message;
    use solana_signer::Signer;
    use solana_slot_hashes::SlotHashes;
    use solana_system_interface::instruction as system_instruction;
    use solana_sysvar_id::SysvarId;
    use solana_transaction::Transaction;
    use solana_transaction::versioned::VersionedTransaction;
    use sonar_sim::ResolvedAccounts;
    use sonar_sim::{AccountAppender, AccountSource};
    use sonar_sim::{FakeAccountProvider, RpcAccountProvider};

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

    // ------------------------------------------------------------------
    // offline mode tests
    // ------------------------------------------------------------------

    #[test]
    fn offline_loader_returns_accounts_from_provider_without_rpc() {
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
    fn offline_mode_skips_rpc_and_treats_missing_as_nonexistent() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let mut loader = AccountLoader::with_provider(Arc::new(FakeAccountProvider::empty()))
            .with_policy(Arc::new(OfflinePolicy))
            .with_observer(Arc::new(OfflineWarningObserver));

        let tx = create_transfer_tx(&payer, &recipient, 1000);
        let resolved = loader
            .load_for_transaction(&tx)
            .expect("offline should succeed even with missing accounts");

        assert!(
            !resolved.accounts.contains_key(&payer.pubkey()),
            "payer should not be found in offline mode without local dir"
        );
        assert!(
            !resolved.accounts.contains_key(&recipient),
            "recipient should not be found in offline mode without local dir"
        );
    }

    #[test]
    fn offline_mode_does_not_call_rpc_provider() {
        struct NeverCalledProvider {
            called: Arc<AtomicBool>,
        }

        impl RpcAccountProvider for NeverCalledProvider {
            fn get_multiple_accounts(
                &self,
                _pubkeys: &[Pubkey],
            ) -> sonar_sim::Result<Vec<Option<AccountSharedData>>> {
                self.called.store(true, Ordering::SeqCst);
                panic!("RPC provider should never be called in offline mode");
            }
        }

        let called = Arc::new(AtomicBool::new(false));
        let provider = NeverCalledProvider { called: called.clone() };
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();
        let tx = create_transfer_tx(&payer, &recipient, 1000);

        let mut loader = AccountLoader::with_provider(Arc::new(provider))
            .with_policy(Arc::new(OfflinePolicy))
            .with_observer(Arc::new(OfflineWarningObserver));

        let resolved =
            loader.load_for_transaction(&tx).expect("offline should succeed without RPC");

        assert!(!called.load(Ordering::SeqCst), "provider should not be called");
        assert!(resolved.accounts.is_empty());
    }

    // ------------------------------------------------------------------
    // cache / in-memory caching tests
    // ------------------------------------------------------------------

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
            ) -> sonar_sim::Result<Vec<Option<AccountSharedData>>> {
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

    // ------------------------------------------------------------------
    // local directory (FS cache) tests
    // ------------------------------------------------------------------

    #[test]
    fn local_dir_accounts_are_loaded_before_rpc() {
        let temp_dir =
            std::env::temp_dir().join(format!("sonar_test_local_dir_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        for (pubkey, lamports) in [(payer.pubkey(), 10_000_000_000u64), (recipient, 1)] {
            let json = serde_json::json!({
                "lamports": lamports,
                "data": ["", "base64"],
                "owner": "11111111111111111111111111111111",
                "executable": false,
                "rentEpoch": 0
            });
            std::fs::write(
                temp_dir.join(format!("{pubkey}.json")),
                serde_json::to_string_pretty(&json).unwrap(),
            )
            .unwrap();
        }

        let tx = create_transfer_tx(&payer, &recipient, 1000);

        let mut loader = AccountLoader::with_provider(Arc::new(FakeAccountProvider::empty()))
            .with_source(Arc::new(LocalDirSource::new(temp_dir.clone())))
            .with_policy(Arc::new(OfflinePolicy))
            .with_observer(Arc::new(OfflineWarningObserver));

        let resolved = loader.load_for_transaction(&tx).expect("should load from local dir");

        assert!(resolved.accounts.contains_key(&payer.pubkey()));
        assert!(resolved.accounts.contains_key(&recipient));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn local_dir_placeholder_accounts_are_treated_as_missing() {
        let temp_dir = std::env::temp_dir()
            .join(format!("sonar_test_local_dir_placeholder_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let missing = Pubkey::new_unique();
        let placeholder = serde_json::json!({
            "pubkey": missing.to_string(),
            "account": {
                "lamports": 0,
                "data": ["", "base64"],
                "owner": solana_sdk_ids::system_program::id().to_string(),
                "executable": false,
                "rentEpoch": 0,
                "space": 0
            }
        });
        std::fs::write(
            temp_dir.join(format!("{missing}.json")),
            serde_json::to_string_pretty(&placeholder).unwrap(),
        )
        .unwrap();

        let source = LocalDirSource::new(temp_dir.clone());
        let resolved = source.resolve(&[missing]).expect("placeholder load should succeed");

        assert!(
            resolved.is_empty(),
            "placeholder cache files should be treated as missing accounts"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // ------------------------------------------------------------------
    // append_accounts tests
    // ------------------------------------------------------------------

    #[test]
    fn append_accounts_fetches_additional_accounts() {
        let extra = Pubkey::new_unique();

        let mut accounts = std::collections::HashMap::new();
        accounts.insert(extra, system_account(42));

        let mut loader =
            AccountLoader::with_provider(Arc::new(FakeAccountProvider::from_accounts(accounts)));

        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        loader.append_accounts(&mut resolved, &[extra]).unwrap();
        assert!(resolved.accounts.contains_key(&extra));
        assert_eq!(resolved.accounts[&extra].lamports(), 42);
    }

    // ------------------------------------------------------------------
    // load_for_transactions (bundle) tests
    // ------------------------------------------------------------------

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
}
