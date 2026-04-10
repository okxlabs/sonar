//! Shared token decoding utilities for SPL Token (legacy) and Token-2022.
//!
//! Both programs share an identical base memory layout for Mint (first 82
//! bytes) and Account (first 165 bytes).  All decoding in this module
//! operates on that shared base layout via `spl_token::state`, so adding
//! a new compatible token program only requires extending
//! [`TokenProgramKind`] — no new decode paths are needed.

use std::fmt;

use solana_account::ReadableAccount;
use solana_pubkey::Pubkey;
use spl_token::solana_program::program_pack::Pack;
use spl_token::state::{Account as SplTokenAccount, Mint as SplMint};

use crate::error::{Result, SonarSimError};

// ── Pubkey conversion helpers ──
//
// SPL Token crates re-export `solana_program::pubkey::Pubkey` which is a
// different type than `solana_pubkey::Pubkey` (same layout, different crate).
// These helpers bridge the gap without the noisy `new_from_array(..to_bytes())`.

/// Convert from SPL's `Pubkey` to `solana_pubkey::Pubkey`.
#[inline]
pub(crate) fn to_pubkey(p: &spl_token::solana_program::pubkey::Pubkey) -> Pubkey {
    Pubkey::new_from_array(p.to_bytes())
}

/// Convert from `solana_pubkey::Pubkey` to SPL's `Pubkey`.
#[inline]
pub(crate) fn to_program_pubkey(p: &Pubkey) -> spl_token::solana_program::pubkey::Pubkey {
    spl_token::solana_program::pubkey::Pubkey::new_from_array(p.to_bytes())
}

// ── Program identification ──

/// Known SPL-compatible token programs.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TokenProgramKind {
    Legacy,
    Token2022,
}

impl TokenProgramKind {
    pub fn from_owner(owner: &Pubkey) -> Option<Self> {
        if *owner == legacy_program_id() {
            Some(Self::Legacy)
        } else if *owner == token2022_program_id() {
            Some(Self::Token2022)
        } else {
            None
        }
    }

    pub fn program_id(&self) -> Pubkey {
        match self {
            Self::Legacy => legacy_program_id(),
            Self::Token2022 => token2022_program_id(),
        }
    }

    pub fn program_name(&self) -> &'static str {
        match self {
            Self::Legacy => "SPL Token",
            Self::Token2022 => "SPL Token 2022",
        }
    }
}

impl fmt::Display for TokenProgramKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.program_name())
    }
}

pub(crate) fn legacy_program_id() -> Pubkey {
    to_pubkey(&spl_token::ID)
}

pub(crate) fn token2022_program_id() -> Pubkey {
    to_pubkey(&spl_token_2022::ID)
}

// ── Mint decimals ──

/// Read the `decimals` field from a mint account.
///
/// Returns an error when the account is not owned by a known token program
/// or the data is malformed.  Both legacy and Token-2022 mints share the
/// same base layout so a single unpack path handles both.
pub fn read_mint_decimals(account: &impl ReadableAccount) -> Result<u8> {
    if TokenProgramKind::from_owner(account.owner()).is_none() {
        return Err(SonarSimError::Token {
            account: None,
            reason: format!(
                "Mint account is not owned by any known SPL Token program (owner: {})",
                account.owner()
            ),
        });
    }

    let data = account.data();
    if data.len() < SplMint::LEN {
        return Err(SonarSimError::Token {
            account: None,
            reason: format!(
                "Mint account data is smaller than expected: {} < {}",
                data.len(),
                SplMint::LEN
            ),
        });
    }
    let parsed = SplMint::unpack(&data[..SplMint::LEN]).map_err(|err| SonarSimError::Token {
        account: None,
        reason: format!("Failed to unpack mint account: {err}"),
    })?;
    Ok(parsed.decimals)
}

/// Try to extract decimals from raw account data + owner.
///
/// Returns `None` when the owner is not a token program, the data is too
/// short, or the mint is not initialized.  Useful for scanning accounts
/// without failing on non-mint data.
pub(crate) fn try_read_mint_decimals(data: &[u8], owner: &Pubkey) -> Option<u8> {
    TokenProgramKind::from_owner(owner)?;
    if data.len() < SplMint::LEN {
        return None;
    }
    let mint = SplMint::unpack(&data[..SplMint::LEN]).ok()?;
    mint.is_initialized.then_some(mint.decimals)
}

// ── Token account decoding ──

/// Decoded fields from an SPL token account (works for both legacy and 2022).
#[derive(Debug, Clone, Copy)]
pub struct DecodedTokenAccount {
    pub mint: Pubkey,
    pub owner: Pubkey,
    pub amount: u64,
}

/// Attempt to decode raw account data as a token account.
///
/// Returns `None` when the owner is not a known token program or the data
/// cannot be parsed.  The first 165 bytes are identical across legacy and
/// Token-2022, so a single unpack path handles both.
pub fn try_decode_token_account(
    data: &[u8],
    program_owner: &Pubkey,
) -> Option<DecodedTokenAccount> {
    TokenProgramKind::from_owner(program_owner)?;
    if data.len() < SplTokenAccount::LEN {
        return None;
    }
    let parsed = SplTokenAccount::unpack(&data[..SplTokenAccount::LEN]).ok()?;
    Some(DecodedTokenAccount {
        mint: to_pubkey(&parsed.mint),
        owner: to_pubkey(&parsed.owner),
        amount: parsed.amount,
    })
}

