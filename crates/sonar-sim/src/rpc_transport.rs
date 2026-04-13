//! Shared JSON-RPC transport with retry logic.
//!
//! This is the single source of truth for HTTP + retry behaviour used by both
//! [`SolanaRpcProvider`](crate::rpc_provider::SolanaRpcProvider) inside
//! `sonar-sim` and by `RpcClient` in the `sonar` binary crate.

use std::thread;
use std::time::Duration;

use crate::error::SonarSimError;
use crate::rpc_json::JsonRpcResponse;

const RPC_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RETRIES: u32 = 5;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(2);

/// Low-level JSON-RPC transport backed by `ureq`.
///
/// Handles envelope construction, retries on 429/503, and JSON-RPC error
/// propagation.  Higher-level structs add domain-specific convenience methods.
pub struct RpcTransport {
    agent: ureq::Agent,
    rpc_url: String,
}

impl RpcTransport {
    pub fn new(rpc_url: impl Into<String>) -> Self {
        let agent =
            ureq::Agent::config_builder().timeout_global(Some(RPC_TIMEOUT)).build().new_agent();
        Self { rpc_url: rpc_url.into(), agent }
    }

    /// Send a JSON-RPC request with automatic retry on transient HTTP errors.
    pub fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T, SonarSimError> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let mut last_err = None;
        for attempt in 0..=MAX_RETRIES {
            match self.agent.post(&self.rpc_url).send_json(&body) {
                Ok(mut response) => {
                    let rpc: JsonRpcResponse<T> = response.body_mut().read_json().map_err(
                        |e| SonarSimError::Rpc { reason: format!("parse response: {e}") },
                    )?;

                    if let Some(err) = rpc.error {
                        return Err(SonarSimError::Rpc { reason: err.to_string() });
                    }
                    return rpc.result.ok_or_else(|| SonarSimError::Rpc {
                        reason: "empty result".into(),
                    });
                }
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
                    last_err =
                        Some(SonarSimError::Rpc { reason: format!("HTTP {status_code}") });
                }
                Err(e) => {
                    return Err(SonarSimError::Rpc { reason: e.to_string() });
                }
            }
        }
        Err(last_err.unwrap_or_else(|| SonarSimError::Rpc {
            reason: "RPC request failed after retries".into(),
        }))
    }
}
