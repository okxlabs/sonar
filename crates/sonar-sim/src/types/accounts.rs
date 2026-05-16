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

/// A mutation to apply to an instruction's account list.
///
/// Operations apply in the order they appear in the mutation list. Positions
/// are interpreted against the instruction's state at the time the op runs,
/// not the original list. To express positions relative to the pre-mutation
/// list, specify ops in descending position order.
///
/// `account_position` is 0-based at this layer; CLI surfaces convert from
/// 1-based.
#[derive(Clone, Debug)]
pub enum InstructionAccountOp {
    /// Replace the account at `account_position` with `new_pubkey`.
    Patch { instruction_index: usize, account_position: usize, new_pubkey: Pubkey, writable: bool },
    /// Insert `new_pubkey` at `account_position`. Existing accounts at and
    /// after this position shift right by one. `account_position` may equal
    /// the current account count to insert at the end.
    Insert { instruction_index: usize, account_position: usize, new_pubkey: Pubkey, writable: bool },
    /// Remove the account reference at `account_position`. Subsequent accounts
    /// shift left by one. The static `account_keys` table is left untouched:
    /// the underlying key may now be unreferenced but remains loadable by the
    /// message (matching how `Patch` leaves the replaced key in place).
    Remove { instruction_index: usize, account_position: usize },
}

impl InstructionAccountOp {
    pub fn instruction_index(&self) -> usize {
        match self {
            Self::Patch { instruction_index, .. }
            | Self::Insert { instruction_index, .. }
            | Self::Remove { instruction_index, .. } => *instruction_index,
        }
    }

    pub fn account_position(&self) -> usize {
        match self {
            Self::Patch { account_position, .. }
            | Self::Insert { account_position, .. }
            | Self::Remove { account_position, .. } => *account_position,
        }
    }
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
