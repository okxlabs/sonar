use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;

use solana_account::{Account, AccountSharedData};
use solana_clock::Clock;
use solana_epoch_schedule::EpochSchedule;
use solana_pubkey::Pubkey;
use solana_slot_hashes::SlotHashes;
use solana_sysvar_id::SysvarId;

use crate::error::{Result, SonarSimError};
use crate::rpc_json::{RpcAccountInfo, RpcResultValue};
use crate::rpc_transport::RpcTransport;

/// Minimal abstraction over Solana RPC account-fetching operations.
///
/// Production code uses [`SolanaRpcProvider`]; tests inject a
/// [`FakeAccountProvider`] to run without network or filesystem access.
pub trait RpcAccountProvider: Send + Sync {
    fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<AccountSharedData>>>;

    /// Report each resolved account (including missing accounts). Providers
    /// that fetch accounts one at a time can report before the batch completes.
    fn get_multiple_accounts_with_progress(
        &self,
        pubkeys: &[Pubkey],
        on_account: &(dyn Fn(&Pubkey) + Sync),
    ) -> Result<Vec<Option<AccountSharedData>>> {
        let accounts = self.get_multiple_accounts(pubkeys)?;
        if accounts.len() == pubkeys.len() {
            for pubkey in pubkeys {
                on_account(pubkey);
            }
        }
        Ok(accounts)
    }
}

/// Production implementation backed by [`RpcTransport`].
pub struct SolanaRpcProvider {
    transport: RpcTransport,
    historical_slot: Option<u64>,
    simulation_sysvars: bool,
}

impl SolanaRpcProvider {
    pub fn new(rpc_url: String) -> Self {
        Self {
            transport: RpcTransport::new(rpc_url),
            historical_slot: None,
            simulation_sysvars: false,
        }
    }

    fn historical_clock(&self, slot: u64) -> Result<AccountSharedData> {
        let schedule: EpochSchedule =
            self.transport.call("getEpochSchedule", serde_json::json!([]))?;
        let epoch = schedule.get_epoch(slot);
        let block_time = |at_slot| -> Result<i64> {
            let time: Option<i64> = self
                .transport
                .call("getBlockTime", serde_json::json!([at_slot]))
                .map_err(|e| SonarSimError::Rpc {
                    reason: format!(
                        "Block time unavailable for historical Clock at slot {at_slot}: {e}"
                    ),
                })?;
            time.ok_or_else(|| SonarSimError::Rpc {
                reason: format!("Block time unavailable for historical Clock at slot {at_slot}"),
            })
        };
        let clock = Clock {
            slot,
            epoch,
            leader_schedule_epoch: schedule.get_leader_schedule_epoch(slot),
            epoch_start_timestamp: block_time(schedule.get_first_slot_in_epoch(epoch))?,
            unix_timestamp: block_time(slot)?,
        };
        Ok(AccountSharedData::from(Account {
            lamports: 1,
            data: bincode::serialize(&clock)?,
            owner: solana_sdk_ids::sysvar::id(),
            executable: false,
            rent_epoch: 0,
        }))
    }

    fn historical_account(&self, slot: u64, pubkey: &Pubkey) -> Result<Option<AccountSharedData>> {
        if self.simulation_sysvars && *pubkey == Clock::id() {
            return self.historical_clock(slot).map(Some);
        }
        if self.simulation_sysvars && *pubkey == SlotHashes::id() {
            log::warn!("No historical SlotHashes; using VM defaults.");
            return Ok(None);
        }
        let result: RpcResultValue<Option<RpcAccountInfo>> = self
            .transport
            .call(
                "getHistoricalAccountInfo",
                serde_json::json!([
                    pubkey.to_string(),
                    {"slot": slot},
                    {"encoding": "base64"}
                ]),
            )
            .or_else(|error| {
                // Helius reports this restriction by account owner. Do
                // not treat unrelated RPC failures or null as unsupported.
                if matches!(&error, SonarSimError::Rpc { reason }
                    if reason.contains("account history is not tracked"))
                {
                    log::warn!("No history for {pubkey} at slot {slot}; using current state.");
                    self.transport
                        .call(
                            "getAccountInfo",
                            serde_json::json!([pubkey.to_string(), {"encoding": "base64"}]),
                        )
                        .map_err(|e| SonarSimError::Rpc {
                            reason: format!("current-state fallback for account {pubkey}: {e}"),
                        })
                } else {
                    Err(error)
                }
            })
            .map_err(|e| {
                e.with_context(|e| SonarSimError::Rpc {
                    reason: format!("historical account {pubkey} at slot {slot}: {e}"),
                })
            })?;
        result
            .value
            .map(|info| {
                info.into_account()
                    .map(AccountSharedData::from)
                    .map_err(|reason| SonarSimError::Rpc { reason })
            })
            .transpose()
    }

