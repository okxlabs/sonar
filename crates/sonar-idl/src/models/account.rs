use serde::{Deserialize, Serialize};

/// An account item in an instruction's account list.
///
/// This can be either a single account or a nested group of accounts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IdlAccountItem {
    Account(IdlAccount),
    Accounts(IdlAccounts),
}

/// A single account reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlAccount {
    pub name: String,
    #[serde(default, alias = "isMut")]
    pub writable: bool,
    #[serde(default, alias = "isSigner")]
    pub signer: bool,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub address: Option<String>,
}

/// A nested group of accounts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlAccounts {
    pub name: String,
    pub accounts: Vec<IdlAccountItem>,
}
