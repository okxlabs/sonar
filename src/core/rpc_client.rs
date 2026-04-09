use std::str::FromStr;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;
use solana_account::Account;
use solana_commitment_config::{CommitmentConfig, CommitmentLevel};
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction_status_client_types::{
    EncodedConfirmedTransactionWithStatusMeta, TransactionStatus, UiTransactionEncoding,
};

use sonar_sim::internals::rpc_json::{JsonRpcResponse, RpcAccountInfo, RpcResultValue};

/// Lightweight Solana JSON-RPC client backed by `ureq`.
pub struct RpcClient {
    agent: ureq::Agent,
    rpc_url: String,
}

#[derive(Deserialize)]
pub struct RpcResponse<T> {
    pub value: T,
}

// ---------------------------------------------------------------------------
// Config types (minimal replacements for solana-rpc-client-api)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct SendTransactionConfig {
    pub skip_preflight: bool,
    pub preflight_commitment: Option<CommitmentConfig>,
}

pub struct GetTransactionConfig {
    pub encoding: UiTransactionEncoding,
    pub commitment: CommitmentConfig,
    pub max_supported_transaction_version: Option<u8>,
}

// ---------------------------------------------------------------------------
// Client implementation
// ---------------------------------------------------------------------------

const RPC_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RETRIES: u32 = 5;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(2);

impl RpcClient {
    pub fn new(rpc_url: impl Into<String>) -> Self {
        let agent =
            ureq::Agent::config_builder().timeout_global(Some(RPC_TIMEOUT)).build().new_agent();
        Self { agent, rpc_url: rpc_url.into() }
    }

    fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T> {
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
                    let rpc: JsonRpcResponse<T> = response
                        .body_mut()
                        .read_json()
                        .map_err(|e| anyhow!("Failed to parse RPC response: {e}"))?;

                    if let Some(err) = rpc.error {
                        return Err(anyhow!("{err}"));
                    }
                    return rpc.result.ok_or_else(|| anyhow!("RPC returned empty result"));
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
                    last_err = Some(anyhow!("RPC returned HTTP {status_code}"));
                }
                Err(e) => return Err(anyhow!("RPC request failed: {e}")),
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow!("RPC request failed after retries")))
    }

    pub fn get_account(&self, pubkey: &Pubkey) -> Result<Account> {
        let result: RpcResultValue<Option<RpcAccountInfo>> = self.call(
            "getAccountInfo",
            serde_json::json!([pubkey.to_string(), {"encoding": "base64"}]),
        )?;
        result
            .value
            .ok_or_else(|| anyhow!("AccountNotFound: {pubkey}"))?
            .into_account()
            .map_err(|e| anyhow!("{e}"))
    }

    pub fn get_account_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment: CommitmentConfig,
    ) -> Result<RpcResponse<Option<Account>>> {
        let result: RpcResultValue<Option<RpcAccountInfo>> = self.call(
            "getAccountInfo",
            serde_json::json!([
                pubkey.to_string(),
                {"encoding": "base64", "commitment": commitment_str(commitment)}
            ]),
        )?;
        let value =
            result.value.map(|info| info.into_account().map_err(|e| anyhow!("{e}"))).transpose()?;
        Ok(RpcResponse { value })
    }

    pub fn send_transaction_with_config(
        &self,
        transaction: &VersionedTransaction,
        config: SendTransactionConfig,
    ) -> Result<Signature> {
        let tx_bytes =
            bincode::serialize(transaction).context("Failed to serialize transaction")?;
        let mut opts = serde_json::json!({
            "encoding": "base64",
            "skipPreflight": config.skip_preflight,
        });
        if let Some(commitment) = config.preflight_commitment {
            opts["preflightCommitment"] =
                serde_json::Value::String(commitment_str(commitment).into());
        }
        let sig_str: String =
            self.call("sendTransaction", serde_json::json!([BASE64.encode(&tx_bytes), opts]))?;
        Signature::from_str(&sig_str).map_err(|e| anyhow!("Invalid signature: {e}"))
    }

    pub fn get_signature_statuses(
        &self,
        signatures: &[Signature],
    ) -> Result<RpcResponse<Vec<Option<TransactionStatus>>>> {
        self.call(
            "getSignatureStatuses",
            serde_json::json!([signatures.iter().map(|s| s.to_string()).collect::<Vec<_>>()]),
        )
    }

    pub fn get_transaction_with_config(
        &self,
        signature: &Signature,
        config: GetTransactionConfig,
    ) -> Result<EncodedConfirmedTransactionWithStatusMeta> {
        self.call(
            "getTransaction",
            serde_json::json!([
                signature.to_string(),
                {
                    "encoding": config.encoding,
                    "commitment": commitment_str(config.commitment),
                    "maxSupportedTransactionVersion": config.max_supported_transaction_version,
                }
            ]),
        )
    }
}

fn commitment_str(config: CommitmentConfig) -> &'static str {
    match config.commitment {
        CommitmentLevel::Processed => "processed",
        CommitmentLevel::Confirmed => "confirmed",
        CommitmentLevel::Finalized => "finalized",
    }
}
