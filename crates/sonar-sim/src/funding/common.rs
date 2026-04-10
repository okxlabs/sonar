use solana_account::{AccountSharedData, ReadableAccount, WritableAccount};
use solana_pubkey::Pubkey;
use spl_token::solana_program::program_pack::Pack;

use crate::error::{Result, SonarSimError};
use crate::token_decode::{TokenProgramKind, ensure_same_program, raw_to_ui_amount};
use crate::types::PreparedTokenFunding;

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

pub(super) fn update_token_amount_account<T: TokenAmountMut>(
    account: &mut AccountSharedData,
    account_pubkey: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    amount_raw: u64,
    decimals: u8,
    program_kind: TokenProgramKind,
) -> Result<PreparedTokenFunding> {
    ensure_same_program(program_kind, account.owner(), "token account", *account_pubkey)?;
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
        owner: *owner,
        decimals,
        amount_raw,
        ui_amount: raw_to_ui_amount(amount_raw, decimals),
        program_kind,
    })
}
