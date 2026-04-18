use std::sync::Arc;

use solana_account::AccountSharedData;
use solana_pubkey::Pubkey;

use crate::error::{Result, SonarSimError};
use crate::rpc_json::{RpcAccountInfo, RpcResultValue};
use crate::rpc_transport::RpcTransport;

/// Non-standard RPC method for fetching accounts at a historical slot.
pub const HISTORICAL_RPC_METHOD: &str = "getMultipleAccountsDataBySlot";

/// Parse a batch account RPC result into a vector of optional `AccountSharedData`.
fn parse_account_result(
    result: RpcResultValue<Vec<Option<RpcAccountInfo>>>,
) -> Result<Vec<Option<AccountSharedData>>> {
    result
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
        .collect()
}

/// Minimal abstraction over Solana RPC account-fetching operations.
///
/// Production code uses [`SolanaRpcProvider`]; tests inject a
/// [`FakeAccountProvider`] to run without network or filesystem access.
pub trait RpcAccountProvider: Send + Sync {
    fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<AccountSharedData>>>;
}

/// Production implementation backed by [`RpcTransport`].
///
/// When `history_slot` is `Some`, uses the non-standard
/// `getMultipleAccountsDataBySlot` RPC method to fetch account state at a
/// specific slot. Otherwise uses standard `getMultipleAccounts`.
pub struct SolanaRpcProvider {
    transport: Arc<RpcTransport>,
    history_slot: Option<u64>,
}

impl SolanaRpcProvider {
    pub fn new(rpc_url: String) -> Self {
        Self { transport: Arc::new(RpcTransport::new(rpc_url)), history_slot: None }
    }

    pub fn historical(rpc_url: String, slot: u64) -> Self {
        Self { transport: Arc::new(RpcTransport::new(rpc_url)), history_slot: Some(slot) }
    }
}

impl RpcAccountProvider for SolanaRpcProvider {
    fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<AccountSharedData>>> {
        let keys: Vec<String> = pubkeys.iter().map(|p| p.to_string()).collect();
        let (method, params) = match self.history_slot {
            Some(slot) => {
                (HISTORICAL_RPC_METHOD, serde_json::json!([keys, slot, {"encoding": "base64"}]))
            }
            None => ("getMultipleAccounts", serde_json::json!([keys, {"encoding": "base64"}])),
        };
        parse_account_result(self.transport.call(method, params)?)
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
