//! Low-level building blocks for callers that drive the simulation stages
//! themselves instead of using the high-level [`Pipeline`](crate::Pipeline).
//!
//! This is the primary API the `sonar-cli` binary is built on: it composes the
//! parse → load → prepare → run stages directly so it can interleave its own
//! caching, IDL fetching, and progress reporting between them. `Pipeline` is the
//! convenience facade over the same pieces for callers that don't need that.
//!
//! Items here carry **no semver guarantees** — they may change in any release.

// ── Transaction parsing ──

pub use crate::transaction::{
    AddressLookupPlan, LookupLocation, MessageAccountPlan, ParsedTransaction,
    RawTransactionEncoding, apply_ix_account_ops, apply_ix_data_patches, build_lookup_locations,
    parse_raw_transaction,
};

// ── Account loading ──

pub use crate::account_fetcher::{AccountFetcher, DEFAULT_RPC_BATCH_SIZE};
pub use crate::account_loader::AccountLoader;

// ── Account types ──

pub use crate::types::{
    AccountDataPatch, AccountOverride, InstructionAccountOp, InstructionDataPatch,
    ResolvedAccounts, ResolvedLookup,
};

// ── Funding ──

pub use crate::funding::prepare_token_fundings;
pub use crate::types::{PreparedTokenFunding, SolFunding, TokenAmount, TokenFunding};

// ── Simulation types ──

pub use crate::types::{ReturnData, SimulationMetadata};

// ── Execution ──

pub use crate::executor::{
    BundleResult, ExecutionOptions, ExecutionResult, ExecutionStatus, PreparedSimulation,
    SignatureVerification, SimulationOptions, SimulationRunner, StateMutationOptions,
    apply_account_closures,
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
pub use crate::rpc_transport::RpcTransport;
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
