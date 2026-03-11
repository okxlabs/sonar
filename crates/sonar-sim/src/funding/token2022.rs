use solana_account::{Account, AccountSharedData, ReadableAccount};
use solana_pubkey::Pubkey;
use solana_rent::Rent;
use spl_token::solana_program::{program_option::COption, pubkey::Pubkey as ProgramPubkey};
use spl_token_2022::extension::{BaseStateWithExtensions, BaseStateWithExtensionsMut};
use spl_token_2022::extension::{ExtensionType, StateWithExtensions, StateWithExtensionsMut};
use spl_token_2022::state::{Account as Token2022Account, AccountState, Mint as Token2022Mint};

use crate::error::{Result, SonarSimError};
use crate::token_decode::{TokenProgramKind, token2022_program_id};
use crate::types::PreparedTokenFunding;

pub(super) fn build_token_account_with_extensions(
    account_pubkey: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    mint_account: &AccountSharedData,
    rent: &Rent,
) -> Result<AccountSharedData> {
    let mint_state =
        StateWithExtensions::<Token2022Mint>::unpack(mint_account.data()).map_err(|e| {
            SonarSimError::Token {
                account: Some(*mint),
                reason: format!("Failed to unpack token-2022 mint {}: {}", mint, e),
            }
        })?;
    let mint_extension_types =
        mint_state.get_extension_types().map_err(|e| SonarSimError::Token {
            account: Some(*mint),
            reason: format!("Failed to get mint extension types: {}", e),
        })?;

    let required_extensions =
        ExtensionType::get_required_init_account_extensions(&mint_extension_types);
    let account_len =
        ExtensionType::try_calculate_account_len::<Token2022Account>(&required_extensions)
            .map_err(|e| SonarSimError::Token {
                account: Some(*account_pubkey),
                reason: format!("Failed to calculate token-2022 account length: {}", e),
            })?;

    let mut data = vec![0u8; account_len];
    let mut state = StateWithExtensionsMut::<Token2022Account>::unpack_uninitialized(&mut data)
        .map_err(|e| SonarSimError::Token {
            account: Some(*account_pubkey),
            reason: format!("Failed to unpack uninitialized token-2022 account buffer: {}", e),
        })?;

    state.base = Token2022Account {
        mint: ProgramPubkey::new_from_array(mint.to_bytes()),
        owner: ProgramPubkey::new_from_array(owner.to_bytes()),
        amount: 0,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    };

    state.init_account_type().map_err(|e| SonarSimError::Token {
        account: Some(*account_pubkey),
        reason: format!("Failed to init account type: {}", e),
    })?;
    state.pack_base();
    for ext_type in &required_extensions {
        state.init_account_extension_from_type(*ext_type).map_err(|e| SonarSimError::Token {
            account: Some(*account_pubkey),
            reason: format!("Failed to init extension {:?}: {}", ext_type, e),
        })?;
    }

    Ok(AccountSharedData::from(Account {
        lamports: rent.minimum_balance(account_len),
        data,
        owner: token2022_program_id(),
        executable: false,
        rent_epoch: 0,
    }))
}

pub(super) fn update_token_balance_in_account(
    account: &mut Account,
    account_pubkey: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    amount_raw: u64,
    decimals: u8,
) -> Result<PreparedTokenFunding> {
    super::common::update_token_amount_account::<spl_token_2022::state::Account>(
        account,
        account_pubkey,
        mint,
        owner,
        amount_raw,
        decimals,
        TokenProgramKind::Token2022,
    )
}

#[cfg(test)]
mod tests {
    use solana_account::{Account, ReadableAccount};
    use solana_pubkey::Pubkey;
    use solana_rent::Rent;
    use spl_token::solana_program::program_option::COption;
    use spl_token::solana_program::program_pack::Pack;
    use spl_token_2022::extension::transfer_fee::TransferFeeConfig;
    use spl_token_2022::extension::{
        BaseStateWithExtensions, BaseStateWithExtensionsMut, ExtensionType, StateWithExtensions,
        StateWithExtensionsMut,
    };
    use spl_token_2022::state::{Account as Token2022Account, Mint as Token2022Mint};

    use super::*;
    use crate::token_decode::token2022_program_id;

