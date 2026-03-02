use std::collections::HashMap;
use std::path::PathBuf;

use solana_account::{Account, AccountSharedData};
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
pub struct AccountDataPatch {
    pub pubkey: Pubkey,
    pub offset: usize,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ResolvedAccounts {
    pub accounts: HashMap<Pubkey, AccountSharedData>,
    pub lookups: Vec<ResolvedLookup>,
}

impl ResolvedAccounts {
    pub fn lookup_details(&self) -> &[ResolvedLookup] {
        &self.lookups
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedLookup {
    pub account_key: Pubkey,
    pub writable_indexes: Vec<u8>,
    pub readonly_indexes: Vec<u8>,
    pub writable_addresses: Vec<Pubkey>,
    pub readonly_addresses: Vec<Pubkey>,
}
