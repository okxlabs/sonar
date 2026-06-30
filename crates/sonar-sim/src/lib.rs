//! sonar-sim: Solana transaction simulation engine powered by LiteSVM.
//!
//! # Public API
//!
//! The stable API consists of the items exported at the top level.
//! For advanced/low-level access, use [`internals`].

pub mod error;
pub mod internals;

pub(crate) mod account_dependencies;
pub(crate) mod account_fetcher;
pub(crate) mod account_loader;
pub(crate) mod balance_changes;
pub(crate) mod executor;
pub(crate) mod funding;
pub(crate) mod known_programs;
pub mod mutations;
pub(crate) mod pipeline;
pub(crate) mod result;
pub(crate) mod rpc_json;
pub(crate) mod rpc_provider;
pub(crate) mod rpc_transport;
pub(crate) mod svm_backend;
#[cfg(test)]
pub(crate) mod test_utils;
pub(crate) mod token_decode;
pub(crate) mod transaction;
pub(crate) mod types;

// ── Public API ──
//
// The stable, supported surface. The high-level [`Pipeline`] typestate is the
// primary entry point; the building blocks below let callers that need to
// interleave their own work between stages (see [`pipeline::PreparedPipeline`])
// drive loading, decoding, and rendering directly. For low-level execution
// plumbing without semver guarantees, see [`internals`].

// ── Errors ──
pub use error::{Result, SonarSimError};

// ── Pipeline (typestate stages) ──
pub use pipeline::{
    LoadedBundlePipeline, LoadedPipeline, ParsedBundlePipeline, ParsedPipeline, Pipeline,
    PreparedBundlePipeline, PreparedPipeline,
};

// ── Mutations (declarative simulation changes) ──
pub use mutations::{Mutations, MutationsBuilder, StateMutations, TransactionMutations};
pub use types::{
    AccountDataPatch, AccountOverride, InstructionAccountOp, InstructionDataPatch, InstructionOp,
    PreparedTokenFunding, SolFunding, TokenAmount, TokenFunding,
};

// ── Results & balance changes ──
pub use balance_changes::{
    SolBalanceChange, TokenBalanceChange, compute_sol_changes, compute_token_changes,
    extract_mint_decimals_combined,
};
pub use result::SimulationResult;
pub use types::{ReturnData, SimulationMetadata};

// ── Resolved account model ──
pub use types::{ResolvedAccounts, ResolvedLookup};

// ── Execution result model (also constructed by callers replaying on-chain meta) ──
pub use executor::{BundleResult, ExecutionResult, ExecutionStatus};

// ── Transaction parsing ──
pub use transaction::{
    AddressLookupPlan, LookupLocation, MessageAccountPlan, ParsedTransaction,
    RawTransactionEncoding, build_lookup_locations, parse_raw_transaction,
};

// ── Account loading & fetch seams ──
pub use account_fetcher::DEFAULT_RPC_BATCH_SIZE;
pub use account_loader::AccountLoader;
pub use types::{
    AccountAppender, AccountSource, FetchEvent, FetchObserver, FetchPolicy, RpcDecision,
};

// ── RPC providers ──
pub use rpc_provider::{FakeAccountProvider, RpcAccountProvider, SolanaRpcProvider};

// ── Program identification ──
pub use known_programs::{is_litesvm_builtin_program, is_native_or_sysvar};

// ── Token account decoding ──
pub use token_decode::{
    DecodedTokenAccount, TokenProgramKind, read_mint_decimals, try_decode_token_account,
};
