use anyhow::{Context, Result, anyhow};
use solana_pubkey::Pubkey;
use spl_token::solana_program::program_pack::Pack;

use crate::{
    account_loader::{AccountLoader, ResolvedAccounts},
    cli::TokenFunding,
};

#[derive(Clone, Debug)]
pub struct PreparedTokenFunding {
    pub account: Pubkey,
    pub mint: Pubkey,
    pub decimals: u8,
    pub amount_raw: u64,
    pub ui_amount: f64,
}

pub fn prepare_token_fundings(
    loader: &AccountLoader,
    resolved: &mut ResolvedAccounts,
    requests: &[TokenFunding],
) -> Result<Vec<PreparedTokenFunding>> {
    let mut prepared = Vec::new();
    if requests.is_empty() {
        return Ok(prepared);
    }

    for request in requests {
        let summary = process_single(loader, resolved, request)
            .with_context(|| format!("Failed to prepare token funding for {}", request.account))?;
        prepared.push(summary);
    }

    Ok(prepared)
}

fn process_single(
    loader: &AccountLoader,
    resolved: &mut ResolvedAccounts,
    request: &TokenFunding,
) -> Result<PreparedTokenFunding> {
    ensure_account_loaded(loader, resolved, &request.mint)
        .with_context(|| format!("Failed to load mint account {}", request.mint))?;
    let mint_account = resolved
        .accounts
        .get(&request.mint)
        .ok_or_else(|| anyhow!("Mint account {} missing after load", request.mint))?;

    let program_kind = TokenProgramKind::from_owner(&mint_account.owner).ok_or_else(|| {
        anyhow!(
            "Mint account {} is not owned by the SPL Token programs; cannot prepare funding",
            request.mint
        )
    })?;

    let decimals = match program_kind {
        TokenProgramKind::Legacy => read_legacy_mint_decimals(mint_account)?,
        TokenProgramKind::Token2022 => read_token2022_mint_decimals(mint_account)?,
    };

    ensure_account_loaded(loader, resolved, &request.account).with_context(|| {
        format!("Failed to load token account {} required for funding", request.account)
    })?;

    if !resolved.accounts.contains_key(&request.account) {
        create_missing_token_account(resolved, request, program_kind)?;
    }

    match program_kind {
        TokenProgramKind::Legacy => update_legacy_account(resolved, request, decimals),
        TokenProgramKind::Token2022 => update_token2022_account(resolved, request, decimals),
    }
}

