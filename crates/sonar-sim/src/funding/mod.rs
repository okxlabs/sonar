mod common;
mod sol;
mod token2022;
mod token_legacy;

pub use sol::apply_sol_fundings;

use std::collections::HashMap;

use solana_account::{AccountSharedData, ReadableAccount};
use solana_pubkey::Pubkey;
use solana_rent::Rent;

use crate::error::{Result, SonarSimError};
use crate::svm_backend::SvmBackend;
use crate::token_decode::{self, TokenProgramKind, ensure_same_program, raw_to_ui_amount};
use crate::types::{
    AccountAppender, PreparedTokenFunding, ResolvedAccounts, TokenAmount, TokenFunding,
};

pub fn prepare_token_fundings(
    loader: &mut dyn AccountAppender,
    resolved: &ResolvedAccounts,
    requests: &[TokenFunding],
) -> Result<Vec<PreparedTokenFunding>> {
    let mut prepared = Vec::new();
    if requests.is_empty() {
        return Ok(prepared);
    }
    let mut extras: HashMap<Pubkey, AccountSharedData> = HashMap::new();

    let total = requests.len();
    for (index, request) in requests.iter().enumerate() {
        log::debug!("Preparing token fundings ({}/{})", index + 1, total);
        let summary = prepare_single_token_funding(loader, resolved, &mut extras, request)
            .map_err(|e| SonarSimError::Token {
                account: Some(request.account),
                reason: format!("Failed to prepare token funding for {}: {e}", request.account),
            })?;
        prepared.push(summary);
    }

    Ok(prepared)
}

fn lookup_account<'a>(
    resolved: &'a ResolvedAccounts,
    extras: &'a HashMap<Pubkey, AccountSharedData>,
    pubkey: &Pubkey,
) -> Option<&'a AccountSharedData> {
    resolved.accounts.get(pubkey).or_else(|| extras.get(pubkey))
}

fn prepare_single_token_funding(
    loader: &mut dyn AccountAppender,
    resolved: &ResolvedAccounts,
    extras: &mut HashMap<Pubkey, AccountSharedData>,
    request: &TokenFunding,
) -> Result<PreparedTokenFunding> {
    ensure_account_loaded(loader, resolved, extras, &request.account).map_err(|e| {
        SonarSimError::Token {
            account: Some(request.account),
            reason: format!(
                "Failed to load token account {} required for funding: {e}",
                request.account
            ),
        }
    })?;

    let (mint, owner) = if let Some(account) = lookup_account(resolved, extras, &request.account) {
        let decoded = decode_existing_token_account(account, &request.account)?;

        if let Some(requested_mint) = request.mint {
            if decoded.mint != requested_mint {
                return Err(SonarSimError::Token {
                    account: Some(request.account),
                    reason: format!(
                        "Token account {} is associated with mint {}, but CLI requested mint {}",
                        request.account, decoded.mint, requested_mint
                    ),
                });
            }
        }

        if let Some(requested_owner) = request.owner {
            if decoded.owner != requested_owner {
                return Err(SonarSimError::Token {
                    account: Some(request.account),
                    reason: format!(
                        "Token account {} has owner {}, but CLI specified owner {}",
                        request.account, decoded.owner, requested_owner
                    ),
                });
            }
        }

        (decoded.mint, decoded.owner)
    } else {
        let mint = request.mint.ok_or_else(|| SonarSimError::Token {
            account: Some(request.account),
            reason: format!(
                "Token account {} does not exist on-chain; \
                 you must specify mint and owner using \
                 <ACCOUNT>:<MINT>:<OWNER>=<AMOUNT> format",
                request.account
            ),
        })?;
        let owner = request.owner.ok_or_else(|| SonarSimError::Token {
            account: Some(request.account),
            reason: format!(
                "Token account {} does not exist on-chain; \
                 you must specify the owner using \
                 <ACCOUNT>:<MINT>:<OWNER>=<AMOUNT> format",
                request.account
            ),
        })?;

        (mint, owner)
    };

    ensure_account_loaded(loader, resolved, extras, &mint).map_err(|e| SonarSimError::Token {
        account: Some(mint),
        reason: format!("Failed to load mint account {}: {e}", mint),
    })?;
    let mint_account = lookup_account(resolved, extras, &mint)
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

    if let Some(account) = lookup_account(resolved, extras, &request.account) {
        ensure_same_program(program_kind, account.owner(), "token account", request.account)?;
    }

    Ok(PreparedTokenFunding {
        account: request.account,
        mint,
        owner,
        decimals,
        amount_raw,
        ui_amount: raw_to_ui_amount(amount_raw, decimals),
        program_kind,
    })
}

