//! Shared test helpers used across `sonar-sim` unit tests.

use std::collections::HashMap;
use std::path::Path;

use litesvm::types::{TransactionMetadata, TransactionResult};
use solana_account::{Account, AccountSharedData};
use solana_hash::Hash;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::instruction as system_instruction;
use solana_transaction::Transaction;
use solana_transaction::versioned::VersionedTransaction;
use spl_token::solana_program::program_option::COption;
use spl_token::solana_program::program_pack::Pack;
use spl_token::state::{Account as SplAccount, AccountState};

use crate::svm_backend::SvmBackend;

pub(crate) fn system_account(lamports: u64) -> Account {
    Account {
        lamports,
        data: vec![],
        owner: solana_sdk_ids::system_program::id(),
        executable: false,
        rent_epoch: 0,
    }
}

pub(crate) fn create_transfer_tx(
    payer: &Keypair,
    recipient: &Pubkey,
    lamports: u64,
) -> VersionedTransaction {
    let blockhash = Hash::new_unique();
    let ix = system_instruction::transfer(&payer.pubkey(), recipient, lamports);
    let message = Message::new(&[ix], Some(&payer.pubkey()));
    VersionedTransaction::from(Transaction::new(&[payer], message, blockhash))
}

pub(crate) fn make_token_account_data(mint: &Pubkey, token_owner: &Pubkey, amount: u64) -> Vec<u8> {
    let state = SplAccount {
        mint: crate::token_decode::to_program_pubkey(mint),
        owner: crate::token_decode::to_program_pubkey(token_owner),
        amount,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    };
    let mut data = vec![0u8; SplAccount::LEN];
    SplAccount::pack(state, &mut data).expect("pack test token account");
    data
}

pub(crate) fn make_token_account_shared(mint: &Pubkey) -> AccountSharedData {
    let token_owner = Pubkey::new_unique();
    make_token_account_shared_with(mint, &token_owner, 0)
}

pub(crate) fn make_token_account_shared_with(
    mint: &Pubkey,
    token_owner: &Pubkey,
    amount: u64,
) -> AccountSharedData {
    let data = make_token_account_data(mint, token_owner, amount);
    AccountSharedData::from(Account {
        lamports: 1,
        data,
        owner: crate::token_decode::to_pubkey(&spl_token::ID),
        executable: false,
        rent_epoch: 0,
    })
}

/// In-memory SVM backend for testing executor pipeline functions
/// without spinning up a real LiteSVM instance.
pub(crate) struct MockSvm {
    pub accounts: HashMap<Pubkey, AccountSharedData>,
    pub slot: u64,
    /// When set, `set_account` returns this error string.
    pub fail_set_account: Option<String>,
    /// Records program IDs loaded via `add_program_from_file`.
    pub loaded_programs: Vec<Pubkey>,
}

impl MockSvm {
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            slot: 0,
            fail_set_account: None,
            loaded_programs: Vec::new(),
        }
    }

    pub fn with_account(mut self, pubkey: Pubkey, account: Account) -> Self {
        self.accounts.insert(pubkey, AccountSharedData::from(account));
        self
    }
}

impl SvmBackend for MockSvm {
    fn set_account(
        &mut self,
        pubkey: Pubkey,
        account: AccountSharedData,
    ) -> std::result::Result<(), String> {
        if let Some(ref msg) = self.fail_set_account {
            return Err(msg.clone());
        }
        self.accounts.insert(pubkey, account);
        Ok(())
    }

    fn get_account(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        self.accounts.get(pubkey).cloned()
    }

    fn add_program_from_file(
        &mut self,
        program_id: Pubkey,
        _so_path: &Path,
    ) -> std::result::Result<(), String> {
        self.loaded_programs.push(program_id);
        Ok(())
    }

    fn send_transaction(&mut self, _tx: VersionedTransaction) -> TransactionResult {
        Ok(TransactionMetadata::default())
    }

    fn warp_to_slot(&mut self, slot: u64) {
        self.slot = slot;
    }
}
