mod sol;
mod token2022;
mod token_legacy;

pub use sol::apply_sol_fundings;

use solana_account::{AccountSharedData, ReadableAccount, WritableAccount};
use solana_pubkey::Pubkey;
use spl_token::solana_program::program_pack::Pack;

use crate::error::{Result, SonarSimError};
use crate::token_decode::{self, TokenProgramKind, ensure_same_program, raw_to_ui_amount};
use crate::types::{
    AccountAppender, PreparedTokenFunding, ResolvedAccounts, TokenAmount, TokenFunding,
};

pub(super) trait TokenAmountMut:
    Pack + spl_token::solana_program::program_pack::IsInitialized
{
    fn set_amount(&mut self, amount: u64);
}

impl TokenAmountMut for spl_token::state::Account {
    fn set_amount(&mut self, amount: u64) {
        self.amount = amount;
    }
}

impl TokenAmountMut for spl_token_2022::state::Account {
    fn set_amount(&mut self, amount: u64) {
        self.amount = amount;
    }
}

pub(super) fn update_token_amount<T: TokenAmountMut>(
    resolved: &mut ResolvedAccounts,
    account_pubkey: &Pubkey,
    mint: &Pubkey,
    amount_raw: u64,
    decimals: u8,
    program_kind: TokenProgramKind,
) -> Result<PreparedTokenFunding> {
    let account = resolved
        .accounts
        .get_mut(account_pubkey)
        .ok_or(SonarSimError::AccountNotFound { pubkey: *account_pubkey })?;
    ensure_same_program(program_kind, account.owner(), "token account")?;
    if account.data().len() < T::LEN {
        return Err(SonarSimError::Token {
            account: Some(*account_pubkey),
            reason: format!(
                "Token account data is smaller than expected: {} < {}",
                account.data().len(),
                T::LEN
            ),
        });
    }
    let data = account.data_as_mut_slice();
    let (account_bytes, _) = data.split_at_mut(T::LEN);
    let mut parsed = T::unpack(account_bytes).map_err(|err| SonarSimError::Token {
        account: Some(*account_pubkey),
        reason: format!("Failed to unpack token account {account_pubkey}: {err}"),
    })?;
    parsed.set_amount(amount_raw);
    T::pack(parsed, account_bytes).map_err(|err| SonarSimError::Token {
        account: Some(*account_pubkey),
        reason: format!("Failed to update token account {account_pubkey}: {err}"),
    })?;
    Ok(PreparedTokenFunding {
        account: *account_pubkey,
        mint: *mint,
        decimals,
        amount_raw,
        ui_amount: raw_to_ui_amount(amount_raw, decimals),
    })
}

pub fn prepare_token_fundings(
    loader: &mut dyn AccountAppender,
    resolved: &mut ResolvedAccounts,
    requests: &[TokenFunding],
) -> Result<Vec<PreparedTokenFunding>> {
    let mut prepared = Vec::new();
    if requests.is_empty() {
        return Ok(prepared);
    }

    let total = requests.len();
    for (index, request) in requests.iter().enumerate() {
        log::debug!("Preparing token fundings ({}/{})", index + 1, total);
        let summary = prepare_single_token_funding(loader, resolved, request).map_err(|e| {
            SonarSimError::Token {
                account: Some(request.account),
                reason: format!("Failed to prepare token funding for {}: {e}", request.account),
            }
        })?;
        prepared.push(summary);
    }

    Ok(prepared)
}

