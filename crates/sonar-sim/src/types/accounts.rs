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

/// A whole-instruction mutation: insert a new instruction, or remove an
/// existing one.
///
/// Operations apply in the order they appear in the mutation list. Indices and
/// positions are 0-based at this layer; CLI surfaces convert from 1-based.
///
/// Whole-instruction ops are intended to run as a restructuring phase
/// *before* account-level ([`InstructionAccountOp`]) and data
/// ([`InstructionDataPatch`]) mutations, so that the latter can target the
/// post-restructure instruction list.
#[derive(Clone, Debug)]
pub enum InstructionOp {
    /// Remove the instruction at `index`. Subsequent instructions shift left
    /// by one. Does not garbage-collect account keys from the message —
    /// unreferenced keys remain loadable (matching account-level `Remove`).
    Remove { index: usize },
    /// Insert `instruction` at `position` (0-based). Existing instructions at
    /// and after `position` shift right by one. `position` may equal the
    /// current instruction count to append.
    ///
    /// The instruction's accounts are merged into the message's account table.
    /// New non-signer accounts are appended (writable keys before the
    /// read-only section; read-only keys at the end) and new signer accounts
    /// are inserted into the signer section with a placeholder signature.
    /// An account that already appears as a non-signer cannot be promoted to
    /// signer in the same op.
    Insert { position: usize, instruction: solana_instruction::Instruction },
}

impl InstructionOp {
    /// Returns the insertion position for `Insert`, or `None` for `Remove`.
    pub fn position(&self) -> Option<usize> {
        match self {
            Self::Insert { position, .. } => Some(*position),
            Self::Remove { .. } => None,
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
