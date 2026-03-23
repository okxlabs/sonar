//! Shared JSON-RPC 2.0 serde types for Solana RPC communication.

use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;
use solana_account::Account;
use solana_pubkey::Pubkey;

#[derive(Deserialize)]
pub struct JsonRpcResponse<T> {
    pub result: Option<T>,
    pub error: Option<JsonRpcError>,
}

#[derive(Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Deserialize)]
pub struct RpcResultValue<T> {
    pub value: T,
}

#[derive(Deserialize)]
pub struct RpcAccountInfo {
    pub lamports: u64,
    pub data: (String, String),
    pub owner: String,
    pub executable: bool,
    #[serde(rename = "rentEpoch")]
    pub rent_epoch: u64,
}

impl RpcAccountInfo {
    pub fn into_account(self) -> Result<Account, String> {
        let data = BASE64.decode(&self.data.0).map_err(|e| format!("base64 decode: {e}"))?;
        let owner = Pubkey::from_str(&self.owner).map_err(|e| format!("owner pubkey: {e}"))?;
        Ok(Account {
            lamports: self.lamports,
            data,
            owner,
            executable: self.executable,
            rent_epoch: self.rent_epoch,
        })
    }
}
