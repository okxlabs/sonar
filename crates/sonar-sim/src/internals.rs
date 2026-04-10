//! Low-level building blocks for advanced consumers.
//!
//! Items here carry **no semver guarantees** — they may change in any release.
//! Prefer the top-level `Pipeline` API for stable usage.

// ── Transaction parsing ──

pub use crate::transaction::{
    AddressLookupPlan, LookupLocation, MessageAccountPlan, ParsedTransaction,
    RawTransactionEncoding, apply_ix_account_appends, apply_ix_account_patches,
    apply_ix_data_patches, build_lookup_locations, parse_raw_transaction,
};

// ── Account loading ──

pub use crate::account_fetcher::AccountFetcher;
pub use crate::account_loader::AccountLoader;

// ── Account types ──

pub use crate::types::{
    AccountDataPatch, AccountOverride, InstructionAccountAppend, InstructionAccountPatch,
    InstructionDataPatch, ResolvedAccounts, ResolvedLookup,
};

// ── Funding ──

pub use crate::funding::prepare_token_fundings;
pub use crate::types::{PreparedTokenFunding, SolFunding, TokenAmount, TokenFunding};

// ── Simulation types ──

pub use crate::types::{ReturnData, SimulationMetadata};

// ── Execution ──

pub use crate::executor::{
    BundleResult, ExecutionOptions, ExecutionResult, ExecutionStatus, PreparedSimulation,
    SignatureVerification, SimulationOptions, SimulationOptionsBuilder, SimulationRunner,
    StateMutationOptions, apply_account_closures,
};

// ── Balance changes ──

pub use crate::balance_changes::{
    compute_sol_changes, compute_token_changes, extract_mint_decimals_combined,
};

// ── Token decoding ──

pub use crate::token_decode::{
    DecodedTokenAccount, TokenProgramKind, read_mint_decimals, try_decode_token_account,
};

// ── RPC ──

pub use crate::rpc_provider::{FakeAccountProvider, RpcAccountProvider, SolanaRpcProvider};
pub use crate::svm_backend::SvmBackend;

// ── Program identification ──

pub use crate::known_programs::{is_litesvm_builtin_program, is_native_or_sysvar};

// ── Traits and events ──

pub use crate::types::{
    AccountAppender, AccountSource, FetchEvent, FetchObserver, FetchPolicy, RpcDecision,
};

// ── JSON-RPC types ──

pub mod rpc_json {
    pub use crate::rpc_json::*;
}
