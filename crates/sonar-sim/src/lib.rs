//! sonar-sim: Solana transaction simulation engine powered by LiteSVM.
//!
//! # Stable Public API
//!
//! All items below constitute the stable facade of this crate.
//! Internal modules are `pub(crate)` and may change without notice;
//! external consumers should only depend on the re-exports listed here.

pub(crate) mod account_loader;
pub(crate) mod balance_changes;
pub(crate) mod executor;
pub(crate) mod funding;
pub(crate) mod rpc_provider;
pub(crate) mod transaction;
pub(crate) mod types;

// ── Types ──

pub use types::{
    AccountDataPatch, Funding, PreparedTokenFunding, Replacement, ResolvedAccounts, ResolvedLookup,
    TokenAmount, TokenFunding,
};

// ── Transaction parsing ──

pub use transaction::{
    AccountReference, AccountSource, AddressLookupPlan, LookupLocation, MessageAccountPlan,
    ParsedTransaction, RawTransactionEncoding, build_lookup_locations, classify_account_reference,
    collect_account_plan, parse_raw_transaction,
};

// ── Account loading ──

pub use account_loader::AccountLoader;

// ── RPC provider trait + implementations ──

pub use rpc_provider::{FakeAccountProvider, RpcAccountProvider, SolanaRpcProvider};

// ── Simulation execution ──

pub use executor::{
    ExecutionStatus, SimulationOptions, SimulationResult, TransactionExecutor, is_native_or_sysvar,
};

// ── Balance change computation ──

pub use balance_changes::{
    SolBalanceChange, TokenBalanceChange, compute_sol_changes, compute_token_changes,
    extract_mint_decimals_combined,
};

// ── Funding ──

pub use funding::{apply_sol_fundings, prepare_token_fundings};
