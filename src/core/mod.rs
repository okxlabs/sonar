// Bridge modules: re-export stable items from sonar-sim's public facade
// so that CLI code can continue to use `crate::core::{types, balance_changes, funding}::...`.
pub mod balance_changes {
    pub use sonar_sim::{
        compute_sol_changes, compute_token_changes, extract_mint_decimals_combined,
    };
}

pub mod types {
    pub use sonar_sim::{AccountDataPatch, Funding, Replacement, TokenAmount, TokenFunding};
}

pub mod funding {
    pub use sonar_sim::{PreparedTokenFunding, prepare_token_fundings};
}

// Kept in CLI (full-featured versions with progress/idl/cache/offline)
pub mod account_file;
pub mod account_loader;
pub mod cache;
pub mod executor;
pub mod idl_fetcher;
pub mod rpc_provider;
pub mod transaction;
