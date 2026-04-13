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

pub use balance_changes::{SolBalanceChange, TokenBalanceChange};
pub use error::{Result, SonarSimError};
pub use mutations::{Mutations, MutationsBuilder};
pub use pipeline::Pipeline;
pub use result::SimulationResult;
pub use types::{AccountSource, FetchEvent, FetchObserver};
