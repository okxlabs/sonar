use anyhow::{Result, anyhow};
use solana_account::Account;
use solana_pubkey::Pubkey;
use spl_token::solana_program::program_pack::Pack;

use crate::types::ResolvedAccounts;

use super::{
    PreparedTokenFunding, TokenProgramKind, ensure_same_program, legacy_program_id,
    raw_to_ui_amount,
};

pub(super) fn read_mint_decimals(account: &Account) -> Result<u8> {
    use spl_token::state::Mint as SplMint;

    if account.data.len() < SplMint::LEN {
        return Err(anyhow!(
            "Mint account data is smaller than expected: {} < {}",
            account.data.len(),
            SplMint::LEN
        ));
    }
    let parsed = SplMint::unpack(&account.data[..SplMint::LEN])
        .map_err(|err| anyhow!("Failed to unpack SPL mint account: {err}"))?;
    Ok(parsed.decimals)
}

pub(super) fn create_token_account(
    resolved: &mut ResolvedAccounts,
    account_pubkey: &Pubkey,
    mint: &Pubkey,
) -> Result<()> {
    use spl_token::solana_program::{program_option::COption, pubkey::Pubkey as ProgramPubkey};
    use spl_token::state::{Account as SplAccount, AccountState};

    let mut data = vec![0u8; SplAccount::LEN];
    let state = SplAccount {
        mint: ProgramPubkey::new_from_array(mint.to_bytes()),
        owner: ProgramPubkey::new_from_array(account_pubkey.to_bytes()),
        amount: 0,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    };
    SplAccount::pack(state, &mut data)
        .map_err(|err| anyhow!("Failed to pack new SPL token account: {err}"))?;
    resolved.accounts.insert(
        *account_pubkey,
        Account { lamports: 0, data, owner: legacy_program_id(), executable: false, rent_epoch: 0 },
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
    use spl_token::state::Account as SplAccount;

    let account = resolved
        .accounts
        .get_mut(account_pubkey)
        .ok_or_else(|| anyhow!("Token account {} missing for mutation", account_pubkey))?;
    ensure_same_program(TokenProgramKind::Legacy, &account.owner, "token account")?;
    if account.data.len() < SplAccount::LEN {
        return Err(anyhow!(
            "Token account data is smaller than expected: {} < {}",
            account.data.len(),
            SplAccount::LEN
        ));
    }
    let (account_bytes, _) = account.data.split_at_mut(SplAccount::LEN);
    let mut parsed = SplAccount::unpack(account_bytes)
        .map_err(|err| anyhow!("Failed to unpack SPL token account: {err}"))?;
    parsed.amount = amount_raw;
    SplAccount::pack(parsed, account_bytes)
        .map_err(|err| anyhow!("Failed to update SPL token account: {err}"))?;

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

    use solana_pubkey::Pubkey;
    use spl_token::solana_program::program_pack::Pack;

    use crate::types::ResolvedAccounts;

    use super::*;

    #[test]
    fn create_initializes_valid_account() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        create_token_account(&mut resolved, &token, &mint).unwrap();

        let account = resolved.accounts.get(&token).expect("account created");
        assert_eq!(account.owner, legacy_program_id());

        use spl_token::state::Account as SplAccount;
        let parsed = SplAccount::unpack(&account.data[..SplAccount::LEN]).unwrap();
        assert_eq!(Pubkey::new_from_array(parsed.mint.to_bytes()), mint);
        assert_eq!(parsed.amount, 0);
    }

    #[test]
    fn update_sets_amount() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        create_token_account(&mut resolved, &token, &mint).unwrap();

        let result = update_account(&mut resolved, &token, &mint, 42_000_000, 6).unwrap();
        assert_eq!(result.amount_raw, 42_000_000);
        assert_eq!(result.decimals, 6);
        assert!((result.ui_amount - 42.0).abs() < f64::EPSILON);

        use spl_token::state::Account as SplAccount;
        let account = resolved.accounts.get(&token).unwrap();
        let parsed = SplAccount::unpack(&account.data[..SplAccount::LEN]).unwrap();
        assert_eq!(parsed.amount, 42_000_000);
    }

    #[test]
    fn update_rejects_wrong_program() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        resolved.accounts.insert(
            token,
            solana_account::Account {
                lamports: 0,
                data: vec![0u8; 165],
                owner: super::super::token2022_program_id(),
                executable: false,
                rent_epoch: 0,
            },
        );

        let err = update_account(&mut resolved, &token, &mint, 100, 6).unwrap_err();
        assert!(err.to_string().contains("not owned by"));
    }

    #[test]
    fn read_mint_decimals_returns_correct_value() {
        use spl_token::solana_program::program_option::COption;
        use spl_token::state::Mint as SplMint;

        let mint_state = SplMint {
            mint_authority: COption::None,
            supply: 0,
            decimals: 9,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        let mut data = vec![0u8; SplMint::LEN];
        SplMint::pack(mint_state, &mut data).unwrap();

        let account = solana_account::Account {
            lamports: 0,
            data,
            owner: legacy_program_id(),
            executable: false,
            rent_epoch: 0,
        };

        assert_eq!(read_mint_decimals(&account).unwrap(), 9);
    }

    #[test]
    fn read_mint_decimals_rejects_short_data() {
        let account = solana_account::Account {
            lamports: 0,
            data: vec![0u8; 10],
            owner: legacy_program_id(),
            executable: false,
            rent_epoch: 0,
        };

        let err = read_mint_decimals(&account).unwrap_err();
        assert!(err.to_string().contains("smaller than expected"));
    }
}
