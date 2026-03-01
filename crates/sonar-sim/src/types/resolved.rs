use std::collections::HashMap;

use solana_account::AccountSharedData;
use solana_message::inner_instruction::InnerInstructionsList;
use solana_pubkey::Pubkey;

use crate::token_decode::TokenProgramKind;

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

#[derive(Clone, Debug)]
pub struct PreparedTokenFunding {
    pub account: Pubkey,
    pub mint: Pubkey,
    pub decimals: u8,
    pub amount_raw: u64,
    pub ui_amount: f64,
    pub program_kind: TokenProgramKind,
}

/// Backend-agnostic return data from a simulated transaction.
///
/// Wraps the program id that produced the return data and the raw bytes.
/// This is sonar-sim's own type so the public API does not depend on any
/// particular SVM backend crate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReturnData {
    pub program_id: Pubkey,
    pub data: Vec<u8>,
}

/// Backend-agnostic metadata produced by a single transaction simulation.
///
/// Upper layers should use this instead of any backend-specific metadata
/// type (e.g. `litesvm::types::TransactionMetadata`). The executor's
/// adapter layer converts the backend-native result into this struct.
#[derive(Debug, Clone, Default)]
pub struct SimulationMetadata {
    pub logs: Vec<String>,
    pub inner_instructions: InnerInstructionsList,
    pub compute_units_consumed: u64,
    pub return_data: ReturnData,
}
