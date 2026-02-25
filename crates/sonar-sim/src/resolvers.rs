use std::collections::HashMap;

use solana_account::{AccountSharedData, ReadableAccount};
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;
use spl_token::solana_program::program_pack::Pack;

/// Inspects already-loaded accounts and yields additional [`Pubkey`]s that
/// must be fetched for a correct simulation.
///
/// Each resolver encapsulates a single protocol-specific dependency rule
/// (e.g. "BPF Upgradeable programs need their ProgramData account").
pub trait AccountDependencyResolver: Send + Sync {
    fn resolve_dependencies(&self, accounts: &HashMap<Pubkey, AccountSharedData>) -> Vec<Pubkey>;
}

/// Identifies ProgramData accounts required by BPF Loader Upgradeable programs.
pub struct BpfUpgradeableResolver;

impl AccountDependencyResolver for BpfUpgradeableResolver {
    fn resolve_dependencies(&self, accounts: &HashMap<Pubkey, AccountSharedData>) -> Vec<Pubkey> {
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
}

/// Identifies missing mint accounts for SPL Token and Token-2022 token accounts.
pub struct TokenMintResolver;

impl AccountDependencyResolver for TokenMintResolver {
    fn resolve_dependencies(&self, accounts: &HashMap<Pubkey, AccountSharedData>) -> Vec<Pubkey> {
        let mut missing = Vec::new();
        for account in accounts.values() {
            if let Some(mint) = token_account_mint(account) {
                if !accounts.contains_key(&mint) && !missing.contains(&mint) {
                    missing.push(mint);
                }
            }
        }
        missing
    }
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

/// Returns the default set of dependency resolvers used by [`AccountLoader`](crate::account_loader::AccountLoader).
pub fn default_resolvers() -> Vec<Box<dyn AccountDependencyResolver>> {
    vec![Box::new(BpfUpgradeableResolver), Box::new(TokenMintResolver)]
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
        let mut data = vec![0u8; spl_token::state::Account::LEN];
        data[0..32].copy_from_slice(&mint.to_bytes());
        AccountSharedData::from(Account {
            lamports: 1,
            data,
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        })
    }

    #[test]
    fn bpf_resolver_finds_missing_programdata() {
        let program_key = Pubkey::new_unique();
        let programdata_key = Pubkey::new_unique();

        let mut accounts = HashMap::new();
        accounts.insert(program_key, make_bpf_program(&programdata_key));

        let resolver = BpfUpgradeableResolver;
        let missing = resolver.resolve_dependencies(&accounts);
        assert_eq!(missing, vec![programdata_key]);
    }

    #[test]
    fn bpf_resolver_skips_already_loaded() {
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

        let resolver = BpfUpgradeableResolver;
        let missing = resolver.resolve_dependencies(&accounts);
        assert!(missing.is_empty());
    }

    #[test]
    fn token_resolver_finds_missing_mint() {
        let mint_key = Pubkey::new_unique();
        let token_key = Pubkey::new_unique();

        let mut accounts = HashMap::new();
        accounts.insert(token_key, make_token_account(&mint_key));

        let resolver = TokenMintResolver;
        let missing = resolver.resolve_dependencies(&accounts);
        assert_eq!(missing, vec![mint_key]);
    }

    #[test]
    fn token_resolver_skips_loaded_mint() {
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

        let resolver = TokenMintResolver;
        let missing = resolver.resolve_dependencies(&accounts);
        assert!(missing.is_empty());
    }

    #[test]
    fn default_resolvers_returns_both() {
        let resolvers = default_resolvers();
        assert_eq!(resolvers.len(), 2);
    }
}
