// crates/sonar-sim/src/pipeline.rs

//! Fluent pipeline API for Solana transaction simulation.

use std::sync::Arc;

use solana_transaction::versioned::VersionedTransaction;

use crate::account_loader::AccountLoader;
use crate::error::{Result, SonarSimError};
use crate::executor::{
    ExecutionOptions, PreparedSimulation, SignatureVerification,
    SimulationOptions, StateMutationOptions,
};
use crate::funding::prepare_token_fundings;
use crate::mutations::Mutations;
use crate::result::SimulationResult;
use crate::rpc_provider::RpcAccountProvider;
use crate::transaction::{
    ParsedTransaction, apply_ix_account_appends, apply_ix_account_patches,
    apply_ix_data_patches, parse_raw_transaction,
};
use crate::types::{AccountSource, FetchObserver, FetchPolicy, ResolvedAccounts, RpcDecision};

// ── Internal offline policy ──

struct OfflinePolicy;

impl FetchPolicy for OfflinePolicy {
    fn decide_rpc(&self, _unresolved: &[solana_pubkey::Pubkey]) -> RpcDecision {
        RpcDecision::Deny
    }
}

// ── Parsed state ──

enum ParsedState {
    Single(ParsedTransaction),
    Bundle(Vec<ParsedTransaction>),
}

// ── Pipeline ──

/// Fluent API for configuring and executing Solana transaction simulations.
///
/// # Usage
///
/// ```ignore
/// let result = Pipeline::new(rpc_url)
///     .parse(raw_tx)?
///     .load_accounts()?
///     .with_mutations(mutations)
///     .execute()?;
/// ```
pub struct Pipeline {
    // RPC config
    rpc_url: Option<String>,
    provider: Option<Arc<dyn RpcAccountProvider>>,
    source: Option<Arc<dyn AccountSource>>,
    observer: Option<Arc<dyn FetchObserver>>,
    offline: bool,

    // Execution config
    verify_signatures: bool,
    slot: Option<u64>,
    timestamp: Option<i64>,

    // Stage state
    parsed: Option<ParsedState>,
    loader: Option<AccountLoader>,
    resolved: Option<ResolvedAccounts>,
    mutations: Option<Mutations>,
}

impl std::fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pipeline")
            .field("rpc_url", &self.rpc_url)
            .field("offline", &self.offline)
            .field("verify_signatures", &self.verify_signatures)
            .field("slot", &self.slot)
            .field("timestamp", &self.timestamp)
            .finish_non_exhaustive()
    }
}

impl Pipeline {
    /// Create a new pipeline with the given RPC URL.
    pub fn new(rpc_url: String) -> Self {
        Self {
            rpc_url: Some(rpc_url),
            provider: None,
            source: None,
            observer: None,
            offline: false,
            verify_signatures: false,
            slot: None,
            timestamp: None,
            parsed: None,
            loader: None,
            resolved: None,
            mutations: None,
        }
    }

    /// Create a pipeline with a custom RPC account provider (useful for testing).
    pub fn with_provider(provider: Arc<dyn RpcAccountProvider>) -> Self {
        Self {
            rpc_url: None,
            provider: Some(provider),
            source: None,
            observer: None,
            offline: false,
            verify_signatures: false,
            slot: None,
            timestamp: None,
            parsed: None,
            loader: None,
            resolved: None,
            mutations: None,
        }
    }

    // ── Config methods ──

    /// Add a local account source (checked before RPC).
    pub fn with_source(mut self, source: Arc<dyn AccountSource>) -> Self {
        self.source = Some(source);
        self
    }