// ── Amount conversion ──

pub(crate) fn raw_to_ui_amount(amount_raw: u64, decimals: u8) -> f64 {
    let factor = 10f64.powi(decimals as i32);
    if factor == 0.0 { amount_raw as f64 } else { (amount_raw as f64) / factor }
}

// ── Program ownership check ──

pub(crate) fn ensure_same_program(
    kind: TokenProgramKind,
    owner: &Pubkey,
    label: &str,
    account: Pubkey,
) -> Result<()> {
    if owner != &kind.program_id() {
        return Err(SonarSimError::Token {
            account: Some(account),
            reason: format!("Provided {label} is not owned by {}", kind.program_name()),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_token_account_data;
    use solana_account::Account;
    use spl_token::solana_program::program_option::COption;

    fn make_mint(decimals: u8, owner: Pubkey) -> Account {
        let state = SplMint {
            mint_authority: COption::None,
            supply: 0,
            decimals,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        let mut data = vec![0u8; SplMint::LEN];
        SplMint::pack(state, &mut data).unwrap();
        Account { lamports: 0, data, owner, executable: false, rent_epoch: 0 }
    }

    #[test]
    fn kind_from_owner_legacy() {
        assert_eq!(
            TokenProgramKind::from_owner(&legacy_program_id()),
            Some(TokenProgramKind::Legacy)
        );
    }

    #[test]
    fn kind_from_owner_2022() {
        assert_eq!(
            TokenProgramKind::from_owner(&token2022_program_id()),
            Some(TokenProgramKind::Token2022)
        );
    }

    #[test]
    fn kind_from_owner_unknown() {
        assert_eq!(TokenProgramKind::from_owner(&Pubkey::new_unique()), None);
    }

    #[test]
    fn token_program_kind_display() {
        assert_eq!(TokenProgramKind::Legacy.to_string(), "SPL Token");
        assert_eq!(TokenProgramKind::Token2022.to_string(), "SPL Token 2022");
    }

    #[test]
    fn read_mint_decimals_legacy_ok() {
        assert_eq!(read_mint_decimals(&make_mint(9, legacy_program_id())).unwrap(), 9);
    }

    #[test]
    fn read_mint_decimals_2022_ok() {
        assert_eq!(read_mint_decimals(&make_mint(6, token2022_program_id())).unwrap(), 6);
    }

    #[test]
    fn read_mint_decimals_rejects_unknown_owner() {
        let account = Account {
            lamports: 0,
            data: vec![0u8; 82],
            owner: Pubkey::new_unique(),
            executable: false,
            rent_epoch: 0,
        };
        let err = read_mint_decimals(&account).unwrap_err();
        assert!(err.to_string().contains("not owned by"));
    }

    #[test]
    fn read_mint_decimals_rejects_short_data() {
        let account = Account {
            lamports: 0,
            data: vec![0u8; 10],
            owner: legacy_program_id(),
            executable: false,
            rent_epoch: 0,
        };
        let err = read_mint_decimals(&account).unwrap_err();
        assert!(err.to_string().contains("smaller than expected"));
    }

    #[test]
    fn try_read_mint_decimals_legacy() {
        let account = make_mint(8, legacy_program_id());
        assert_eq!(try_read_mint_decimals(&account.data, &account.owner), Some(8));
    }

    #[test]
    fn try_read_mint_decimals_2022() {
        let account = make_mint(6, token2022_program_id());
        assert_eq!(try_read_mint_decimals(&account.data, &account.owner), Some(6));
    }

    #[test]
    fn try_read_mint_decimals_unknown_owner_returns_none() {
        assert_eq!(try_read_mint_decimals(&[0u8; 82], &Pubkey::new_unique()), None);
    }

    #[test]
    fn try_read_mint_decimals_short_data_returns_none() {
        assert_eq!(try_read_mint_decimals(&[0u8; 10], &legacy_program_id()), None);
    }

    #[test]
    fn decode_token_account_legacy() {
        let mint = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let data = make_token_account_data(&mint, &owner, 42);

        let decoded = try_decode_token_account(&data, &legacy_program_id()).unwrap();
        assert_eq!(decoded.mint, mint);
        assert_eq!(decoded.owner, owner);
        assert_eq!(decoded.amount, 42);
    }

    #[test]
    fn decode_token_account_2022() {
        let mint = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let data = make_token_account_data(&mint, &owner, 100);

        let decoded = try_decode_token_account(&data, &token2022_program_id()).unwrap();
        assert_eq!(decoded.mint, mint);
        assert_eq!(decoded.owner, owner);
        assert_eq!(decoded.amount, 100);
    }

    #[test]
    fn decode_token_account_unknown_owner_returns_none() {
        assert!(try_decode_token_account(&[0u8; 165], &Pubkey::new_unique()).is_none());
    }

    #[test]
    fn raw_to_ui_amount_basic() {
        assert!((raw_to_ui_amount(1_500_000, 6) - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn raw_to_ui_amount_zero_decimals() {
        assert!((raw_to_ui_amount(42, 0) - 42.0).abs() < f64::EPSILON);
    }
}
