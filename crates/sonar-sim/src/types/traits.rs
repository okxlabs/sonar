use std::collections::HashMap;

use solana_account::AccountSharedData;
use solana_pubkey::Pubkey;

use super::accounts::ResolvedAccounts;
use crate::error::Result;

/// Minimal abstraction for on-demand account loading.
///
/// `funding` and other subsystems depend on this trait instead of the
/// concrete `AccountLoader`, allowing test doubles and alternative
/// data sources without an RPC connection.
pub trait AccountAppender {
    fn append_accounts(
        &mut self,
        resolved: &mut ResolvedAccounts,
        pubkeys: &[Pubkey],
    ) -> Result<()>;
}

/// Local account source used before RPC.
///
/// Sources are chained in order. Each source receives keys that are still
/// unresolved and may return a subset of accounts.
pub trait AccountSource: Send + Sync {
    fn resolve(&self, pubkeys: &[Pubkey]) -> Result<HashMap<Pubkey, AccountSharedData>>;
}

/// Policy gate that decides whether unresolved accounts may use RPC.
pub trait FetchPolicy: Send + Sync {
    fn decide_rpc(&self, unresolved: &[Pubkey]) -> RpcDecision;
}

/// Observer for account fetch pipeline events.
pub trait FetchObserver: Send + Sync {
    fn on_event(&self, event: &FetchEvent);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchEvent {
    LocalResolved { pubkey: Pubkey },
    RpcSkippedByPolicy { missing: Vec<Pubkey> },
    RpcBatchStarted { batch_index: usize, batch_size: usize, total_requested: usize },
    RpcProgress { pubkey: Pubkey, current: usize, total: usize },
    RpcFinished { requested: usize, fetched: usize, missing: usize },
}
