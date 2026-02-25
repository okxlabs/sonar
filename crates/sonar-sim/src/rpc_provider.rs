use std::sync::Arc;

use solana_account::AccountSharedData;
use solana_pubkey::Pubkey;

use crate::error::{Result, SonarSimError};

/// Minimal abstraction over Solana RPC account-fetching operations.
///
/// Production code uses [`SolanaRpcProvider`]; tests inject a
/// [`FakeAccountProvider`] to run without network or filesystem access.
pub trait RpcAccountProvider: Send + Sync {
    fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<AccountSharedData>>>;
}

/// Production implementation backed by `solana_client::RpcClient`.
pub struct SolanaRpcProvider {
    client: Arc<solana_client::rpc_client::RpcClient>,
}

impl SolanaRpcProvider {
    pub fn new(rpc_url: String) -> Self {
        Self { client: Arc::new(solana_client::rpc_client::RpcClient::new(rpc_url)) }
    }
}

impl RpcAccountProvider for SolanaRpcProvider {
    fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<AccountSharedData>>> {
        let accounts = self
            .client
            .get_multiple_accounts(pubkeys)
            .map_err(|e| SonarSimError::Rpc { message: e.to_string() })?;
        Ok(accounts.into_iter().map(|opt| opt.map(AccountSharedData::from)).collect())
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
