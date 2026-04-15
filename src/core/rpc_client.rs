use std::str::FromStr;

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
    EncodedTransaction, TransactionStatus, UiTransactionEncoding, UiTransactionStatusMeta,
};

use sonar_sim::internals::rpc_json::{RpcAccountInfo, RpcResultValue};
use sonar_sim::internals::{HISTORICAL_RPC_METHOD, RpcTransport};

/// Lightweight Solana JSON-RPC client backed by [`RpcTransport`].
pub struct RpcClient {
    transport: RpcTransport,
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
        Self { transport: RpcTransport::new(rpc_url) }
    }

    /// Convenience wrapper that converts [`sonar_sim::SonarSimError`] → [`anyhow::Error`].
    fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T> {
        self.transport.call(method, params).map_err(|e| anyhow!("{e}"))
    }

    /// Fetch a single account, optionally at a historical slot.
    ///
    /// When `history_slot` is `Some`, uses the non-standard
    /// `getMultipleAccountsDataBySlot` RPC method. Otherwise uses
    /// standard `getAccountInfo`.
    pub fn get_account_maybe_historical(
        &self,
        pubkey: &Pubkey,
        history_slot: Option<u64>,
    ) -> Result<Account> {
        match history_slot {
            Some(slot) => self.get_account_at_slot(pubkey, slot),
            None => self.get_account(pubkey),
        }
    }

    fn get_account_at_slot(&self, pubkey: &Pubkey, slot: u64) -> Result<Account> {
        let result: RpcResultValue<Vec<Option<RpcAccountInfo>>> = self.call(
            HISTORICAL_RPC_METHOD,
            serde_json::json!([
                [pubkey.to_string()],
                slot,
                {"encoding": "base64"}
            ]),
        )?;
        result
            .value
            .into_iter()
            .next()
            .flatten()
            .ok_or_else(|| anyhow!("AccountNotFound: {pubkey} at slot {slot}"))?
            .into_account()
            .map_err(|e| anyhow!("{e}"))
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

    pub fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<Account>>> {
        let keys: Vec<String> = pubkeys.iter().map(|k| k.to_string()).collect();
        let result: RpcResultValue<Vec<Option<RpcAccountInfo>>> =
            self.call("getMultipleAccounts", serde_json::json!([keys, {"encoding": "base64"}]))?;
        result
            .value
            .into_iter()
            .map(|opt| opt.map(|info| info.into_account().map_err(|e| anyhow!("{e}"))).transpose())
            .collect()
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
