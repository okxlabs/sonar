use anyhow::{Result, anyhow};
use solana_account::Account;
use solana_pubkey::Pubkey;
use spl_token::solana_program::program_pack::Pack;
use spl_token_2022::extension::{BaseStateWithExtensions, BaseStateWithExtensionsMut};

use crate::types::ResolvedAccounts;

use super::{
    PreparedTokenFunding, TokenProgramKind, ensure_same_program, raw_to_ui_amount,
    token2022_program_id,
};

pub(super) fn read_mint_decimals(account: &Account) -> Result<u8> {
    use spl_token_2022::state::Mint as Token2022Mint;

    if account.data.len() < Token2022Mint::LEN {
        return Err(anyhow!(
            "Mint account data is smaller than expected: {} < {}",
            account.data.len(),
            Token2022Mint::LEN
        ));
    }
    let parsed = Token2022Mint::unpack(&account.data[..Token2022Mint::LEN])
        .map_err(|err| anyhow!("Failed to unpack token-2022 mint account: {err}"))?;
    Ok(parsed.decimals)
}

pub(super) fn create_token_account_with_extensions(
    resolved: &mut ResolvedAccounts,
    account_pubkey: &Pubkey,
    mint: &Pubkey,
    mint_account: &Account,
) -> Result<()> {
    use spl_token::solana_program::{program_option::COption, pubkey::Pubkey as ProgramPubkey};
    use spl_token_2022::extension::{ExtensionType, StateWithExtensions, StateWithExtensionsMut};
    use spl_token_2022::state::{Account as Token2022Account, AccountState, Mint as Token2022Mint};

    let mint_state = StateWithExtensions::<Token2022Mint>::unpack(&mint_account.data)
        .map_err(|e| anyhow!("Failed to unpack token-2022 mint {}: {}", mint, e))?;
    let mint_extension_types = mint_state
        .get_extension_types()
        .map_err(|e| anyhow!("Failed to get mint extension types: {}", e))?;

    let required_extensions =
        ExtensionType::get_required_init_account_extensions(&mint_extension_types);
    let account_len =
        ExtensionType::try_calculate_account_len::<Token2022Account>(&required_extensions)
            .map_err(|e| anyhow!("Failed to calculate token-2022 account length: {}", e))?;

    let mut data = vec![0u8; account_len];
    let mut state = StateWithExtensionsMut::<Token2022Account>::unpack_uninitialized(&mut data)
        .map_err(|e| anyhow!("Failed to unpack uninitialized token-2022 account buffer: {}", e))?;

    state.base = Token2022Account {
        mint: ProgramPubkey::new_from_array(mint.to_bytes()),
        owner: ProgramPubkey::new_from_array(account_pubkey.to_bytes()),
        amount: 0,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    };

    state.init_account_type().map_err(|e| anyhow!("Failed to init account type: {}", e))?;
    state.pack_base();
    for ext_type in &required_extensions {
        state
            .init_account_extension_from_type(*ext_type)
            .map_err(|e| anyhow!("Failed to init extension {:?}: {}", ext_type, e))?;
    }

    resolved.accounts.insert(
        *account_pubkey,
        Account {
            lamports: 0,
            data,
            owner: token2022_program_id(),
            executable: false,
            rent_epoch: 0,
        },
    );
    Ok(())
}

