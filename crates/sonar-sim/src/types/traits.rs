use std::collections::HashMap;

use anyhow::Result;
use solana_account::Account;
use solana_pubkey::Pubkey;

use super::ResolvedAccounts;

/// Minimal abstraction for on-demand account loading.
///
/// `funding` and other subsystems depend on this trait instead of the
/// concrete `AccountLoader`, allowing test doubles and alternative
/// data sources without an RPC connection.
pub trait AccountAppender {
    fn append_accounts(&self, resolved: &mut ResolvedAccounts, pubkeys: &[Pubkey]) -> Result<()>;
}

/// Hook into the account fetch pipeline.
///
/// Implementors can provide accounts from local sources (e.g. file cache),
/// skip RPC access (offline mode), and report fetch progress.
/// All methods have no-op defaults so callers only override what they need.
pub trait AccountFetchMiddleware: Send + Sync {
    /// Try to resolve accounts from a local source before hitting RPC.
    /// Returns found accounts; keys absent from the result proceed to RPC.
    fn try_resolve_local(&self, pubkeys: &[Pubkey]) -> Result<HashMap<Pubkey, Account>> {
        let _ = pubkeys;
        Ok(HashMap::new())
    }

    /// When true, skip RPC fetching entirely. Missing accounts after local
    /// resolution are treated as non-existent.
    fn is_offline(&self) -> bool {
        false
    }

    /// Called for accounts that could not be resolved in offline mode.
    fn on_offline_missing(&self, pubkeys: &[Pubkey]) {
        let _ = pubkeys;
    }

    /// Called during RPC fetch for progress reporting.
    fn on_fetch_progress(&self, pubkey: &Pubkey, current: usize, total: usize) {
        let _ = (pubkey, current, total);
    }
}
