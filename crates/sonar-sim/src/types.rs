use std::path::PathBuf;

use anyhow::Result;
use solana_account::Account;
use solana_pubkey::Pubkey;

/// Minimal abstraction for on-demand account loading.
///
/// `funding` and other subsystems depend on this trait instead of the
/// concrete `AccountLoader`, allowing test doubles and alternative
/// data sources without an RPC connection.
pub trait AccountAppender {
    fn append_accounts(&self, resolved: &mut ResolvedAccounts, pubkeys: &[Pubkey]) -> Result<()>;
}

// ── From core/types.rs ──

#[derive(Clone, Debug)]
pub enum Replacement {
    Program { program_id: Pubkey, so_path: PathBuf },
    Account { pubkey: Pubkey, account: Account, source_path: PathBuf },
}

impl Replacement {
    pub fn pubkey(&self) -> Pubkey {
        match self {
            Replacement::Program { program_id, .. } => *program_id,
            Replacement::Account { pubkey, .. } => *pubkey,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Funding {
    pub pubkey: Pubkey,
    pub amount_lamports: u64,
}

/// How the user specified the token amount on the CLI.
#[derive(Clone, Debug)]
pub enum TokenAmount {
    /// Raw u64 value — used when the input has no decimal point (e.g. `1500000`).
    Raw(u64),
    /// Human-readable decimal — will be converted using the mint's `decimals` (e.g. `1.5`).
    Decimal(f64),
}

#[derive(Clone, Debug)]
pub struct TokenFunding {
    pub account: Pubkey,
    pub mint: Option<Pubkey>,
    pub amount: TokenAmount,
}

#[derive(Clone, Debug)]
pub struct AccountDataPatch {
    pub pubkey: Pubkey,
    pub offset: usize,
    pub data: Vec<u8>,
}

// ── From core/account_loader.rs ──

#[derive(Debug, Clone)]
pub struct ResolvedAccounts {
    pub accounts: std::collections::HashMap<Pubkey, Account>,
    pub lookups: Vec<ResolvedLookup>,
}

#[derive(Debug, Clone)]
pub struct ResolvedLookup {
    pub account_key: Pubkey,
    pub writable_indexes: Vec<u8>,
    pub readonly_indexes: Vec<u8>,
    pub writable_addresses: Vec<Pubkey>,
    pub readonly_addresses: Vec<Pubkey>,
}

// ── From core/funding/mod.rs ──

#[derive(Clone, Debug)]
pub struct PreparedTokenFunding {
    pub account: Pubkey,
    pub mint: Pubkey,
    pub decimals: u8,
    pub amount_raw: u64,
    pub ui_amount: f64,
}