fn read_legacy_mint_decimals(account: &solana_account::Account) -> Result<u8> {
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

fn read_token2022_mint_decimals(account: &solana_account::Account) -> Result<u8> {
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

fn create_missing_token_account(
    resolved: &mut ResolvedAccounts,
    request: &TokenFunding,
    kind: TokenProgramKind,
) -> Result<()> {
    use solana_account::Account;

    match kind {
        TokenProgramKind::Legacy => {
            use spl_token::solana_program::{
                program_option::COption, pubkey::Pubkey as ProgramPubkey,
            };
            use spl_token::state::{Account as SplAccount, AccountState};

            let mut data = vec![0u8; SplAccount::LEN];
            let state = SplAccount {
                mint: ProgramPubkey::new_from_array(request.mint.to_bytes()),
                owner: ProgramPubkey::new_from_array(request.account.to_bytes()),
                amount: request.amount_raw,
                delegate: COption::None,
                state: AccountState::Initialized,
                is_native: COption::None,
                delegated_amount: 0,
                close_authority: COption::None,
            };
            SplAccount::pack(state, &mut data)
                .map_err(|err| anyhow!("Failed to pack new SPL token account: {err}"))?;
            resolved.accounts.insert(
                request.account,
                Account {
                    lamports: 0,
                    data,
                    owner: legacy_program_id(),
                    executable: false,
                    rent_epoch: 0,
                },
            );
        }
        TokenProgramKind::Token2022 => {
            use spl_token::solana_program::{
                program_option::COption, pubkey::Pubkey as ProgramPubkey,
            };
            use spl_token_2022::state::{Account as Token2022Account, AccountState};

            let mut data = vec![0u8; Token2022Account::LEN];
            let state = Token2022Account {
                mint: ProgramPubkey::new_from_array(request.mint.to_bytes()),
                owner: ProgramPubkey::new_from_array(request.account.to_bytes()),
                amount: request.amount_raw,
                delegate: COption::None,
                state: AccountState::Initialized,
                is_native: COption::None,
                delegated_amount: 0,
                close_authority: COption::None,
            };
            Token2022Account::pack(state, &mut data)
                .map_err(|err| anyhow!("Failed to pack new token-2022 account: {err}"))?;
            resolved.accounts.insert(
                request.account,
                Account {
                    lamports: 0,
                    data,
                    owner: token2022_program_id(),
                    executable: false,
                    rent_epoch: 0,
                },
            );
        }
    }

    Ok(())
}

fn update_legacy_account(
    resolved: &mut ResolvedAccounts,
    request: &TokenFunding,
    decimals: u8,
) -> Result<PreparedTokenFunding> {
    use spl_token::state::Account as SplAccount;

    let account = resolved
        .accounts
        .get_mut(&request.account)
        .ok_or_else(|| anyhow!("Token account {} missing for mutation", request.account))?;
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
    let stored_mint = Pubkey::new_from_array(parsed.mint.to_bytes());
    if stored_mint != request.mint {
        return Err(anyhow!(
            "Token account {} is associated with mint {}, but CLI requested mint {}",
            request.account,
            stored_mint,
            request.mint
        ));
    }
    parsed.amount = request.amount_raw;
    SplAccount::pack(parsed, account_bytes)
        .map_err(|err| anyhow!("Failed to update SPL token account: {err}"))?;

    Ok(PreparedTokenFunding {
        account: request.account,
        mint: request.mint,
        decimals,
        amount_raw: request.amount_raw,
        ui_amount: raw_to_ui_amount(request.amount_raw, decimals),
    })
}

fn update_token2022_account(
    resolved: &mut ResolvedAccounts,
    request: &TokenFunding,
    decimals: u8,
) -> Result<PreparedTokenFunding> {
    use spl_token_2022::state::Account as Token2022Account;

    let account = resolved
        .accounts
        .get_mut(&request.account)
        .ok_or_else(|| anyhow!("Token account {} missing for mutation", request.account))?;
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
    let stored_mint = Pubkey::new_from_array(parsed.mint.to_bytes());
    if stored_mint != request.mint {
        return Err(anyhow!(
            "Token account {} is associated with mint {}, but CLI requested mint {}",
            request.account,
            stored_mint,
            request.mint
        ));
    }
    parsed.amount = request.amount_raw;
    Token2022Account::pack(parsed, account_bytes)
        .map_err(|err| anyhow!("Failed to update token-2022 account {}: {err}", request.account))?;

    Ok(PreparedTokenFunding {
        account: request.account,
        mint: request.mint,
        decimals,
        amount_raw: request.amount_raw,
        ui_amount: raw_to_ui_amount(request.amount_raw, decimals),
    })
}

fn ensure_account_loaded(
    loader: &AccountLoader,
    resolved: &mut ResolvedAccounts,
    pubkey: &Pubkey,
) -> Result<()> {
    if resolved.accounts.contains_key(pubkey) {
        return Ok(());
    }
    loader.append_accounts(resolved, &[*pubkey])
}

fn raw_to_ui_amount(amount_raw: u64, decimals: u8) -> f64 {
    let factor = 10f64.powi(decimals as i32);
    if factor == 0.0 { amount_raw as f64 } else { (amount_raw as f64) / factor }
}

fn ensure_same_program(kind: TokenProgramKind, owner: &Pubkey, label: &str) -> Result<()> {
    if owner != &kind.program_id() {
        return Err(anyhow!("Provided {label} is not owned by {}", kind.program_name()));
    }
    Ok(())
}

#[derive(Copy, Clone)]
enum TokenProgramKind {
    Legacy,
    Token2022,
}

impl TokenProgramKind {
    fn from_owner(owner: &Pubkey) -> Option<Self> {
        if *owner == legacy_program_id() {
            Some(TokenProgramKind::Legacy)
        } else if *owner == token2022_program_id() {
            Some(TokenProgramKind::Token2022)
        } else {
            None
        }
    }

    fn program_id(&self) -> Pubkey {
        match self {
            TokenProgramKind::Legacy => legacy_program_id(),
            TokenProgramKind::Token2022 => token2022_program_id(),
        }
    }

    fn program_name(&self) -> &'static str {
        match self {
            TokenProgramKind::Legacy => "SPL Token",
            TokenProgramKind::Token2022 => "SPL Token 2022",
        }
    }
}

fn legacy_program_id() -> Pubkey {
    Pubkey::new_from_array(spl_token::ID.to_bytes())
}

fn token2022_program_id() -> Pubkey {
    Pubkey::new_from_array(spl_token_2022::ID.to_bytes())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use solana_account::Account;
    use solana_pubkey::Pubkey;
    use spl_token::solana_program::program_pack::Pack;

    use super::*;

    #[test]
    fn prepares_spl_token_funding_and_updates_account_data() {
        let loader = AccountLoader::new("http://localhost:8899".into()).unwrap();
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let (token_account, mint_account) = spl_token_account_and_mint(&mint, &owner);

        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };
        resolved.accounts.insert(token, token_account);
        resolved.accounts.insert(mint, mint_account);

        let funding = TokenFunding { account: token, mint, amount_raw: 1_500_000 };
        let prepared =
            prepare_token_fundings(&loader, &mut resolved, &[funding]).expect("prepares funding");
        assert_eq!(prepared.len(), 1);
        let summary = &prepared[0];
        assert_eq!(summary.mint, mint);
        assert_eq!(summary.decimals, 6);
        assert!((summary.ui_amount - 1.5).abs() < f64::EPSILON);
        assert!(summary.amount_raw > 0);

        let updated_account = resolved.accounts.get(&token).unwrap();
        use spl_token::state::Account as SplAccount;
        let parsed = SplAccount::unpack(&updated_account.data[..SplAccount::LEN]).unwrap();
        assert_eq!(parsed.amount, summary.amount_raw);
    }

    #[test]
    fn create_missing_token_account_initializes_data() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        let request = TokenFunding { account: token, mint, amount_raw: 42 };
        create_missing_token_account(&mut resolved, &request, TokenProgramKind::Legacy).unwrap();

        let account = resolved.accounts.get(&token).expect("account created");
        assert_eq!(account.owner, legacy_program_id());
        use spl_token::state::Account as SplAccount;
        let parsed = SplAccount::unpack(&account.data[..SplAccount::LEN]).unwrap();
        assert_eq!(Pubkey::new_from_array(parsed.mint.to_bytes()), mint);
        assert_eq!(parsed.amount, 42);
    }

    fn spl_token_account_and_mint(mint: &Pubkey, owner: &Pubkey) -> (Account, Account) {
        use spl_token::solana_program::{program_option::COption, pubkey::Pubkey as ProgramPubkey};
        use spl_token::state::{Account as SplAccount, AccountState, Mint as SplMint};

        let token_state = SplAccount {
            mint: ProgramPubkey::new_from_array(mint.to_bytes()),
            owner: ProgramPubkey::new_from_array(owner.to_bytes()),
            amount: 0,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        };

        let mut token_data = vec![0u8; SplAccount::LEN];
        SplAccount::pack(token_state, &mut token_data).unwrap();

        let mint_state = SplMint {
            mint_authority: COption::None,
            supply: 0,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        let mut mint_data = vec![0u8; SplMint::LEN];
        SplMint::pack(mint_state, &mut mint_data).unwrap();

        let token_account = Account {
            lamports: 0,
            data: token_data,
            owner: legacy_program_id(),
            executable: false,
            rent_epoch: 0,
        };
        let mint_account = Account {
            lamports: 0,
            data: mint_data,
            owner: legacy_program_id(),
            executable: false,
            rent_epoch: 0,
        };
        (token_account, mint_account)
    }
}