    /// Add a fetch observer for progress reporting.
    pub fn with_observer(mut self, observer: Arc<dyn FetchObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Enable offline mode (blocks all RPC calls).
    pub fn offline(mut self, offline: bool) -> Self {
        self.offline = offline;
        self
    }

    /// Enable/disable signature verification (default: disabled).
    pub fn verify_signatures(mut self, verify: bool) -> Self {
        self.verify_signatures = verify;
        self
    }

    /// Set the SVM slot for simulation.
    pub fn slot(mut self, slot: u64) -> Self {
        self.slot = Some(slot);
        self
    }

    /// Set the SVM clock timestamp for simulation.
    pub fn timestamp(mut self, ts: i64) -> Self {
        self.timestamp = Some(ts);
        self
    }

    // ── Parse stage ──

    /// Parse a single raw transaction (base64 or base58 encoded).
    pub fn parse(mut self, raw_tx: &str) -> Result<Self> {
        let parsed = parse_raw_transaction(raw_tx)?;
        self.parsed = Some(ParsedState::Single(parsed));
        Ok(self)
    }

    /// Parse multiple raw transactions as a bundle.
    pub fn parse_bundle(mut self, raw_txs: &[&str]) -> Result<Self> {
        let mut parsed = Vec::with_capacity(raw_txs.len());
        for raw in raw_txs {
            parsed.push(parse_raw_transaction(raw)?);
        }
        self.parsed = Some(ParsedState::Bundle(parsed));
        Ok(self)
    }

    // ── Accessors ──

    /// Access the parsed transaction (single-tx pipelines only).
    pub fn parsed(&self) -> Option<&ParsedTransaction> {
        match &self.parsed {
            Some(ParsedState::Single(p)) => Some(p),
            _ => None,
        }
    }

    /// Access the resolved accounts (available after `load_accounts()`).
    pub fn resolved(&self) -> Option<&ResolvedAccounts> {
        self.resolved.as_ref()
    }

    // ── Load stage ──

    /// Fetch all accounts referenced by the parsed transaction(s).
    pub fn load_accounts(mut self) -> Result<Self> {
        let parsed = self.parsed.as_ref().ok_or(SonarSimError::Validation {
            reason: "parse() must be called before load_accounts()".into(),
        })?;

        let mut loader = if let Some(provider) = self.provider.clone() {
            AccountLoader::with_provider(provider)
        } else {
            AccountLoader::new(
                self.rpc_url
                    .clone()
                    .ok_or(SonarSimError::Validation {
                        reason: "Pipeline requires either an RPC URL or a custom provider".into(),
                    })?,
            )?
        };

        if let Some(source) = &self.source {
            loader = loader.with_source(source.clone());
        }
        if self.offline {
            loader = loader.with_policy(Arc::new(OfflinePolicy));
        }
        if let Some(observer) = &self.observer {
            loader = loader.with_observer(observer.clone());
        }

        let resolved = match parsed {
            ParsedState::Single(p) => loader.load_for_transaction(&p.transaction)?,
            ParsedState::Bundle(txs) => {
                let refs: Vec<&VersionedTransaction> =
                    txs.iter().map(|t| &t.transaction).collect();
                loader.load_for_transactions(&refs)?
            }
        };

        self.loader = Some(loader);
        self.resolved = Some(resolved);
        Ok(self)
    }

    // ── Mutations ──

    /// Set mutations to apply before execution.
    pub fn with_mutations(mut self, mutations: Mutations) -> Self {
        self.mutations = Some(mutations);
        self
    }

    // ── Execute stage ──

    /// Execute a single transaction simulation.
    pub fn execute(mut self) -> Result<SimulationResult> {
        let parsed_state = self.parsed.take().ok_or(SonarSimError::Validation {
            reason: "parse() must be called before execute()".into(),
        })?;
        let resolved = self.resolved.take().ok_or(SonarSimError::Validation {
            reason: "load_accounts() must be called before execute()".into(),
        })?;
        let mut loader = self.loader.take().ok_or(SonarSimError::Validation {
            reason: "load_accounts() must be called before execute()".into(),
        })?;

        let parsed = match parsed_state {
            ParsedState::Single(p) => p,
            ParsedState::Bundle(_) => {
                return Err(SonarSimError::Validation {
                    reason: "use execute_bundle() for bundle pipelines".into(),
                });
            }
        };

        let mutations = self.mutations.take().unwrap_or_default();
        self.execute_inner(parsed.transaction, mutations, resolved, &mut loader)
    }

    /// Execute a bundle of transactions sequentially.
    pub fn execute_bundle(mut self) -> Result<Vec<SimulationResult>> {
        let parsed_state = self.parsed.take().ok_or(SonarSimError::Validation {
            reason: "parse_bundle() must be called before execute_bundle()".into(),
        })?;
        let resolved = self.resolved.take().ok_or(SonarSimError::Validation {
            reason: "load_accounts() must be called before execute_bundle()".into(),
        })?;
        let mut loader = self.loader.take().ok_or(SonarSimError::Validation {
            reason: "load_accounts() must be called before execute_bundle()".into(),
        })?;

        let parsed_txs = match parsed_state {
            ParsedState::Bundle(txs) => txs,
            ParsedState::Single(_) => {
                return Err(SonarSimError::Validation {
                    reason: "use execute() for single-tx pipelines".into(),
                });
            }
        };

        let mutations = self.mutations.unwrap_or_default();

        // Clone and mutate all transactions
        let mut txs: Vec<VersionedTransaction> = Vec::with_capacity(parsed_txs.len());
        for parsed in &parsed_txs {
            let mut tx = parsed.transaction.clone();
            if !mutations.ix_account_patches.is_empty() {
                apply_ix_account_patches(&mut tx, &mutations.ix_account_patches)?;
            }
            if !mutations.ix_account_appends.is_empty() {
                apply_ix_account_appends(&mut tx, &mutations.ix_account_appends)?;
            }
            if !mutations.ix_data_patches.is_empty() {
                apply_ix_data_patches(&mut tx, &mutations.ix_data_patches)?;
            }
            txs.push(tx);
        }

        // Prepare token fundings
        let prepared_fundings = if !mutations.token_fundings.is_empty() {
            prepare_token_fundings(&mut loader, &resolved, &mutations.token_fundings)?
        } else {
            vec![]
        };

        // Build simulation options
        let sim_opts = SimulationOptions {
            execution: ExecutionOptions {
                signature_verification: SignatureVerification::from(self.verify_signatures),
                slot: self.slot,
                timestamp: self.timestamp,
            },
            mutations: StateMutationOptions {
                account_closures: mutations.account_closures,
                overrides: mutations.account_overrides,
                sol_fundings: mutations.sol_fundings,
                token_fundings: prepared_fundings,
                data_patches: mutations.account_data_patches,
            },
        };

        let prepared = PreparedSimulation::prepare(resolved, sim_opts)?;
        let mut runner = prepared.into_runner();

        let tx_refs: Vec<&VersionedTransaction> = txs.iter().collect();
        let exec_results = runner.execute_bundle(&tx_refs);

        exec_results
            .into_iter()
            .map(|r| r.map(SimulationResult::from_execution))
            .collect()
    }

    // ── Private helpers ──

    fn execute_inner(
        &self,
        transaction: VersionedTransaction,
        mutations: Mutations,
        resolved: ResolvedAccounts,
        loader: &mut AccountLoader,
    ) -> Result<SimulationResult> {
        let mut tx = transaction;

        // Apply transaction-level mutations
        if !mutations.ix_account_patches.is_empty() {
            apply_ix_account_patches(&mut tx, &mutations.ix_account_patches)?;
        }
        if !mutations.ix_account_appends.is_empty() {
            apply_ix_account_appends(&mut tx, &mutations.ix_account_appends)?;
        }
        if !mutations.ix_data_patches.is_empty() {
            apply_ix_data_patches(&mut tx, &mutations.ix_data_patches)?;
        }

        // Prepare token fundings
        let prepared_fundings = if !mutations.token_fundings.is_empty() {
            prepare_token_fundings(loader, &resolved, &mutations.token_fundings)?
        } else {
            vec![]
        };

        // Build simulation options
        let sim_opts = SimulationOptions {
            execution: ExecutionOptions {
                signature_verification: SignatureVerification::from(self.verify_signatures),
                slot: self.slot,
                timestamp: self.timestamp,
            },
            mutations: StateMutationOptions {
                account_closures: mutations.account_closures,
                overrides: mutations.account_overrides,
                sol_fundings: mutations.sol_fundings,
                token_fundings: prepared_fundings,
                data_patches: mutations.account_data_patches,
            },
        };

        let prepared = PreparedSimulation::prepare(resolved, sim_opts)?;
        let mut runner = prepared.into_runner();
        let exec_result = runner.execute(&tx)?;

        Ok(SimulationResult::from_execution(exec_result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Base64-encoded SOL transfer transaction (deterministic: payer=[1;32], recipient=[2;32])
    const TEST_TX_BASE64: &str = "AYXl4tu2q/qsjwA+woUaYKC+uPuAozXJHsgxsZLux/8uXuN2z8P1tLt0wHkQImIfxXBjg3dT8ryk8D5BA6g+/QABAAEDiojj3XQJ8ZX9UtstPLpdcspnCb8dlBIb83SIAbQPb1wCAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAgIAAQwCAAAAgJaYAAAAAAA=";

    #[test]
    fn execute_before_parse_returns_validation_error() {
        let pipeline = Pipeline::new("http://localhost:8899".into());
        let result = pipeline.execute();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SonarSimError::Validation { .. }));
    }

    #[test]
    fn execute_before_load_returns_validation_error() {
        let pipeline = Pipeline::new("http://localhost:8899".into())
            .parse(TEST_TX_BASE64).unwrap();
        let result = pipeline.execute();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SonarSimError::Validation { .. }));
    }

