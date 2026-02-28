//! sonar-sim: Solana transaction simulation engine powered by LiteSVM.
//!
//! # Stable Public API
//!
//! All items below constitute the stable facade of this crate.
//! Internal modules are `pub(crate)` and may change without notice;
//! external consumers should only depend on the re-exports listed here.

pub mod error;

pub(crate) mod account_dependencies;
pub(crate) mod account_fetcher;
pub(crate) mod account_loader;
pub(crate) mod balance_changes;
pub(crate) mod executor;
pub(crate) mod funding;
pub(crate) mod known_programs;
pub(crate) mod rpc_provider;
#[cfg(test)]
pub(crate) mod test_utils;
pub(crate) mod token_decode;
pub(crate) mod transaction;
pub(crate) mod types;

// ── Error types ──

pub use error::{Result, SonarSimError};

// ── Types ──

pub use token_decode::TokenProgramKind;
pub use types::{
    AccountAppender, AccountDataPatch, AccountReplacement, AccountSource, FetchEvent,
    FetchObserver, FetchPolicy, PreparedTokenFunding, ResolvedAccounts, ResolvedLookup, ReturnData,
    RpcDecision, SimulationMetadata, SolFunding, TokenAmount, TokenFunding,
};

// ── Transaction parsing ──

pub use transaction::{
    AddressLookupPlan, LookupLocation, MessageAccountPlan, ParsedTransaction,
    RawTransactionEncoding, build_lookup_locations, parse_raw_transaction,
};

// ── Account loading ──

pub use account_fetcher::AccountFetcher;
pub use account_loader::AccountLoader;

// ── RPC provider trait + implementations ──

pub use rpc_provider::{FakeAccountProvider, RpcAccountProvider, SolanaRpcProvider};

// ── Simulation execution ──

pub use executor::{
    ExecutionOptions, ExecutionStatus, SignatureVerification, SimulationOptions,
    SimulationOptionsBuilder, SimulationResult, StateMutationOptions, TransactionExecutor,
};
pub use known_programs::{is_litesvm_builtin_program, is_native_or_sysvar};

// ── Balance change computation ──

pub use balance_changes::{
    SolBalanceChange, TokenBalanceChange, compute_sol_changes, compute_token_changes,
    extract_mint_decimals_combined,
};

// ── Funding ──

pub use funding::prepare_token_fundings;