fn prepare_single_token_funding(
    loader: &mut dyn AccountAppender,
    resolved: &mut ResolvedAccounts,
    request: &TokenFunding,
) -> Result<PreparedTokenFunding> {
    ensure_account_loaded(loader, resolved, &request.account).map_err(|e| {
        SonarSimError::Token {
            account: Some(request.account),
            reason: format!(
                "Failed to load token account {} required for funding: {e}",
                request.account
            ),
        }
    })?;

    let mint = if let Some(account) = resolved.accounts.get(&request.account) {
        let detected_mint =
            detect_mint_from_token_account(account).map_err(|e| SonarSimError::Token {
                account: Some(request.account),
                reason: format!(
                    "Failed to detect mint from existing token account {}: {e}",
                    request.account
                ),
            })?;
        if let Some(requested_mint) = request.mint {
            if detected_mint != requested_mint {
                return Err(SonarSimError::Token {
                    account: Some(request.account),
                    reason: format!(
                        "Token account {} is associated with mint {}, but CLI requested mint {}",
                        request.account, detected_mint, requested_mint
                    ),
                });
            }
        }
        detected_mint
    } else {
        request.mint.ok_or_else(|| SonarSimError::Token {
            account: Some(request.account),
            reason: format!(
                "Token account {} does not exist on-chain; \
                 you must specify the mint using <ACCOUNT>:<MINT>=<AMOUNT> format",
                request.account
            ),
        })?
    };

    ensure_account_loaded(loader, resolved, &mint).map_err(|e| SonarSimError::Token {
        account: Some(mint),
        reason: format!("Failed to load mint account {}: {e}", mint),
    })?;
    let mint_account = resolved
        .accounts
        .get(&mint)
        .ok_or(SonarSimError::AccountNotFound { pubkey: mint })?
        .clone();

    let program_kind =
        TokenProgramKind::from_owner(mint_account.owner()).ok_or_else(|| SonarSimError::Token {
            account: Some(mint),
            reason: format!(
                "Mint account {} is not owned by the SPL Token programs; cannot prepare funding",
                mint
            ),
        })?;

    let decimals = token_decode::read_mint_decimals(&mint_account)?;

    let amount_raw =
        resolve_token_amount(&request.amount, decimals).map_err(|e| SonarSimError::Token {
            account: Some(request.account),
            reason: format!("Failed to resolve token amount for {}: {e}", request.account),
        })?;

    if !resolved.accounts.contains_key(&request.account) {
        create_missing_token_account(
            resolved,
            &request.account,
            &mint,
            program_kind,
            &mint_account,
        )?;
    }

    match program_kind {
        TokenProgramKind::Legacy => token_legacy::update_token_balance(
            resolved,
            &request.account,
            &mint,
            amount_raw,
            decimals,
        ),
        TokenProgramKind::Token2022 => {
            token2022::update_token_balance(resolved, &request.account, &mint, amount_raw, decimals)
        }
    }
}

fn detect_mint_from_token_account(account: &AccountSharedData) -> Result<Pubkey> {
    const MINT_OFFSET: usize = 0;
    const MINT_LEN: usize = 32;

    if TokenProgramKind::from_owner(account.owner()).is_none() {
        return Err(SonarSimError::Token {
            account: None,
            reason: format!(
                "Token account is not owned by any known SPL Token program (owner: {})",
                account.owner()
            ),
        });
    }
    let data = account.data();
    if data.len() < MINT_OFFSET + MINT_LEN {
        return Err(SonarSimError::Token {
            account: None,
            reason: format!(
                "Token account data too small to read mint: {} < {}",
                data.len(),
                MINT_OFFSET + MINT_LEN
            ),
        });
    }
    let mint_bytes: [u8; 32] =
        data[MINT_OFFSET..MINT_OFFSET + MINT_LEN].try_into().expect("slice length is 32");
    Ok(Pubkey::new_from_array(mint_bytes))
}

fn create_missing_token_account(
    resolved: &mut ResolvedAccounts,
    account_pubkey: &Pubkey,
    mint: &Pubkey,
    kind: TokenProgramKind,
    mint_account: &AccountSharedData,
) -> Result<()> {
    match kind {
        TokenProgramKind::Legacy => {
            token_legacy::create_token_account(resolved, account_pubkey, mint)
        }
        TokenProgramKind::Token2022 => token2022::create_token_account_with_extensions(
            resolved,
            account_pubkey,
            mint,
            mint_account,
        ),
    }
}

