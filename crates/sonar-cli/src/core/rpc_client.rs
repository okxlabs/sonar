use std::str::FromStr;

use anyhow::{Result, anyhow};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::Deserialize;
use solana_account::Account;
use solana_commitment_config::{CommitmentConfig, CommitmentLevel};
use solana_hash::Hash;
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_transaction_status_client_types::{
    EncodedTransaction, TransactionStatus, UiTransactionEncoding, UiTransactionStatusMeta,
};

use sonar_sim::internals::RpcTransport;
use sonar_sim::internals::rpc_json::{RpcAccountInfo, RpcResultValue};
use sonar_sim::{RpcAccountProvider, SolanaRpcProvider};

/// Lightweight Solana JSON-RPC client backed by [`RpcTransport`].
pub struct RpcClient {
    transport: RpcTransport,
    historical_provider: Option<SolanaRpcProvider>,
}

#[derive(Deserialize)]
pub struct RpcResponse<T> {
    pub value: T,
}

// ---------------------------------------------------------------------------
// Config types (minimal replacements for solana-rpc-client-api)
// ---------------------------------------------------------------------------

#[derive(Default, Clone, Copy)]
pub struct SendTransactionConfig {
    pub skip_preflight: bool,
    pub preflight_commitment: Option<CommitmentConfig>,
}

/// `getLatestBlockhash` result.
#[derive(Debug, Clone, Copy)]
pub struct LatestBlockhash {
    pub blockhash: Hash,
    pub last_valid_block_height: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcLatestBlockhash {
    blockhash: String,
    last_valid_block_height: u64,
}

pub struct GetTransactionConfig {
    pub encoding: UiTransactionEncoding,
    pub commitment: CommitmentConfig,
    pub max_supported_transaction_version: Option<u8>,
}

// RPC `getTransaction` response, deserialized without `#[serde(flatten)]`.
//
// The upstream `EncodedConfirmedTransactionWithStatusMeta` uses
// `#[serde(flatten)]` which breaks untagged enum deserialization of the
// `version` field (serde bug <https://github.com/serde-rs/serde/issues/1183>).
// V0 transactions return `"version": 0` (integer) which the flattened
// deserializer misinterprets. This struct matches the flat JSON shape directly.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // slot, block_time, version are needed for deserialization only
pub struct RpcTransactionResponse {
    pub slot: u64,
    pub block_time: Option<i64>,
    pub transaction: EncodedTransaction,
    pub meta: Option<UiTransactionStatusMeta>,
    #[serde(default)]
    version: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Client implementation
// ---------------------------------------------------------------------------

impl RpcClient {
    pub fn new(rpc_url: impl Into<String>) -> Self {
        Self::new_at_slot(rpc_url, None)
    }

    pub fn new_at_slot(rpc_url: impl Into<String>, historical_slot: Option<u64>) -> Self {
        let rpc_url = rpc_url.into();
        let historical_provider = historical_slot
            .map(|slot| SolanaRpcProvider::new(rpc_url.clone()).with_historical_slot(slot));
        Self { transport: RpcTransport::new(rpc_url), historical_provider }
    }

    /// Convenience wrapper that converts [`sonar_sim::SonarSimError`] → [`anyhow::Error`].
    fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T> {
        self.transport.call(method, params).map_err(|e| anyhow!("{e}"))
    }

    pub fn get_account(&self, pubkey: &Pubkey) -> Result<Account> {
        if self.historical_provider.is_some() {
            return self
                .get_multiple_accounts(&[*pubkey])?
                .into_iter()
                .next()
                .flatten()
                .ok_or_else(|| anyhow!("AccountNotFound: {pubkey}"));
        }
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

    pub fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<Account>>> {
        if let Some(provider) = &self.historical_provider {
            return Ok(provider
                .get_multiple_accounts(pubkeys)?
                .into_iter()
                .map(|account| account.map(Account::from))
                .collect());
        }
        let keys: Vec<String> = pubkeys.iter().map(|k| k.to_string()).collect();
        let result: RpcResultValue<Vec<Option<RpcAccountInfo>>> =
            self.call("getMultipleAccounts", serde_json::json!([keys, {"encoding": "base64"}]))?;
        result
            .value
            .into_iter()
            .map(|opt| opt.map(|info| info.into_account().map_err(|e| anyhow!("{e}"))).transpose())
            .collect()
    }

    pub fn get_account_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment: CommitmentConfig,
    ) -> Result<RpcResponse<Option<Account>>> {
        if self.historical_provider.is_some() {
            let value = self.get_multiple_accounts(&[*pubkey])?.into_iter().next().flatten();
            return Ok(RpcResponse { value });
        }
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

    /// Submit a transaction's wire bytes.
    ///
    /// The caller owns serialization because the wire layout is format-specific:
    /// legacy/v0 use the `wincode` (frozen bincode 1.3) layout of
    /// [`VersionedTransaction`](solana_transaction::versioned::VersionedTransaction),
    /// while v1 writes its signature array as a trailing fixed-length array. The
    /// base64 the RPC asks for is an encoding detail of this call, not of the caller.
    pub fn send_transaction_with_config(
        &self,
        tx_bytes: &[u8],
        config: SendTransactionConfig,
    ) -> Result<Signature> {
        let mut opts = serde_json::json!({
            "encoding": "base64",
            "skipPreflight": config.skip_preflight,
        });
        if let Some(commitment) = config.preflight_commitment {
            opts["preflightCommitment"] =
                serde_json::Value::String(commitment_str(commitment).into());
        }
        let tx_base64 = BASE64_STANDARD.encode(tx_bytes);
        let sig_str: String = self.call("sendTransaction", serde_json::json!([tx_base64, opts]))?;
        Signature::from_str(&sig_str).map_err(|e| anyhow!("Invalid signature: {e}"))
    }

    /// Latest blockhash and the last block height at which it is still valid.
    pub fn get_latest_blockhash(&self, commitment: CommitmentConfig) -> Result<LatestBlockhash> {
        let result: RpcResultValue<RpcLatestBlockhash> = self.call(
            "getLatestBlockhash",
            serde_json::json!([{"commitment": commitment_str(commitment)}]),
        )?;
        let blockhash = Hash::from_str(&result.value.blockhash)
            .map_err(|e| anyhow!("Invalid blockhash from RPC: {e}"))?;
        Ok(LatestBlockhash {
            blockhash,
            last_valid_block_height: result.value.last_valid_block_height,
        })
    }

    pub fn get_block_height(&self, commitment: CommitmentConfig) -> Result<u64> {
        self.call("getBlockHeight", serde_json::json!([{"commitment": commitment_str(commitment)}]))
    }

    pub fn get_minimum_balance_for_rent_exemption(&self, data_len: usize) -> Result<u64> {
        self.call("getMinimumBalanceForRentExemption", serde_json::json!([data_len]))
    }

    pub fn get_balance(&self, pubkey: &Pubkey, commitment: CommitmentConfig) -> Result<u64> {
        let result: RpcResultValue<u64> = self.call(
            "getBalance",
            serde_json::json!([pubkey.to_string(), {"commitment": commitment_str(commitment)}]),
        )?;
        Ok(result.value)
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
    ) -> Result<RpcTransactionResponse> {
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
