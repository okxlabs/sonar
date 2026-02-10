//! Balance change calculation module for SOL and Token accounts.
//!
//! This module computes balance differences between pre-simulation and post-simulation
//! account states, supporting both single transaction and bundle simulation modes.

use std::collections::HashMap;

use serde::Serialize;
use solana_account::{Account, AccountSharedData, ReadableAccount};
use solana_pubkey::Pubkey;

/// SOL balance change for a single account.
#[derive(Debug, Clone, Serialize)]
pub struct SolBalanceChange {
    pub account: Pubkey,
    pub before: u64,
    pub after: u64,
    pub change: i128,
}

/// Token balance change for a single token account.
#[derive(Debug, Clone, Serialize)]
pub struct TokenBalanceChange {
    pub account: Pubkey,
    pub mint: Pubkey,
    pub before: u64,
    pub after: u64,
    pub change: i128,
    pub decimals: u8,
}

/// Compute SOL balance changes between pre and post account states.
///
/// Only accounts with actual balance changes (change != 0) are included.
pub fn compute_sol_changes(
    pre_accounts: &HashMap<Pubkey, Account>,
    post_accounts: &HashMap<Pubkey, AccountSharedData>,
) -> Vec<SolBalanceChange> {
    let mut changes = Vec::new();

    for (pubkey, post_account) in post_accounts {
        let after = post_account.lamports();
        let before = pre_accounts.get(pubkey).map(|acc| acc.lamports).unwrap_or(0);

        let change = after as i128 - before as i128;
        if change != 0 {
            changes.push(SolBalanceChange { account: *pubkey, before, after, change });
        }
    }

    // Sort by absolute change descending for better readability
    changes.sort_by(|a, b| b.change.abs().cmp(&a.change.abs()));
    changes
}

/// Compute token balance changes between pre and post account states.
///
/// Only token accounts with actual balance changes (change != 0) are included.
/// Uses SPL Token and Token-2022 decoding to extract token amounts.
pub fn compute_token_changes(
    pre_accounts: &HashMap<Pubkey, Account>,
    post_accounts: &HashMap<Pubkey, AccountSharedData>,
    mint_decimals: &HashMap<Pubkey, u8>,
) -> Vec<TokenBalanceChange> {
    let mut changes = Vec::new();

    for (pubkey, post_account) in post_accounts {
        let post_owner = post_account.owner();
        // Try to decode as token account
        if let Some((mint, after_amount)) = try_decode_token_account(post_account.data(), &post_owner)
        {
            let before_amount = pre_accounts
                .get(pubkey)
                .and_then(|acc| try_decode_token_account(&acc.data, &acc.owner))
                .map(|(_, amount)| amount)
                .unwrap_or(0);

            let change = after_amount as i128 - before_amount as i128;
            if change != 0 {
                let decimals = mint_decimals.get(&mint).copied().unwrap_or(0);
                changes.push(TokenBalanceChange {
                    account: *pubkey,
                    mint,
                    before: before_amount,
                    after: after_amount,
                    change,
                    decimals,
                });
            }
        }
    }

    // Sort by absolute change descending
    changes.sort_by(|a, b| b.change.abs().cmp(&a.change.abs()));
    changes
}

/// Extract mint decimals from both pre and post accounts.
///
/// This ensures we capture mint decimals even if the mint was not in the original resolved accounts.
pub fn extract_mint_decimals_combined(
    pre_accounts: &HashMap<Pubkey, Account>,
    post_accounts: &HashMap<Pubkey, AccountSharedData>,
) -> HashMap<Pubkey, u8> {
    let mut decimals = HashMap::new();

    // Extract from pre accounts
    extract_mint_decimals_from_accounts(pre_accounts, &mut decimals);

    // Extract from post accounts
    extract_mint_decimals_from_shared_accounts(post_accounts, &mut decimals);

    decimals
}

fn extract_mint_decimals_from_accounts(
    accounts: &HashMap<Pubkey, Account>,
    decimals: &mut HashMap<Pubkey, u8>,
) {
    use spl_token::solana_program::program_pack::Pack;

    let token_program = spl_token::ID;
    let token2022_program = spl_token_2022::ID;

    for (pubkey, account) in accounts {
        let owner_bytes = account.owner.to_bytes();
        if owner_bytes != token_program.to_bytes() && owner_bytes != token2022_program.to_bytes() {
            continue;
        }

        // Try to decode as mint (legacy)
        if account.data.len() >= spl_token::state::Mint::LEN {
            if let Ok(mint) = spl_token::state::Mint::unpack(&account.data) {
                if mint.is_initialized {
                    decimals.insert(*pubkey, mint.decimals);
                    continue;
                }
            }
        }

        // Try to decode as Token-2022 mint
        use spl_token_2022::extension::StateWithExtensions;
        use spl_token_2022::state::Mint as Token2022Mint;
        if let Ok(state) = StateWithExtensions::<Token2022Mint>::unpack(&account.data) {
            if state.base.is_initialized {
                decimals.insert(*pubkey, state.base.decimals);
            }
        }
    }
}

