//! sonar-sim: Solana transaction simulation engine powered by LiteSVM.
//!
//! # Stable Public API
//!
//! All items below constitute the stable facade of this crate.
//! Internal modules are `pub(crate)` and may change without notice;
//! external consumers should only depend on the re-exports listed here.

pub mod error;

pub(crate) mod account_fetcher;
pub(crate) mod account_loader;
pub(crate) mod balance_changes;
pub(crate) mod executor;
pub(crate) mod funding;
pub mod resolvers;
pub(crate) mod rpc_provider;
pub(crate) mod token_utils;
pub(crate) mod transaction;
pub(crate) mod types;

// ── Error types ──

pub use error::{Result, SonarSimError};

// ── Types ──

pub use types::{
    AccountAppender, AccountDataPatch, AccountFetchMiddleware, Funding, PreparedTokenFunding,
    Replacement, ResolvedAccounts, ResolvedLookup, ReturnData, SimulationMetadata, TokenAmount,
    TokenFunding,
};

// ── Transaction parsing ──

pub use transaction::{
    AddressLookupPlan, LookupLocation, MessageAccountPlan, ParsedTransaction,
    RawTransactionEncoding, build_lookup_locations, parse_raw_transaction,
};

// ── Account loading ──

pub use account_fetcher::AccountFetcher;
pub use account_loader::AccountLoader;

// ── Dependency resolvers ──

pub use resolvers::{
    AccountDependencyResolver, BpfUpgradeableResolver, TokenMintResolver, default_resolvers,
};

// ── RPC provider trait + implementations ──

pub use rpc_provider::{FakeAccountProvider, RpcAccountProvider, SolanaRpcProvider};

// ── Simulation execution ──

pub use executor::{
    ExecutionOptions, ExecutionStatus, SignatureVerification, SimulationOptions,
    SimulationOptionsBuilder, SimulationResult, StateMutationOptions, TransactionExecutor,
    is_native_or_sysvar,
};

// ── Balance change computation ──

pub use balance_changes::{
    SolBalanceChange, TokenBalanceChange, compute_sol_changes, compute_token_changes,
    extract_mint_decimals_combined,
};

// ── Funding ──

pub use funding::prepare_token_fundings;
