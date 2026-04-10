use solana_account::{Account, AccountSharedData};
use solana_pubkey::Pubkey;
use solana_rent::Rent;
use spl_token::solana_program::program_option::COption;
use spl_token::solana_program::program_pack::Pack;
use spl_token::state::{Account as SplAccount, AccountState};

use crate::error::{Result, SonarSimError};
use crate::token_decode::{TokenProgramKind, legacy_program_id, to_program_pubkey};
use crate::types::PreparedTokenFunding;

pub(super) fn build_token_account(
    account_pubkey: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    rent: &Rent,
) -> Result<AccountSharedData> {
    let mut data = vec![0u8; SplAccount::LEN];
    let state = SplAccount {
        mint: to_program_pubkey(mint),
        owner: to_program_pubkey(owner),
        amount: 0,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    };
    SplAccount::pack(state, &mut data).map_err(|err| SonarSimError::Token {
        account: Some(*account_pubkey),
        reason: format!("Failed to pack new SPL token account: {err}"),
    })?;
    Ok(AccountSharedData::from(Account {
        lamports: rent.minimum_balance(SplAccount::LEN),
        data,
        owner: legacy_program_id(),
        executable: false,
        rent_epoch: 0,
    }))
}

pub(super) fn update_token_balance_in_account(
    account: &mut AccountSharedData,
    account_pubkey: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    amount_raw: u64,
    decimals: u8,
) -> Result<PreparedTokenFunding> {
    super::common::update_token_amount_account::<spl_token::state::Account>(
        account,
        account_pubkey,
        mint,
        owner,
        amount_raw,
        decimals,
        TokenProgramKind::Legacy,
    )
}

#[cfg(test)]
mod tests {
    use solana_account::ReadableAccount;
    use solana_pubkey::Pubkey;
    use solana_rent::Rent;
    use spl_token::solana_program::program_pack::Pack;
    use spl_token::state::Account as SplAccount;

    use crate::token_decode::token2022_program_id;

    use super::*;

    #[test]
    fn create_initializes_valid_account() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let rent = Rent::default();
        let account = build_token_account(&token, &mint, &owner, &rent).unwrap();
        assert_eq!(*account.owner(), legacy_program_id());
        assert_eq!(account.lamports(), rent.minimum_balance(SplAccount::LEN));

        let parsed = SplAccount::unpack(&account.data()[..SplAccount::LEN]).unwrap();
        assert_eq!(crate::token_decode::to_pubkey(&parsed.mint), mint);
        assert_eq!(parsed.amount, 0);
    }

    #[test]
    fn update_sets_amount() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let mut account = build_token_account(&token, &mint, &owner, &Rent::default()).unwrap();
        let result =
            update_token_balance_in_account(&mut account, &token, &mint, &owner, 42_000_000, 6)
                .unwrap();
        assert_eq!(result.amount_raw, 42_000_000);
        assert_eq!(result.decimals, 6);
        assert!((result.ui_amount - 42.0).abs() < f64::EPSILON);

        let parsed = SplAccount::unpack(&account.data()[..SplAccount::LEN]).unwrap();
        assert_eq!(parsed.amount, 42_000_000);
    }

    #[test]
    fn update_rejects_wrong_program() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let mut account = AccountSharedData::from(solana_account::Account {
            lamports: 0,
            data: vec![0u8; 165],
            owner: token2022_program_id(),
            executable: false,
            rent_epoch: 0,
        });
        let err = update_token_balance_in_account(&mut account, &token, &mint, &owner, 100, 6)
            .unwrap_err();
        assert!(err.to_string().contains("not owned by"));
    }

    #[test]
    fn create_sets_owner_field() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let rent = Rent::default();
        let account = build_token_account(&token, &mint, &owner, &rent).unwrap();
        let parsed = SplAccount::unpack(&account.data()[..SplAccount::LEN]).unwrap();
        assert_eq!(Pubkey::new_from_array(parsed.owner.to_bytes()), owner);
    }
}
