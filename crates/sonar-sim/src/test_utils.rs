//! Shared test helpers used across `sonar-sim` unit tests.

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
use spl_token::solana_program::pubkey::Pubkey as ProgramPubkey;
use spl_token::state::{Account as SplAccount, AccountState};

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
        mint: ProgramPubkey::new_from_array(mint.to_bytes()),
        owner: ProgramPubkey::new_from_array(token_owner.to_bytes()),
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
        owner: spl_token::ID,
        executable: false,
        rent_epoch: 0,
    })
}