fn extract_mint_decimals_from_shared_accounts(
    accounts: &HashMap<Pubkey, AccountSharedData>,
    decimals: &mut HashMap<Pubkey, u8>,
) {
    use spl_token::solana_program::program_pack::Pack;

    let token_program = spl_token::ID;
    let token2022_program = spl_token_2022::ID;

    for (pubkey, account) in accounts {
        let owner_bytes = account.owner().to_bytes();
        if owner_bytes != token_program.to_bytes() && owner_bytes != token2022_program.to_bytes() {
            continue;
        }

        let data = account.data();

        // Try to decode as mint (legacy)
        if data.len() >= spl_token::state::Mint::LEN {
            if let Ok(mint) = spl_token::state::Mint::unpack(data) {
                if mint.is_initialized {
                    decimals.insert(*pubkey, mint.decimals);
                    continue;
                }
            }
        }

        // Try to decode as Token-2022 mint
        use spl_token_2022::extension::StateWithExtensions;
        use spl_token_2022::state::Mint as Token2022Mint;
        if let Ok(state) = StateWithExtensions::<Token2022Mint>::unpack(data) {
            if state.base.is_initialized {
                decimals.insert(*pubkey, state.base.decimals);
            }
        }
    }
}

/// Try to decode account data as a token account, returning (mint, amount) if successful.
fn try_decode_token_account(data: &[u8], owner: &Pubkey) -> Option<(Pubkey, u64)> {
    use spl_token::solana_program::program_pack::Pack;
    use spl_token::state::Account as TokenAccount;

    if *owner == spl_token::ID {
        // Legacy token account
        if data.len() < TokenAccount::LEN {
            return None;
        }
        if let Ok(token_account) = TokenAccount::unpack(data) {
            let mint = Pubkey::new_from_array(token_account.mint.to_bytes());
            return Some((mint, token_account.amount));
        }
    } else if *owner == spl_token_2022::ID {
        // Token-2022 account
        use spl_token_2022::extension::StateWithExtensions;
        use spl_token_2022::state::Account as Token2022Account;
        if let Ok(state) = StateWithExtensions::<Token2022Account>::unpack(data) {
            let mint = Pubkey::new_from_array(state.base.mint.to_bytes());
            return Some((mint, state.base.amount));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk_ids::system_program;

    fn create_account_with_lamports(lamports: u64) -> Account {
        Account {
            lamports,
            data: vec![],
            owner: system_program::id(),
            executable: false,
            rent_epoch: 0,
        }
    }

    fn create_shared_account_with_lamports(lamports: u64) -> AccountSharedData {
        AccountSharedData::from(create_account_with_lamports(lamports))
    }

    #[test]
    fn test_compute_sol_changes_with_increase() {
        let pubkey = Pubkey::new_unique();

        let mut pre = HashMap::new();
        pre.insert(pubkey, create_account_with_lamports(1_000_000_000));

        let mut post = HashMap::new();
        post.insert(pubkey, create_shared_account_with_lamports(2_000_000_000));

        let changes = compute_sol_changes(&pre, &post);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].account, pubkey);
        assert_eq!(changes[0].before, 1_000_000_000);
        assert_eq!(changes[0].after, 2_000_000_000);
        assert_eq!(changes[0].change, 1_000_000_000);
    }

    #[test]
    fn test_compute_sol_changes_with_decrease() {
        let pubkey = Pubkey::new_unique();

        let mut pre = HashMap::new();
        pre.insert(pubkey, create_account_with_lamports(2_000_000_000));

        let mut post = HashMap::new();
        post.insert(pubkey, create_shared_account_with_lamports(1_000_000_000));

        let changes = compute_sol_changes(&pre, &post);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].change, -1_000_000_000);
    }

    #[test]
    fn test_compute_sol_changes_no_change() {
        let pubkey = Pubkey::new_unique();

        let mut pre = HashMap::new();
        pre.insert(pubkey, create_account_with_lamports(1_000_000_000));

        let mut post = HashMap::new();
        post.insert(pubkey, create_shared_account_with_lamports(1_000_000_000));

        let changes = compute_sol_changes(&pre, &post);

        assert!(changes.is_empty());
    }

    #[test]
    fn test_compute_sol_changes_new_account() {
        let pubkey = Pubkey::new_unique();

        let pre = HashMap::new();

        let mut post = HashMap::new();
        post.insert(pubkey, create_shared_account_with_lamports(1_000_000_000));

        let changes = compute_sol_changes(&pre, &post);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].before, 0);
        assert_eq!(changes[0].after, 1_000_000_000);
        assert_eq!(changes[0].change, 1_000_000_000);
    }
}
