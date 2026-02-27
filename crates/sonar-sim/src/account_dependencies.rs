use std::collections::{HashMap, HashSet};

use solana_account::{AccountSharedData, ReadableAccount};
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;
use spl_token::solana_program::program_pack::Pack;

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
    let owner = *account.owner();
    if owner == spl_token::ID {
        let token_account = spl_token::state::Account::unpack(account.data()).ok()?;
        return Some(Pubkey::new_from_array(token_account.mint.to_bytes()));
    }
    if owner == spl_token_2022::ID {
        use spl_token_2022::extension::StateWithExtensions;
        use spl_token_2022::state::Account as Token2022Account;
        let token_account = StateWithExtensions::<Token2022Account>::unpack(account.data()).ok()?;
        return Some(Pubkey::new_from_array(token_account.base.mint.to_bytes()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn make_token_account(mint: &Pubkey) -> AccountSharedData {
        use spl_token::solana_program::program_option::COption;
        use spl_token::solana_program::pubkey::Pubkey as ProgramPubkey;
        use spl_token::state::{Account as SplAccount, AccountState};

        let owner = Pubkey::new_unique();
        let state = SplAccount {
            mint: ProgramPubkey::new_from_array(mint.to_bytes()),
            owner: ProgramPubkey::new_from_array(owner.to_bytes()),
            amount: 0,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        };
        let mut data = vec![0u8; SplAccount::LEN];
        SplAccount::pack(state, &mut data).unwrap();
        AccountSharedData::from(Account {
            lamports: 1,
            data,
            owner: spl_token::ID,
            executable: false,
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
        accounts.insert(token_key, make_token_account(&mint_key));

        let missing = collect_token_mint_dependencies(&accounts);
        assert_eq!(missing, vec![mint_key]);
    }

    #[test]
    fn token_dependency_collection_skips_loaded_mint() {
        let mint_key = Pubkey::new_unique();
        let token_key = Pubkey::new_unique();

        let mut accounts = HashMap::new();
        accounts.insert(token_key, make_token_account(&mint_key));
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
