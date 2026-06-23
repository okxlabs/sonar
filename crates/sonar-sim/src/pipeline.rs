// crates/sonar-sim/src/pipeline.rs

//! Fluent pipeline API for Solana transaction simulation.
//!
//! The pipeline is a typestate: each stage returns a distinct type that only
//! exposes the methods legal at that point. `parse` → `load_accounts` →
//! `execute` ordering is enforced by the compiler, so the illegal sequences
//! (executing before parsing, loading before parsing) are unrepresentable
//! rather than runtime errors. Single-transaction and bundle pipelines are
//! likewise separate types, so `execute`/`execute_bundle` can't be mismatched.

use std::sync::Arc;

use solana_transaction::versioned::VersionedTransaction;

use crate::account_loader::AccountLoader;
use crate::error::{Result, SonarSimError};
use crate::executor::BundleResult;
use crate::executor::{
    ExecutionOptions, PreparedSimulation, SignatureVerification, SimulationOptions,
    SimulationRunner, StateMutationOptions,
};
use crate::funding::prepare_token_fundings;
use crate::mutations::Mutations;
use crate::result::SimulationResult;
use crate::rpc_provider::RpcAccountProvider;
use crate::transaction::{
    ParsedTransaction, apply_ix_account_ops, apply_ix_data_patches, parse_raw_transaction,
};
use crate::types::{AccountSource, FetchObserver, FetchPolicy, ResolvedAccounts, RpcDecision};

// ── Internal offline policy ──

struct OfflinePolicy;

impl FetchPolicy for OfflinePolicy {
    fn decide_rpc(&self, _unresolved: &[solana_pubkey::Pubkey]) -> RpcDecision {
        RpcDecision::Deny
    }
}

// ── Shared configuration ──

/// RPC and execution configuration, captured before parsing and threaded
/// through every stage. The setters live on [`Pipeline`]; later stages only
/// read it.
#[derive(Default)]
struct PipelineConfig {
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
}

impl PipelineConfig {
    /// Build a configured [`AccountLoader`] from the provider/RPC settings.
    fn build_loader(&self) -> Result<AccountLoader> {
        let mut loader = if let Some(provider) = self.provider.clone() {
            AccountLoader::with_provider(provider)
        } else {
            AccountLoader::new(self.rpc_url.clone().ok_or(SonarSimError::Validation {
                reason: "Pipeline requires either an RPC URL or a custom provider".into(),
            })?)?
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
        Ok(loader)
    }

    /// Build [`SimulationOptions`] from config and user-facing [`Mutations`].
    ///
    /// Handles token funding preparation and maps `Mutations` fields into the
    /// executor's [`StateMutationOptions`].
    fn build_sim_options(
        &self,
        mutations: Mutations,
        loader: &mut AccountLoader,
        resolved: &ResolvedAccounts,
    ) -> Result<SimulationOptions> {
        let state = mutations.state;
        let prepared_fundings = if !state.token_fundings.is_empty() {
            prepare_token_fundings(loader, resolved, &state.token_fundings)?
        } else {
            vec![]
        };

        Ok(SimulationOptions {
            execution: ExecutionOptions {
                signature_verification: SignatureVerification::from(self.verify_signatures),
                slot: self.slot,
                timestamp: self.timestamp,
            },
            mutations: StateMutationOptions {
                account_closures: state.account_closures,
                account_overrides: state.account_overrides,
                sol_fundings: state.sol_fundings,
                token_fundings: prepared_fundings,
                account_data_patches: state.account_data_patches,
            },
        })
    }
}

/// Apply transaction-level mutations (instruction patches) to a transaction.
fn apply_tx_mutations(tx: &mut VersionedTransaction, mutations: &Mutations) -> Result<()> {
    let tx_m = &mutations.transaction;
    if !tx_m.ix_account_ops.is_empty() {
        apply_ix_account_ops(tx, &tx_m.ix_account_ops)?;
    }
    if !tx_m.ix_data_patches.is_empty() {
        apply_ix_data_patches(tx, &tx_m.ix_data_patches)?;
    }
    Ok(())
}

/// Prepare a ready-to-run [`SimulationRunner`] from resolved accounts and
/// mutations. Shared by single and bundle execution.
fn build_runner(
    config: &PipelineConfig,
    mut loader: AccountLoader,
    resolved: ResolvedAccounts,
    mutations: Mutations,
) -> Result<SimulationRunner> {
    let sim_opts = config.build_sim_options(mutations, &mut loader, &resolved)?;
    let prepared = PreparedSimulation::prepare(resolved, sim_opts)?;
    Ok(prepared.into_runner())
}

// ── Stage 0: Pipeline (config + parse) ──

/// Entry point: configure RPC/execution settings, then `parse` (single) or
/// `parse_bundle` to advance to the next stage.
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
#[derive(Default)]
pub struct Pipeline {
    config: PipelineConfig,
}

impl std::fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pipeline")
            .field("rpc_url", &self.config.rpc_url)
            .field("offline", &self.config.offline)
            .field("verify_signatures", &self.config.verify_signatures)
            .field("slot", &self.config.slot)
            .field("timestamp", &self.config.timestamp)
            .finish_non_exhaustive()
    }
}