    #[test]
    fn load_before_parse_returns_validation_error() {
        let pipeline = Pipeline::new("http://localhost:8899".into());
        let result = pipeline.load_accounts();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SonarSimError::Validation { .. }));
    }

    #[test]
    fn parse_stores_transaction() {
        let pipeline = Pipeline::new("http://localhost:8899".into())
            .parse(TEST_TX_BASE64).unwrap();
        assert!(pipeline.parsed().is_some());
    }

    #[test]
    fn execute_bundle_on_single_returns_error() {
        let pipeline = Pipeline::new("http://localhost:8899".into())
            .parse(TEST_TX_BASE64).unwrap();
        let result = pipeline.execute_bundle();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SonarSimError::Validation { .. }));
    }

    #[test]
    fn execute_on_bundle_returns_error() {
        let pipeline = Pipeline::new("http://localhost:8899".into())
            .parse_bundle(&[TEST_TX_BASE64, TEST_TX_BASE64]).unwrap();
        let result = pipeline.execute();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SonarSimError::Validation { .. }));
    }

    #[test]
    fn parsed_returns_none_for_bundle() {
        let pipeline = Pipeline::new("http://localhost:8899".into())
            .parse_bundle(&[TEST_TX_BASE64]).unwrap();
        assert!(pipeline.parsed().is_none());
    }

    #[test]
    fn resolved_returns_none_before_load() {
        let pipeline = Pipeline::new("http://localhost:8899".into())
            .parse(TEST_TX_BASE64).unwrap();
        assert!(pipeline.resolved().is_none());
    }

}
