use std::collections::{HashMap, HashSet};

use solana_account::{AccountSharedData, ReadableAccount};
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;

use crate::token_decode;

/// Collect missing ProgramData accounts for BPF Upgradeable program accounts.
pub(crate) fn collect_bpf_upgradeable_programdata_dependencies(
    accounts: &HashMap<Pubkey, AccountSharedData>,
) -> Vec<Pubkey> {
    let mut missing = Vec::new();
    for account in accounts.values() {
        if *account.owner() != bpf_loader_upgradeable::id() {
            continue;
        }
        if let Ok(UpgradeableLoaderState::Program { programdata_address }) =
            bincode::deserialize::<UpgradeableLoaderState>(account.data())
        {
            let key = Pubkey::new_from_array(programdata_address.to_bytes());
            if !accounts.contains_key(&key) {
                missing.push(key);
            }
        }
    }
    missing
}

/// Collect missing mint accounts referenced by SPL Token / Token-2022 accounts.
pub(crate) fn collect_token_mint_dependencies(
    accounts: &HashMap<Pubkey, AccountSharedData>,
) -> Vec<Pubkey> {
    let mut missing = Vec::new();
    let mut seen = HashSet::new();
    for account in accounts.values() {
        if let Some(mint) = token_account_mint(account) {
            if !accounts.contains_key(&mint) && seen.insert(mint) {
                missing.push(mint);
            }
        }
    }
    missing
}

fn token_account_mint(account: &AccountSharedData) -> Option<Pubkey> {
    token_decode::try_decode_token_account(account.data(), account.owner())
        .map(|decoded| decoded.mint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_token_account_shared;
    use solana_account::Account;

    fn make_bpf_program(programdata_address: &Pubkey) -> AccountSharedData {
        let state = UpgradeableLoaderState::Program { programdata_address: *programdata_address };
        let data = bincode::serialize(&state).unwrap();
        AccountSharedData::from(Account {
            lamports: 1,
            data,
            owner: bpf_loader_upgradeable::id(),
            executable: true,
            rent_epoch: 0,
        })
    }

    #[test]
    fn bpf_dependency_collection_finds_missing_programdata() {
        let program_key = Pubkey::new_unique();
        let programdata_key = Pubkey::new_unique();

        let mut accounts = HashMap::new();
        accounts.insert(program_key, make_bpf_program(&programdata_key));

        let missing = collect_bpf_upgradeable_programdata_dependencies(&accounts);
        assert_eq!(missing, vec![programdata_key]);
    }

    #[test]
    fn bpf_dependency_collection_skips_already_loaded() {
        let program_key = Pubkey::new_unique();
        let programdata_key = Pubkey::new_unique();

        let mut accounts = HashMap::new();
        accounts.insert(program_key, make_bpf_program(&programdata_key));
        accounts.insert(
            programdata_key,
            AccountSharedData::from(Account {
                lamports: 1,
                data: vec![],
                owner: bpf_loader_upgradeable::id(),
                executable: false,
                rent_epoch: 0,
            }),
        );

        let missing = collect_bpf_upgradeable_programdata_dependencies(&accounts);
        assert!(missing.is_empty());
    }

    #[test]
    fn token_dependency_collection_finds_missing_mint() {
        let mint_key = Pubkey::new_unique();
        let token_key = Pubkey::new_unique();

        let mut accounts = HashMap::new();
        accounts.insert(token_key, make_token_account_shared(&mint_key));

        let missing = collect_token_mint_dependencies(&accounts);
        assert_eq!(missing, vec![mint_key]);
    }

    #[test]
    fn token_dependency_collection_skips_loaded_mint() {
        let mint_key = Pubkey::new_unique();
        let token_key = Pubkey::new_unique();

        let mut accounts = HashMap::new();
        accounts.insert(token_key, make_token_account_shared(&mint_key));
        accounts.insert(
            mint_key,
            AccountSharedData::from(Account {
                lamports: 1,
                data: vec![],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            }),
        );

        let missing = collect_token_mint_dependencies(&accounts);
        assert!(missing.is_empty());
    }
}