    fn mint_account_base_only() -> AccountSharedData {
        let mint = Token2022Mint {
            mint_authority: COption::None,
            supply: 0,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        let mut data = vec![0u8; Token2022Mint::LEN];
        Token2022Mint::pack(mint, &mut data).unwrap();
        AccountSharedData::from(Account {
            lamports: 0,
            data,
            owner: token2022_program_id(),
            executable: false,
            rent_epoch: 0,
        })
    }

    fn mint_account_with_transfer_fee_config() -> AccountSharedData {
        let account_len = ExtensionType::try_calculate_account_len::<Token2022Mint>(&[
            ExtensionType::TransferFeeConfig,
        ])
        .expect("calculate mint len");
        let mut data = vec![0u8; account_len];
        let mut state = StateWithExtensionsMut::<Token2022Mint>::unpack_uninitialized(&mut data)
            .expect("unpack_uninitialized mint");

        state.base = Token2022Mint {
            mint_authority: COption::None,
            supply: 0,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        state.init_account_type().expect("init_account_type");
        state.pack_base();
        state.init_extension::<TransferFeeConfig>(true).expect("init TransferFeeConfig");

        AccountSharedData::from(Account {
            lamports: 0,
            data,
            owner: token2022_program_id(),
            executable: false,
            rent_epoch: 0,
        })
    }

    #[test]
    fn create_base_only_account() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let mint_account = mint_account_base_only();
        let rent = Rent::default();
        let account =
            build_token_account_with_extensions(&token, &mint, &owner, &mint_account, &rent)
                .unwrap();
        assert_eq!(*account.owner(), token2022_program_id());
        assert_eq!(account.lamports(), rent.minimum_balance(Token2022Account::LEN));
        assert_eq!(
            account.data().len(),
            Token2022Account::LEN,
            "base Token2022 account has no extensions"
        );
        let state =
            StateWithExtensions::<Token2022Account>::unpack(account.data()).expect("unpack");
        assert_eq!(state.base.amount, 0);
        assert!(state.get_extension_types().unwrap().is_empty());
    }

    #[test]
    fn create_account_with_transfer_fee_extension() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let mint_account = mint_account_with_transfer_fee_config();
        let rent = Rent::default();
        let account =
            build_token_account_with_extensions(&token, &mint, &owner, &mint_account, &rent)
                .unwrap();
        assert_eq!(*account.owner(), token2022_program_id());
        assert!(
            account.data().len() > Token2022Account::LEN,
            "account with TransferFeeAmount extension must be larger than base"
        );
        assert_eq!(account.lamports(), rent.minimum_balance(account.data().len()));
        let state =
            StateWithExtensions::<Token2022Account>::unpack(account.data()).expect("unpack");
        assert_eq!(state.base.amount, 0);
        let ext_types = state.get_extension_types().unwrap();
        assert!(
            ext_types.contains(&ExtensionType::TransferFeeAmount),
            "expected TransferFeeAmount extension, got {:?}",
            ext_types
        );
    }

    #[test]
    fn update_sets_amount() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let mint_account = mint_account_base_only();
        let mut account = Account::from(
            build_token_account_with_extensions(
                &token,
                &mint,
                &owner,
                &mint_account,
                &Rent::default(),
            )
            .unwrap(),
        );
        let result =
            update_token_balance_in_account(&mut account, &token, &mint, &owner, 5_000_000, 6).unwrap();
        assert_eq!(result.amount_raw, 5_000_000);
        assert!((result.ui_amount - 5.0).abs() < f64::EPSILON);

        let state = StateWithExtensions::<Token2022Account>::unpack(&account.data).expect("unpack");
        assert_eq!(state.base.amount, 5_000_000);
    }

    #[test]
    fn create_sets_owner_field() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let mint_account = mint_account_base_only();
        let rent = Rent::default();
        let account =
            build_token_account_with_extensions(&token, &mint, &owner, &mint_account, &rent)
                .unwrap();
        let state = StateWithExtensions::<Token2022Account>::unpack(account.data()).expect("unpack");
        assert_eq!(Pubkey::new_from_array(state.base.owner.to_bytes()), owner);
    }
}