impl Pipeline {
    /// Create a new pipeline with the given RPC URL.
    pub fn new(rpc_url: String) -> Self {
        Self { config: PipelineConfig { rpc_url: Some(rpc_url), ..Default::default() } }
    }

    /// Create a pipeline with a custom RPC account provider (useful for testing).
    pub fn with_provider(provider: Arc<dyn RpcAccountProvider>) -> Self {
        Self { config: PipelineConfig { provider: Some(provider), ..Default::default() } }
    }

    // ── Config methods ──

    /// Add a local account source (checked before RPC).
    pub fn with_source(mut self, source: Arc<dyn AccountSource>) -> Self {
        self.config.source = Some(source);
        self
    }

    /// Add a fetch observer for progress reporting.
    pub fn with_observer(mut self, observer: Arc<dyn FetchObserver>) -> Self {
        self.config.observer = Some(observer);
        self
    }

    /// Enable offline mode (blocks all RPC calls).
    pub fn offline(mut self, offline: bool) -> Self {
        self.config.offline = offline;
        self
    }

    /// Enable/disable signature verification (default: disabled).
    pub fn verify_signatures(mut self, verify: bool) -> Self {
        self.config.verify_signatures = verify;
        self
    }

    /// Set the SVM slot for simulation.
    pub fn slot(mut self, slot: u64) -> Self {
        self.config.slot = Some(slot);
        self
    }

    /// Set the SVM clock timestamp for simulation.
    pub fn timestamp(mut self, ts: i64) -> Self {
        self.config.timestamp = Some(ts);
        self
    }

    // ── Parse stage ──

    /// Parse a single raw transaction (base64 or base58 encoded).
    pub fn parse(self, raw_tx: &str) -> Result<ParsedPipeline> {
        let parsed = parse_raw_transaction(raw_tx)?;
        Ok(ParsedPipeline { config: self.config, parsed })
    }

    /// Parse multiple raw transactions as a bundle.
    pub fn parse_bundle(self, raw_txs: &[&str]) -> Result<ParsedBundlePipeline> {
        let mut parsed = Vec::with_capacity(raw_txs.len());
        for raw in raw_txs {
            parsed.push(parse_raw_transaction(raw)?);
        }
        Ok(ParsedBundlePipeline { config: self.config, parsed })
    }
}

// ── Stage 1: parsed (single) ──

/// A parsed single-transaction pipeline. Call `load_accounts` to advance.
pub struct ParsedPipeline {
    config: PipelineConfig,
    parsed: ParsedTransaction,
}

impl ParsedPipeline {
    /// Access the parsed transaction.
    pub fn parsed(&self) -> &ParsedTransaction {
        &self.parsed
    }

    /// Fetch all accounts referenced by the parsed transaction.
    pub fn load_accounts(self) -> Result<LoadedPipeline> {
        let mut loader = self.config.build_loader()?;
        let resolved = loader.load_for_transaction(&self.parsed.transaction)?;
        Ok(LoadedPipeline {
            config: self.config,
            parsed: self.parsed,
            loader,
            resolved,
            mutations: None,
        })
    }
}

// ── Stage 1: parsed (bundle) ──

/// A parsed bundle pipeline. Call `load_accounts` to advance.
pub struct ParsedBundlePipeline {
    config: PipelineConfig,
    parsed: Vec<ParsedTransaction>,
}

impl ParsedBundlePipeline {
    /// Fetch all accounts referenced by every transaction in the bundle.
    pub fn load_accounts(self) -> Result<LoadedBundlePipeline> {
        let mut loader = self.config.build_loader()?;
        let refs: Vec<&VersionedTransaction> = self.parsed.iter().map(|t| &t.transaction).collect();
        let resolved = loader.load_for_transactions(&refs)?;
        Ok(LoadedBundlePipeline {
            config: self.config,
            parsed: self.parsed,
            loader,
            resolved,
            mutations: None,
        })
    }
}

// ── Stage 2: loaded (single) ──

/// A single-transaction pipeline with accounts loaded; ready to `execute`.
pub struct LoadedPipeline {
    config: PipelineConfig,
    parsed: ParsedTransaction,
    loader: AccountLoader,
    resolved: ResolvedAccounts,
    mutations: Option<Mutations>,
}