pub fn apply_token_fundings<B: SvmBackend + ?Sized>(
    svm: &mut B,
    fundings: &[PreparedTokenFunding],
    resolved: &ResolvedAccounts,
) -> Result<()> {
    for funding in fundings {
        apply_single_token_funding(svm, funding, resolved)?;
    }
    Ok(())
}

fn apply_single_token_funding<B: SvmBackend + ?Sized>(
    svm: &mut B,
    funding: &PreparedTokenFunding,
    resolved: &ResolvedAccounts,
) -> Result<()> {
    let kind = funding.program_kind;

    let mut account = if let Some(existing) = svm.get_account(&funding.account) {
        existing
    } else {
        let rent = read_rent_from_svm(svm)?;
        let mint_account = resolved
            .accounts
            .get(&funding.mint)
            .ok_or(SonarSimError::AccountNotFound { pubkey: funding.mint })?;
        match kind {
            TokenProgramKind::Legacy => token_legacy::build_token_account(
                &funding.account,
                &funding.mint,
                &funding.owner,
                &rent,
            )?,
            TokenProgramKind::Token2022 => token2022::build_token_account_with_extensions(
                &funding.account,
                &funding.mint,
                &funding.owner,
                mint_account,
                &rent,
            )?,
        }
    };

    let _ = match kind {
        TokenProgramKind::Legacy => token_legacy::update_token_balance_in_account(
            &mut account,
            &funding.account,
            &funding.mint,
            &funding.owner,
            funding.amount_raw,
            funding.decimals,
        )?,
        TokenProgramKind::Token2022 => token2022::update_token_balance_in_account(
            &mut account,
            &funding.account,
            &funding.mint,
            &funding.owner,
            funding.amount_raw,
            funding.decimals,
        )?,
    };

    svm.set_account(funding.account, account).map_err(|e| SonarSimError::Svm {
        reason: format!("Failed to set token funding account `{}`: {}", funding.account, e),
    })?;
    Ok(())
}

fn read_rent_from_svm<B: SvmBackend + ?Sized>(svm: &B) -> Result<Rent> {
    let rent_id = solana_sdk_ids::sysvar::rent::id();
    let rent_account = svm.get_account(&rent_id).ok_or_else(|| SonarSimError::Svm {
        reason: "Rent sysvar account not found in SVM".into(),
    })?;
    bincode::deserialize(rent_account.data()).map_err(|e| SonarSimError::Serialization {
        reason: format!("Failed to deserialize Rent sysvar: {e}"),
    })
}

fn decode_existing_token_account(
    account: &AccountSharedData,
    account_pubkey: &Pubkey,
) -> Result<token_decode::DecodedTokenAccount> {
    token_decode::try_decode_token_account(account.data(), account.owner())
        .ok_or_else(|| SonarSimError::Token {
            account: Some(*account_pubkey),
            reason: format!(
                "Token account is not owned by any known SPL Token program or cannot be decoded (owner: {})",
                account.owner()
            ),
        })
}

