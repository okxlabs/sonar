use std::path::PathBuf;

use solana_account::Account;
use solana_pubkey::Pubkey;

#[derive(Clone, Debug)]
pub enum AccountReplacement {
    Program { program_id: Pubkey, so_path: PathBuf },
    Account { pubkey: Pubkey, account: Account, source_path: PathBuf },
}

impl AccountReplacement {
    pub fn pubkey(&self) -> Pubkey {
        match self {
            AccountReplacement::Program { program_id, .. } => *program_id,
            AccountReplacement::Account { pubkey, .. } => *pubkey,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SolFunding {
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