impl LoadedPipeline {
    /// Access the resolved accounts.
    pub fn resolved(&self) -> &ResolvedAccounts {
        &self.resolved
    }

    /// Set mutations to apply before execution.
    pub fn with_mutations(mut self, mutations: Mutations) -> Self {
        self.mutations = Some(mutations);
        self
    }

    /// Execute the transaction simulation.
    pub fn execute(self) -> Result<SimulationResult> {
        let mutations = self.mutations.unwrap_or_default();
        let mut tx = self.parsed.transaction;
        apply_tx_mutations(&mut tx, &mutations)?;

        let mut runner = build_runner(&self.config, self.loader, self.resolved, mutations)?;
        let exec_result = runner.execute(&tx)?;
        Ok(SimulationResult::from_execution(exec_result))
    }
}

// ── Stage 2: loaded (bundle) ──

/// A bundle pipeline with accounts loaded; ready to `execute_bundle`.
pub struct LoadedBundlePipeline {
    config: PipelineConfig,
    parsed: Vec<ParsedTransaction>,
    loader: AccountLoader,
    resolved: ResolvedAccounts,
    mutations: Option<Mutations>,
}

impl LoadedBundlePipeline {
    /// Access the resolved accounts.
    pub fn resolved(&self) -> &ResolvedAccounts {
        &self.resolved
    }

    /// Set mutations to apply before execution.
    pub fn with_mutations(mut self, mutations: Mutations) -> Self {
        self.mutations = Some(mutations);
        self
    }

    /// Execute the bundle of transactions sequentially.
    ///
    /// Returns a [`BundleResult`] containing results for all executed
    /// transactions and the total count. Check [`BundleResult::skipped_count`]
    /// to detect transactions that were never attempted due to a prior failure.
    pub fn execute_bundle(self) -> Result<BundleResult<Result<SimulationResult>>> {
        let mutations = self.mutations.unwrap_or_default();

        let mut txs: Vec<VersionedTransaction> = Vec::with_capacity(self.parsed.len());
        for parsed in &self.parsed {
            let mut tx = parsed.transaction.clone();
            apply_tx_mutations(&mut tx, &mutations)?;
            txs.push(tx);
        }

        let mut runner = build_runner(&self.config, self.loader, self.resolved, mutations)?;

        let tx_refs: Vec<&VersionedTransaction> = txs.iter().collect();
        let bundle = runner.execute_bundle(&tx_refs);

        let total = bundle.total();
        let mapped = bundle
            .into_executed()
            .into_iter()
            .map(|r| r.map(SimulationResult::from_execution))
            .collect();

        Ok(BundleResult::new(mapped, total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Base64-encoded SOL transfer transaction (deterministic: payer=[1;32], recipient=[2;32])
    const TEST_TX_BASE64: &str = "AYXl4tu2q/qsjwA+woUaYKC+uPuAozXJHsgxsZLux/8uXuN2z8P1tLt0wHkQImIfxXBjg3dT8ryk8D5BA6g+/QABAAEDiojj3XQJ8ZX9UtstPLpdcspnCb8dlBIb83SIAbQPb1wCAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAgIAAQwCAAAAgJaYAAAAAAA=";

    // Note: the staged typestate makes "execute before parse", "execute before
    // load", "load before parse", and single/bundle execute mismatches into
    // compile errors rather than runtime `Validation` errors — so those former
    // negative tests no longer exist (the illegal states can't be constructed).

    #[test]
    fn parse_exposes_transaction() {
        let parsed = Pipeline::new("http://localhost:8899".into()).parse(TEST_TX_BASE64).unwrap();
        // `parsed()` is available on the parsed stage and returns the tx.
        let _ = parsed.parsed();
    }

    #[test]
    fn parse_bundle_succeeds() {
        let bundle = Pipeline::new("http://localhost:8899".into())
            .parse_bundle(&[TEST_TX_BASE64, TEST_TX_BASE64]);
        assert!(bundle.is_ok());
    }

    #[test]
    fn bundle_result_skipped_and_complete() {
        let br: crate::executor::BundleResult<std::result::Result<(), String>> =
            crate::executor::BundleResult::new(vec![], 5);
        assert_eq!(br.skipped_count(), 5);
        assert!(!br.is_complete());
        assert_eq!(br.total(), 5);
        assert!(br.executed().is_empty());

        let br_full = crate::executor::BundleResult::new(vec![Ok::<_, String>(())], 1);
        assert_eq!(br_full.skipped_count(), 0);
        assert!(br_full.is_complete());
    }
}