fn ensure_account_loaded(
    loader: &mut dyn AccountAppender,
    resolved: &ResolvedAccounts,
    extras: &mut HashMap<Pubkey, AccountSharedData>,
    pubkey: &Pubkey,
) -> Result<()> {
    if resolved.accounts.contains_key(pubkey) || extras.contains_key(pubkey) {
        return Ok(());
    }
    let mut temp = ResolvedAccounts { accounts: HashMap::new(), lookups: Vec::new() };
    loader.append_accounts(&mut temp, &[*pubkey])?;
    extras.extend(temp.accounts);
    Ok(())
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

    use litesvm::LiteSVM;
    use solana_account::{Account, AccountSharedData, ReadableAccount};
    use solana_pubkey::Pubkey;
    use spl_token::solana_program::program_option::COption;
    use spl_token::solana_program::program_pack::Pack;
    use spl_token::state::{Account as SplAccount, AccountState, Mint as SplMint};

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
    fn prepares_spl_token_funding_without_mutating_account_data() {
        let mut loader = NoopAppender;
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let (token_account, mint_account) = spl_token_account_and_mint(&mint, &owner);

        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };
        resolved.accounts.insert(token, AccountSharedData::from(token_account));
        resolved.accounts.insert(mint, AccountSharedData::from(mint_account.clone()));

        let funding = TokenFunding {
            account: token,
            mint: Some(mint),
            owner: None,
            amount: TokenAmount::Raw(1_500_000),
        };
        let before_data = resolved.accounts.get(&token).unwrap().data().to_vec();

        let prepared =
            prepare_token_fundings(&mut loader, &resolved, &[funding]).expect("prepares funding");
        assert_eq!(prepared.len(), 1);
        let summary = &prepared[0];
        assert_eq!(summary.mint, mint);
        assert_eq!(summary.decimals, 6);
        assert!((summary.ui_amount - 1.5).abs() < f64::EPSILON);
        assert!(summary.amount_raw > 0);

        let updated_account = resolved.accounts.get(&token).unwrap();
        assert_eq!(updated_account.data(), before_data);

        let mut svm = LiteSVM::new().with_blockhash_check(false).with_sigverify(false);
        svm.set_account(token, Account::from(updated_account.clone())).unwrap();
        svm.set_account(mint, mint_account.clone()).unwrap();

        apply_token_fundings(&mut svm, &prepared, &resolved).expect("applies funding to svm");

        let updated_in_svm = svm.get_account(&token).unwrap();
        let parsed = SplAccount::unpack(&updated_in_svm.data[..SplAccount::LEN]).unwrap();
        assert_eq!(parsed.amount, summary.amount_raw);
    }

    #[test]
    fn prepare_validates_owner_matches_onchain() {
        let mut loader = NoopAppender;
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let wrong_owner = Pubkey::new_unique();

        let (token_account, mint_account) = spl_token_account_and_mint(&mint, &owner);

        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };
        resolved.accounts.insert(token, AccountSharedData::from(token_account));
        resolved.accounts.insert(mint, AccountSharedData::from(mint_account));

        let funding = TokenFunding {
            account: token,
            mint: Some(mint),
            owner: Some(wrong_owner),
            amount: TokenAmount::Raw(100),
        };
        let err = prepare_token_fundings(&mut loader, &resolved, &[funding]).unwrap_err();
        assert!(err.to_string().contains("has owner"));
    }

    #[test]
    fn prepare_accepts_matching_owner() {
        let mut loader = NoopAppender;
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let (token_account, mint_account) = spl_token_account_and_mint(&mint, &owner);

        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };
        resolved.accounts.insert(token, AccountSharedData::from(token_account));
        resolved.accounts.insert(mint, AccountSharedData::from(mint_account));

        let funding = TokenFunding {
            account: token,
            mint: Some(mint),
            owner: Some(owner),
            amount: TokenAmount::Raw(100),
        };
        let prepared =
            prepare_token_fundings(&mut loader, &resolved, &[funding]).expect("prepares");
        assert_eq!(prepared[0].owner, owner);
    }

    #[test]
    fn prepare_reads_owner_from_onchain_account() {
        let mut loader = NoopAppender;
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let (token_account, mint_account) = spl_token_account_and_mint(&mint, &owner);

        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };
        resolved.accounts.insert(token, AccountSharedData::from(token_account));
        resolved.accounts.insert(mint, AccountSharedData::from(mint_account));

        let funding =
            TokenFunding { account: token, mint: None, owner: None, amount: TokenAmount::Raw(100) };
        let prepared =
            prepare_token_fundings(&mut loader, &resolved, &[funding]).expect("prepares");
        assert_eq!(prepared[0].owner, owner);
    }

    #[test]
    fn prepare_errors_when_account_missing_and_no_owner() {
        let mut loader = NoopAppender;
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let (_, mint_account) = spl_token_account_and_mint(&mint, &owner);

        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };
        resolved.accounts.insert(mint, AccountSharedData::from(mint_account));

        let funding = TokenFunding {
            account: token,
            mint: Some(mint),
            owner: None,
            amount: TokenAmount::Raw(100),
        };
        let err = prepare_token_fundings(&mut loader, &resolved, &[funding]).unwrap_err();
        assert!(err.to_string().contains("you must specify the owner"));
    }

    #[test]
    fn prepare_creates_with_explicit_owner() {
        let mut loader = NoopAppender;
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let (_, mint_account) = spl_token_account_and_mint(&mint, &owner);

        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };
        resolved.accounts.insert(mint, AccountSharedData::from(mint_account));

        let funding = TokenFunding {
            account: token,
            mint: Some(mint),
            owner: Some(owner),
            amount: TokenAmount::Raw(100),
        };
        let prepared =
            prepare_token_fundings(&mut loader, &resolved, &[funding]).expect("prepares");
        assert_eq!(prepared[0].owner, owner);
        assert_eq!(prepared[0].mint, mint);
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

    #[test]
    fn apply_token_funding_creates_rent_exempt_account() {
        let mut loader = NoopAppender;
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let (_, mint_account) = spl_token_account_and_mint(&mint, &owner);
        let mint_shared = AccountSharedData::from(mint_account.clone());

        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };
        resolved.accounts.insert(mint, mint_shared.clone());

        let funding = TokenFunding {
            account: token,
            mint: Some(mint),
            owner: Some(owner),
            amount: TokenAmount::Raw(1_500_000),
        };
        let prepared =
            prepare_token_fundings(&mut loader, &resolved, &[funding]).expect("prepares funding");

        let mut svm = LiteSVM::new().with_blockhash_check(false).with_sigverify(false);
        svm.set_account(mint, mint_account).unwrap();

        let rent = read_rent_from_svm(&svm).expect("Rent sysvar should exist");
        let expected = rent.minimum_balance(SplAccount::LEN);

        apply_token_fundings(&mut svm, &prepared, &resolved).expect("applies funding to svm");

        let created = svm.get_account(&token).expect("token account should be created");
        assert_eq!(created.lamports, expected);
    }

    #[test]
    fn apply_token_funding_creates_account_with_correct_owner() {
        let mut loader = NoopAppender;
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let (_, mint_account) = spl_token_account_and_mint(&mint, &owner);
        let mint_shared = AccountSharedData::from(mint_account.clone());

        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };
        resolved.accounts.insert(mint, mint_shared);

        let funding = TokenFunding {
            account: token,
            mint: Some(mint),
            owner: Some(owner),
            amount: TokenAmount::Raw(1_000_000),
        };
        let prepared =
            prepare_token_fundings(&mut loader, &resolved, &[funding]).expect("prepares funding");

        let mut svm = LiteSVM::new().with_blockhash_check(false).with_sigverify(false);
        svm.set_account(mint, mint_account).unwrap();

        apply_token_fundings(&mut svm, &prepared, &resolved).expect("applies funding to svm");

        let created = svm.get_account(&token).expect("token account should be created");
        let parsed = SplAccount::unpack(&created.data[..SplAccount::LEN]).unwrap();
        assert_eq!(crate::token_decode::to_pubkey(&parsed.owner), owner);
        assert_eq!(parsed.amount, 1_000_000);
    }

    fn spl_token_account_and_mint(mint: &Pubkey, owner: &Pubkey) -> (Account, Account) {
        let token_state = SplAccount {
            mint: crate::token_decode::to_program_pubkey(mint),
            owner: crate::token_decode::to_program_pubkey(owner),
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
            lamports: 2_039_280,
            data: token_data,
            owner: legacy_program_id(),
            executable: false,
            rent_epoch: 0,
        };
        let mint_account = Account {
            lamports: 1_461_600,
            data: mint_data,
            owner: legacy_program_id(),
            executable: false,
            rent_epoch: 0,
        };
        (token_account, mint_account)
    }
}