    /// Read account state at the end of `slot` using Helius Historical Accounts.
    ///
    /// Use `with_simulation_sysvars` to reconstruct Clock and keep VM SlotHashes
    /// when preparing a simulation. Account inspection uses current-state fallback.
    /// Accounts whose history is explicitly not tracked use current state with
    /// a warning. Other RPC failures are returned to the caller.
    pub fn with_historical_slot(mut self, slot: u64) -> Self {
        self.historical_slot = Some(slot);
        self.transport.enable_historical_limits();
        log::debug!(
            "Historical RPC limits: {} concurrent requests, 80 requests/s (process-wide)",
            crate::historical_rpc::concurrency()
        );
        self
    }

    /// Use simulation-specific Clock reconstruction and VM SlotHashes defaults.
    pub fn with_simulation_sysvars(mut self) -> Self {
        self.simulation_sysvars = true;
        self
    }
}

impl RpcAccountProvider for SolanaRpcProvider {
    fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<AccountSharedData>>> {
        self.get_multiple_accounts_with_progress(pubkeys, &|_| {})
    }

    fn get_multiple_accounts_with_progress(
        &self,
        pubkeys: &[Pubkey],
        on_account: &(dyn Fn(&Pubkey) + Sync),
    ) -> Result<Vec<Option<AccountSharedData>>> {
        if let Some(slot) = self.historical_slot {
            let workers = crate::historical_rpc::concurrency().min(pubkeys.len());
            let pending = Mutex::new(pubkeys.iter().enumerate());
            let failed = AtomicBool::new(false);
            let mut results = thread::scope(|scope| {
                let handles: Vec<_> = (0..workers)
                    .map(|_| {
                        scope.spawn(|| {
                            let mut completed = Vec::new();
                            while !failed.load(Ordering::Relaxed) {
                                let Some((index, pubkey)) = pending.lock().unwrap().next() else {
                                    break;
                                };
                                let result = self.historical_account(slot, pubkey);
                                if result.is_ok() {
                                    on_account(pubkey);
                                } else {
                                    failed.store(true, Ordering::Relaxed);
                                }
                                completed.push((index, result));
                            }
                            completed
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .flat_map(|handle| handle.join().expect("historical account worker panicked"))
                    .collect::<Vec<_>>()
            });
            // Requests complete out of order, but callers still receive results
            // aligned with the input pubkeys, including missing accounts.
            results.sort_unstable_by_key(|(index, _)| *index);
            return results.into_iter().map(|(_, result)| result).collect();
        }
        let keys: Vec<String> = pubkeys.iter().map(|p| p.to_string()).collect();
        let result: RpcResultValue<Vec<Option<RpcAccountInfo>>> = self
            .transport
            .call("getMultipleAccounts", serde_json::json!([keys, {"encoding": "base64"}]))?;

        let accounts = result
            .value
            .into_iter()
            .map(|opt| {
                opt.map(|info| {
                    info.into_account()
                        .map(AccountSharedData::from)
                        .map_err(|e| SonarSimError::Rpc { reason: e })
                })
                .transpose()
            })
            .collect::<Result<Vec<_>>>()?;
        if accounts.len() == pubkeys.len() {
            for pubkey in pubkeys {
                on_account(pubkey);
            }
        }
        Ok(accounts)
    }
}

/// In-memory fake provider for unit tests.
///
/// Returns cloned accounts from an internal map; keys not present
/// in the map yield `None` (simulating non-existent on-chain accounts).
pub struct FakeAccountProvider {
    accounts: std::collections::HashMap<Pubkey, AccountSharedData>,
}

impl FakeAccountProvider {
    pub fn new(accounts: std::collections::HashMap<Pubkey, AccountSharedData>) -> Self {
        Self { accounts }
    }

    /// Convenience constructor accepting `Account` values (auto-converts to `AccountSharedData`).
    pub fn from_accounts(
        accounts: std::collections::HashMap<Pubkey, solana_account::Account>,
    ) -> Self {
        Self {
            accounts: accounts.into_iter().map(|(k, v)| (k, AccountSharedData::from(v))).collect(),
        }
    }

    pub fn empty() -> Self {
        Self { accounts: std::collections::HashMap::new() }
    }
}

impl RpcAccountProvider for FakeAccountProvider {
    fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<AccountSharedData>>> {
        Ok(pubkeys.iter().map(|key| self.accounts.get(key).cloned()).collect())
    }
}
