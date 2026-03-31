use std::sync::Arc;
use std::thread;
use std::time::Duration;

use solana_account::AccountSharedData;
use solana_pubkey::Pubkey;

use crate::error::{Result, SonarSimError};
use crate::rpc_json::{JsonRpcResponse, RpcAccountInfo, RpcResultValue};

const HISTORICAL_RPC_METHOD: &str = "getMultipleAccountsDataBySlot";
const RPC_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RETRIES: u32 = 5;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(2);

/// Minimal abstraction over Solana RPC account-fetching operations.
///
/// Production code uses [`SolanaRpcProvider`]; tests inject a
/// [`FakeAccountProvider`] to run without network or filesystem access.
pub trait RpcAccountProvider: Send + Sync {
    fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<AccountSharedData>>>;
}

fn new_agent() -> Arc<ureq::Agent> {
    Arc::new(ureq::Agent::config_builder().timeout_global(Some(RPC_TIMEOUT)).build().new_agent())
}

/// Send a JSON-RPC request with retry logic and parse the batch account response.
fn fetch_accounts_with_retry(
    agent: &ureq::Agent,
    rpc_url: &str,
    body: &serde_json::Value,
) -> Result<Vec<Option<AccountSharedData>>> {
    let mut last_err = None;
    for attempt in 0..=MAX_RETRIES {
        let mut response = match agent.post(rpc_url).send_json(body) {
            Ok(resp) => resp,
            Err(ureq::Error::StatusCode(status_code))
                if (status_code == 429 || status_code == 503) && attempt < MAX_RETRIES =>
            {
                let delay = DEFAULT_RETRY_DELAY * (attempt + 1);
                log::warn!(
                    "RPC returned {}; retrying in {:?} (attempt {}/{})",
                    status_code,
                    delay,
                    attempt + 1,
                    MAX_RETRIES,
                );
                thread::sleep(delay);
                last_err = Some(SonarSimError::Rpc { reason: format!("HTTP {status_code}") });
                continue;
            }
            Err(e) => return Err(SonarSimError::Rpc { reason: e.to_string() }),
        };

        let rpc: JsonRpcResponse<RpcResultValue<Vec<Option<RpcAccountInfo>>>> = response
            .body_mut()
            .read_json()
            .map_err(|e| SonarSimError::Rpc { reason: format!("parse response: {e}") })?;

        if let Some(err) = rpc.error {
            return Err(SonarSimError::Rpc { reason: err.to_string() });
        }

        let result =
            rpc.result.ok_or_else(|| SonarSimError::Rpc { reason: "empty result".into() })?;

        return result
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
            .collect();
    }

    Err(last_err.unwrap_or_else(|| SonarSimError::Rpc {
        reason: "RPC request failed after retries".into(),
    }))
}

/// Production implementation backed by a lightweight `ureq` HTTP client.
pub struct SolanaRpcProvider {
    agent: Arc<ureq::Agent>,
    rpc_url: String,
}

impl SolanaRpcProvider {
    pub fn new(rpc_url: String) -> Self {
        Self { agent: new_agent(), rpc_url }
    }
}

impl RpcAccountProvider for SolanaRpcProvider {
    fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<AccountSharedData>>> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getMultipleAccounts",
            "params": [
                pubkeys.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
                {"encoding": "base64"}
            ]
        });
        fetch_accounts_with_retry(&self.agent, &self.rpc_url, &body)
    }
}

/// Production implementation that fetches historical account state at a
/// specific slot via the non-standard `getMultipleAccountsDataBySlot` RPC method.
pub struct HistoricalRpcProvider {
    agent: Arc<ureq::Agent>,
    rpc_url: String,
    slot: u64,
}

impl HistoricalRpcProvider {
    pub fn new(rpc_url: String, slot: u64) -> Self {
        Self { agent: new_agent(), rpc_url, slot }
    }
}

impl RpcAccountProvider for HistoricalRpcProvider {
    fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<AccountSharedData>>> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": HISTORICAL_RPC_METHOD,
            "params": [
                pubkeys.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
                self.slot,
                {"encoding": "base64"}
            ]
        });
        fetch_accounts_with_retry(&self.agent, &self.rpc_url, &body)
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