fn ensure_account_loaded(
    loader: &mut dyn AccountAppender,
    resolved: &mut ResolvedAccounts,
    pubkey: &Pubkey,
) -> Result<()> {
    if resolved.accounts.contains_key(pubkey) {
        return Ok(());
    }
    loader.append_accounts(resolved, &[*pubkey])
}

/// Convert a [`TokenAmount`] to a raw `u64` value using the mint's decimals.
///
/// - `TokenAmount::Raw(v)` is returned as-is.
/// - `TokenAmount::Decimal(v)` is multiplied by `10^decimals` and rounded.
fn resolve_token_amount(amount: &TokenAmount, decimals: u8) -> Result<u64> {
    match amount {
        TokenAmount::Raw(raw) => Ok(*raw),
        TokenAmount::Decimal(ui) => {
            let factor = 10u64.pow(decimals as u32);
            let raw_f64 = ui * factor as f64;
            if raw_f64 < 0.0 {
                return Err(SonarSimError::Validation {
                    reason: "Token funding amount must be non-negative".into(),
                });
            }
            if raw_f64 > u64::MAX as f64 {
                return Err(SonarSimError::Validation {
                    reason: format!(
                        "Token funding amount {ui} with {decimals} decimals overflows u64"
                    ),
                });
            }
            Ok(raw_f64.round() as u64)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use solana_account::{Account, AccountSharedData, ReadableAccount};
    use solana_pubkey::Pubkey;
    use spl_token::solana_program::program_pack::Pack;

    use crate::token_decode::{legacy_program_id, raw_to_ui_amount};
    use crate::types::{AccountAppender, ResolvedAccounts, TokenAmount, TokenFunding};

    use super::*;

    struct NoopAppender;

    impl AccountAppender for NoopAppender {
        fn append_accounts(
            &mut self,
            _resolved: &mut ResolvedAccounts,
            _pubkeys: &[Pubkey],
        ) -> crate::error::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn prepares_spl_token_funding_and_updates_account_data() {
        let mut loader = NoopAppender;
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let (token_account, mint_account) = spl_token_account_and_mint(&mint, &owner);

        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };
        resolved.accounts.insert(token, AccountSharedData::from(token_account));
        resolved.accounts.insert(mint, AccountSharedData::from(mint_account));

        let funding =
            TokenFunding { account: token, mint: Some(mint), amount: TokenAmount::Raw(1_500_000) };
        let prepared = prepare_token_fundings(&mut loader, &mut resolved, &[funding])
            .expect("prepares funding");
        assert_eq!(prepared.len(), 1);
        let summary = &prepared[0];
        assert_eq!(summary.mint, mint);
        assert_eq!(summary.decimals, 6);
        assert!((summary.ui_amount - 1.5).abs() < f64::EPSILON);
        assert!(summary.amount_raw > 0);

        let updated_account = resolved.accounts.get(&token).unwrap();
        use spl_token::state::Account as SplAccount;
        let parsed = SplAccount::unpack(&updated_account.data()[..SplAccount::LEN]).unwrap();
        assert_eq!(parsed.amount, summary.amount_raw);
    }

    #[test]
    fn resolve_raw_amount_passthrough() {
        assert_eq!(resolve_token_amount(&TokenAmount::Raw(42), 6).unwrap(), 42);
    }

    #[test]
    fn resolve_decimal_amount_scales() {
        assert_eq!(resolve_token_amount(&TokenAmount::Decimal(1.5), 6).unwrap(), 1_500_000);
    }

    #[test]
    fn resolve_decimal_rejects_negative() {
        let err = resolve_token_amount(&TokenAmount::Decimal(-1.0), 6).unwrap_err();
        assert!(err.to_string().contains("non-negative"));
    }

    #[test]
    fn raw_to_ui_roundtrip() {
        let ui = raw_to_ui_amount(1_500_000, 6);
        assert!((ui - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn raw_to_ui_zero_decimals() {
        let ui = raw_to_ui_amount(42, 0);
        assert!((ui - 42.0).abs() < f64::EPSILON);
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
