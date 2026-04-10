use std::path::Path;

use litesvm::LiteSVM;
use litesvm::types::TransactionResult;
use solana_account::{Account, AccountSharedData};
use solana_pubkey::Pubkey;
use solana_transaction::versioned::VersionedTransaction;

#[allow(clippy::result_large_err)] // TransactionResult uses upstream litesvm types
/// Minimal simulation backend abstraction.
///
/// This decouples executor/funding logic from a concrete SVM implementation.
/// The trait uses `AccountSharedData` throughout; the `Account` ↔ `AccountSharedData`
/// conversion is isolated to the LiteSVM implementation below.
pub trait SvmBackend {
    fn set_account(
        &mut self,
        pubkey: Pubkey,
        account: AccountSharedData,
    ) -> std::result::Result<(), String>;
    fn get_account(&self, pubkey: &Pubkey) -> Option<AccountSharedData>;
    fn add_program_from_file(
        &mut self,
        program_id: Pubkey,
        so_path: &Path,
    ) -> std::result::Result<(), String>;
    fn send_transaction(&mut self, tx: VersionedTransaction) -> TransactionResult;
    fn warp_to_slot(&mut self, slot: u64);
}

impl SvmBackend for LiteSVM {
    fn set_account(
        &mut self,
        pubkey: Pubkey,
        account: AccountSharedData,
    ) -> std::result::Result<(), String> {
        let account: Account = account.into();
        LiteSVM::set_account(self, pubkey, account).map_err(|e| e.to_string())
    }

    fn get_account(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        LiteSVM::get_account(self, pubkey).map(|a| a.into())
    }

    fn add_program_from_file(
        &mut self,
        program_id: Pubkey,
        so_path: &Path,
    ) -> std::result::Result<(), String> {
        LiteSVM::add_program_from_file(self, program_id, so_path).map_err(|e| e.to_string())
    }

    fn send_transaction(&mut self, tx: VersionedTransaction) -> TransactionResult {
        LiteSVM::send_transaction(self, tx)
    }

    fn warp_to_slot(&mut self, slot: u64) {
        LiteSVM::warp_to_slot(self, slot);
    }
}