pub(super) fn update_account(
    resolved: &mut ResolvedAccounts,
    account_pubkey: &Pubkey,
    mint: &Pubkey,
    amount_raw: u64,
    decimals: u8,
) -> Result<PreparedTokenFunding> {
    use spl_token_2022::state::Account as Token2022Account;

    let account = resolved
        .accounts
        .get_mut(account_pubkey)
        .ok_or_else(|| anyhow!("Token account {} missing for mutation", account_pubkey))?;
    ensure_same_program(TokenProgramKind::Token2022, &account.owner, "token account")?;
    if account.data.len() < Token2022Account::LEN {
        return Err(anyhow!(
            "Token account data is smaller than expected: {} < {}",
            account.data.len(),
            Token2022Account::LEN
        ));
    }
    let (account_bytes, _) = account.data.split_at_mut(Token2022Account::LEN);
    let mut parsed = Token2022Account::unpack(account_bytes)
        .map_err(|err| anyhow!("Failed to unpack token-2022 account: {err}"))?;
    parsed.amount = amount_raw;
    Token2022Account::pack(parsed, account_bytes)
        .map_err(|err| anyhow!("Failed to update token-2022 account {}: {err}", account_pubkey))?;

    Ok(PreparedTokenFunding {
        account: *account_pubkey,
        mint: *mint,
        decimals,
        amount_raw,
        ui_amount: raw_to_ui_amount(amount_raw, decimals),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use solana_account::Account;
    use solana_pubkey::Pubkey;
    use spl_token_2022::extension::{
        BaseStateWithExtensions, BaseStateWithExtensionsMut, ExtensionType, StateWithExtensions,
        StateWithExtensionsMut,
    };
    use spl_token_2022::state::{Account as Token2022Account, Mint as Token2022Mint};

    use crate::types::ResolvedAccounts;

    use super::*;

    fn mint_account_base_only() -> Account {
        use spl_token::solana_program::program_option::COption;

        let mint = Token2022Mint {
            mint_authority: COption::None,
            supply: 0,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        let mut data = vec![0u8; Token2022Mint::LEN];
        Token2022Mint::pack(mint, &mut data).unwrap();
        Account {
            lamports: 0,
            data,
            owner: token2022_program_id(),
            executable: false,
            rent_epoch: 0,
        }
    }

    fn mint_account_with_transfer_fee_config() -> Account {
        use spl_token::solana_program::program_option::COption;
        use spl_token_2022::extension::transfer_fee::TransferFeeConfig;

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

        Account {
            lamports: 0,
            data,
            owner: token2022_program_id(),
            executable: false,
            rent_epoch: 0,
        }
    }

    #[test]
    fn create_base_only_account() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        let mint_account = mint_account_base_only();
        create_token_account_with_extensions(&mut resolved, &token, &mint, &mint_account).unwrap();

        let account = resolved.accounts.get(&token).expect("account created");
        assert_eq!(account.owner, token2022_program_id());
        assert_eq!(
            account.data.len(),
            Token2022Account::LEN,
            "base Token2022 account has no extensions"
        );
        let state = StateWithExtensions::<Token2022Account>::unpack(&account.data).expect("unpack");
        assert_eq!(state.base.amount, 0);
        assert!(state.get_extension_types().unwrap().is_empty());
    }

    #[test]
    fn create_account_with_transfer_fee_extension() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        let mint_account = mint_account_with_transfer_fee_config();
        create_token_account_with_extensions(&mut resolved, &token, &mint, &mint_account).unwrap();

        let account = resolved.accounts.get(&token).expect("account created");
        assert_eq!(account.owner, token2022_program_id());
        assert!(
            account.data.len() > Token2022Account::LEN,
            "account with TransferFeeAmount extension must be larger than base"
        );
        let state = StateWithExtensions::<Token2022Account>::unpack(&account.data).expect("unpack");
        assert_eq!(state.base.amount, 0);
        let ext_types = state.get_extension_types().unwrap();
        assert!(
            ext_types.contains(&ExtensionType::TransferFeeAmount),
            "expected TransferFeeAmount extension, got {:?}",
            ext_types
        );
    }

    #[test]
    fn read_mint_decimals_returns_correct_value() {
        let account = mint_account_base_only();
        assert_eq!(read_mint_decimals(&account).unwrap(), 6);
    }

    #[test]
    fn read_mint_decimals_rejects_short_data() {
        let account = Account {
            lamports: 0,
            data: vec![0u8; 10],
            owner: token2022_program_id(),
            executable: false,
            rent_epoch: 0,
        };
        let err = read_mint_decimals(&account).unwrap_err();
        assert!(err.to_string().contains("smaller than expected"));
    }

    #[test]
    fn update_sets_amount() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        let mint_account = mint_account_base_only();
        create_token_account_with_extensions(&mut resolved, &token, &mint, &mint_account).unwrap();

        let result = update_account(&mut resolved, &token, &mint, 5_000_000, 6).unwrap();
        assert_eq!(result.amount_raw, 5_000_000);
        assert!((result.ui_amount - 5.0).abs() < f64::EPSILON);

        let account = resolved.accounts.get(&token).unwrap();
        let state = StateWithExtensions::<Token2022Account>::unpack(&account.data).expect("unpack");
        assert_eq!(state.base.amount, 5_000_000);
    }
}
