use std::collections::HashMap;
use std::path::PathBuf;

use solana_account::{Account, AccountSharedData};
use solana_pubkey::Pubkey;

#[derive(Clone, Debug)]
pub enum AccountOverride {
    Program { program_id: Pubkey, so_path: PathBuf },
    Account { pubkey: Pubkey, account: Account, source_path: PathBuf },
}

impl AccountOverride {
    pub fn pubkey(&self) -> Pubkey {
        match self {
            AccountOverride::Program { program_id, .. } => *program_id,
            AccountOverride::Account { pubkey, .. } => *pubkey,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AccountDataPatch {
    pub pubkey: Pubkey,
    pub offset: usize,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct InstructionDataPatch {
    pub instruction_index: usize,
    pub offset: usize,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct InstructionAccountPatch {
    pub instruction_index: usize,
    pub account_position: usize,
    pub new_pubkey: Pubkey,
    /// Whether the new account should be writable. Default: `true`.
    pub writable: bool,
}

#[derive(Clone, Debug)]
pub struct InstructionAccountAppend {
    pub instruction_index: usize,
    pub new_pubkey: Pubkey,
    /// Whether the appended account should be writable. Default: `true`.
    pub writable: bool,
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
