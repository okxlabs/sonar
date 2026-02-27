//! Balance change calculation module for SOL and Token accounts.
//!
//! This module computes balance differences between pre-simulation and post-simulation
//! account states, supporting both single transaction and bundle simulation modes.

use std::collections::HashMap;

use serde::Serialize;
use solana_account::{AccountSharedData, ReadableAccount};
use solana_pubkey::Pubkey;

use crate::token_decode;

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
    pub owner: Pubkey,
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
    pre_accounts: &HashMap<Pubkey, AccountSharedData>,
    post_accounts: &HashMap<Pubkey, AccountSharedData>,
) -> Vec<SolBalanceChange> {
    let mut changes = Vec::new();

    for (pubkey, post_account) in post_accounts {
        let after = post_account.lamports();
        let before = pre_accounts.get(pubkey).map(|acc| acc.lamports()).unwrap_or(0);

        let change = after as i128 - before as i128;
        if change != 0 {
            changes.push(SolBalanceChange { account: *pubkey, before, after, change });
        }
    }

    // Accounts that existed before but are missing after (closed/reclaimed)
    for (pubkey, pre_account) in pre_accounts {
        if !post_accounts.contains_key(pubkey) {
            let before = pre_account.lamports();
            if before != 0 {
                changes.push(SolBalanceChange {
                    account: *pubkey,
                    before,
                    after: 0,
                    change: -(before as i128),
                });
            }
        }
    }

    changes.sort_by(|a, b| b.change.abs().cmp(&a.change.abs()));
    changes
}

/// Compute token balance changes between pre and post account states.
///
/// Only token accounts with actual balance changes (change != 0) are included.
/// Uses the shared `token_decode` decoder for both SPL Token and Token-2022.
pub fn compute_token_changes(
    pre_accounts: &HashMap<Pubkey, AccountSharedData>,
    post_accounts: &HashMap<Pubkey, AccountSharedData>,
    mint_decimals: &HashMap<Pubkey, u8>,
) -> Vec<TokenBalanceChange> {
    let mut changes = Vec::new();

    for (pubkey, post_account) in post_accounts {
        let post_owner = post_account.owner();
        if let Some(decoded) =
            token_decode::try_decode_token_account(post_account.data(), post_owner)
        {
            let before_amount = pre_accounts
                .get(pubkey)
                .and_then(|acc| token_decode::try_decode_token_account(acc.data(), acc.owner()))
                .map(|d| d.amount)
                .unwrap_or(0);

            let change = decoded.amount as i128 - before_amount as i128;
            if change != 0 {
                let decimals = mint_decimals.get(&decoded.mint).copied().unwrap_or(0);
                changes.push(TokenBalanceChange {
                    account: *pubkey,
                    owner: decoded.owner,
                    mint: decoded.mint,
                    before: before_amount,
                    after: decoded.amount,
                    change,
                    decimals,
                });
            }
        }
    }

    // Token accounts that existed before but are missing after (closed)
    for (pubkey, pre_account) in pre_accounts {
        if !post_accounts.contains_key(pubkey) {
            if let Some(decoded) =
                token_decode::try_decode_token_account(pre_account.data(), pre_account.owner())
            {
                if decoded.amount != 0 {
                    let decimals = mint_decimals.get(&decoded.mint).copied().unwrap_or(0);
                    changes.push(TokenBalanceChange {
                        account: *pubkey,
                        owner: decoded.owner,
                        mint: decoded.mint,
                        before: decoded.amount,
                        after: 0,
                        change: -(decoded.amount as i128),
                        decimals,
                    });
                }
            }
        }
    }

    changes.sort_by(|a, b| b.change.abs().cmp(&a.change.abs()));
    changes
}

/// Extract mint decimals from both pre and post accounts.
///
/// This ensures we capture mint decimals even if the mint was not in the original resolved accounts.
pub fn extract_mint_decimals_combined(
    pre_accounts: &HashMap<Pubkey, AccountSharedData>,
    post_accounts: &HashMap<Pubkey, AccountSharedData>,
) -> HashMap<Pubkey, u8> {
    let mut decimals = HashMap::new();

    for (pubkey, account) in pre_accounts {
        if let Some(d) = token_decode::try_read_mint_decimals(account.data(), account.owner()) {
            decimals.insert(*pubkey, d);
        }
    }

    for (pubkey, account) in post_accounts {
        if let Some(d) = token_decode::try_read_mint_decimals(account.data(), account.owner()) {
            decimals.insert(*pubkey, d);
        }
    }

    decimals
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_account::Account;
    use solana_sdk_ids::system_program;
    use spl_token::solana_program::program_option::COption;
    use spl_token::solana_program::program_pack::Pack;
    use spl_token::solana_program::pubkey::Pubkey as ProgramPubkey;
    use spl_token::state::{Account as SplTokenAccount, AccountState};

    fn create_shared_account_with_lamports(lamports: u64) -> AccountSharedData {
        AccountSharedData::from(Account {
            lamports,
            data: vec![],
            owner: system_program::id(),
            executable: false,
            rent_epoch: 0,
        })
    }

    fn create_token_account(
        mint: &Pubkey,
        token_owner: &Pubkey,
        amount: u64,
        lamports: u64,
    ) -> AccountSharedData {
        let state = SplTokenAccount {
            mint: ProgramPubkey::new_from_array(mint.to_bytes()),
            owner: ProgramPubkey::new_from_array(token_owner.to_bytes()),
            amount,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        };
        let mut data = vec![0u8; SplTokenAccount::LEN];
        SplTokenAccount::pack(state, &mut data).unwrap();
        AccountSharedData::from(Account {
            lamports,
            data,
            owner: Pubkey::new_from_array(spl_token::ID.to_bytes()),
            executable: false,
            rent_epoch: 0,
        })
    }

    #[test]
    fn test_compute_sol_changes_with_increase() {
        let pubkey = Pubkey::new_unique();

        let mut pre = HashMap::new();
        pre.insert(pubkey, create_shared_account_with_lamports(1_000_000_000));

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
        pre.insert(pubkey, create_shared_account_with_lamports(2_000_000_000));

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
        pre.insert(pubkey, create_shared_account_with_lamports(1_000_000_000));

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

    #[test]
    fn test_compute_sol_changes_account_closed() {
        let pubkey = Pubkey::new_unique();

        let mut pre = HashMap::new();
        pre.insert(pubkey, create_shared_account_with_lamports(1_000_000_000));

        let post = HashMap::new();

        let changes = compute_sol_changes(&pre, &post);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].account, pubkey);
        assert_eq!(changes[0].before, 1_000_000_000);
        assert_eq!(changes[0].after, 0);
        assert_eq!(changes[0].change, -1_000_000_000);
    }

    #[test]
    fn test_compute_token_changes_account_closed() {
        let mint = Pubkey::new_unique();
        let token_owner = Pubkey::new_unique();
        let token_account_pubkey = Pubkey::new_unique();

        let mut pre = HashMap::new();
        pre.insert(token_account_pubkey, create_token_account(&mint, &token_owner, 100, 2_039_280));

        let post = HashMap::new();

        let mut mint_decimals = HashMap::new();
        mint_decimals.insert(mint, 6u8);

        let changes = compute_token_changes(&pre, &post, &mint_decimals);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].account, token_account_pubkey);
        assert_eq!(changes[0].owner, token_owner);
        assert_eq!(changes[0].mint, mint);
        assert_eq!(changes[0].before, 100);
        assert_eq!(changes[0].after, 0);
        assert_eq!(changes[0].change, -100);
        assert_eq!(changes[0].decimals, 6);
    }
}
