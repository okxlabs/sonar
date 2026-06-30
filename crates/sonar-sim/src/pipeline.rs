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

use solana_pubkey::Pubkey;
use solana_transaction::versioned::VersionedTransaction;

use crate::account_loader::AccountLoader;
use crate::error::{Result, SonarSimError};
use crate::executor::BundleResult;
use crate::executor::{
    ExecutionOptions, ExecutionResult, PreparedSimulation, SignatureVerification,
    SimulationOptions, SimulationRunner, StateMutationOptions,
};
use crate::funding::prepare_token_fundings;
use crate::mutations::{Mutations, StateMutations};
use crate::result::SimulationResult;
use crate::rpc_provider::RpcAccountProvider;
use crate::transaction::{
    MessageAccountPlan, ParsedTransaction, apply_instruction_ops, apply_ix_account_ops,
    apply_ix_data_patches, parse_raw_transaction,
};
use crate::types::{
    AccountSource, FetchObserver, FetchPolicy, PreparedTokenFunding, ResolvedAccounts, RpcDecision,
};

// ── Internal offline policy ──

struct OfflinePolicy;

impl FetchPolicy for OfflinePolicy {
    fn decide_rpc(&self, _unresolved: &[Pubkey]) -> RpcDecision {
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
    /// Caller-supplied, fully-configured loader. Takes precedence over the
    /// `rpc_url`/`provider`/`source`/`observer`/`offline` settings, which exist
    /// for callers that want the pipeline to assemble the loader itself.
    loader: Option<AccountLoader>,
    source: Option<Arc<dyn AccountSource>>,
    observer: Option<Arc<dyn FetchObserver>>,
    offline: bool,

    // Execution config
    verify_signatures: bool,
    slot: Option<u64>,
    timestamp: Option<i64>,
}

impl PipelineConfig {
    /// Build (or take) a configured [`AccountLoader`]. A loader supplied via
    /// [`Pipeline::with_loader`] is used as-is; otherwise one is assembled from
    /// the provider/RPC/source/observer/offline settings.
    fn build_loader(&mut self) -> Result<AccountLoader> {
        if let Some(loader) = self.loader.take() {
            return Ok(loader);
        }

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

    /// Map config + user-facing [`Mutations`] and the already-prepared token
    /// fundings into the executor's [`SimulationOptions`]. Token fundings are
    /// prepared earlier (in [`LoadedPipeline::prepare`]) so the prepared stage
    /// can expose them to callers before execution.
    fn build_sim_options(
        &self,
        state: StateMutations,
        prepared_fundings: Vec<PreparedTokenFunding>,
    ) -> SimulationOptions {
        SimulationOptions {
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
        }
    }
}

/// Prepare token fundings (resolving mints/decimals via `loader`) for the
/// requested state mutations. Returns the empty vec when none were requested.
fn prepare_fundings(
    loader: &mut AccountLoader,
    resolved: &ResolvedAccounts,
    state: &StateMutations,
) -> Result<Vec<PreparedTokenFunding>> {
    if state.token_fundings.is_empty() {
        return Ok(vec![]);
    }
    prepare_token_fundings(loader, resolved, &state.token_fundings)
}

/// Apply transaction-level mutations (instruction patches) to a transaction.
///
/// Whole-instruction ops (insert / remove) run first so account-level and
/// data mutations target the post-restructure instruction list.
fn apply_tx_mutations(tx: &mut VersionedTransaction, mutations: &Mutations) -> Result<()> {
    let tx_m = &mutations.transaction;
    if !tx_m.instruction_ops.is_empty() {
        apply_instruction_ops(tx, &tx_m.instruction_ops)?;
    }
    if !tx_m.ix_account_ops.is_empty() {
        apply_ix_account_ops(tx, &tx_m.ix_account_ops)?;
    }
    if !tx_m.ix_data_patches.is_empty() {
        apply_ix_data_patches(tx, &tx_m.ix_data_patches)?;
    }
    Ok(())
}

/// Prepare a ready-to-run [`SimulationRunner`] from resolved accounts, the state
/// mutations, and the already-prepared token fundings. Shared by single and
/// bundle execution.
fn build_runner(
    config: &PipelineConfig,
    resolved: ResolvedAccounts,
    state: StateMutations,
    prepared_fundings: Vec<PreparedTokenFunding>,
) -> Result<SimulationRunner> {
    let sim_opts = config.build_sim_options(state, prepared_fundings);
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
///     .with_mutations(mutations)? // optional
///     .load_accounts()?
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

    /// Create a pipeline that loads accounts through a caller-supplied
    /// [`AccountLoader`]. Use this when the loader needs configuration the
    /// pipeline's own setters don't cover — multiple fetch observers, a custom
    /// RPC batch size, or a bespoke fetch policy. The loader's provider is later
    /// reachable via [`PreparedPipeline::provider`] for follow-on fetches.
    pub fn with_loader(loader: AccountLoader) -> Self {
        Self { config: PipelineConfig { loader: Some(loader), ..Default::default() } }
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
        Ok(ParsedPipeline { config: self.config, parsed, state: StateMutations::default() })
    }

    /// Parse multiple raw transactions as a bundle.
    pub fn parse_bundle(self, raw_txs: &[&str]) -> Result<ParsedBundlePipeline> {
        let mut parsed = Vec::with_capacity(raw_txs.len());
        for raw in raw_txs {
            parsed.push(parse_raw_transaction(raw)?);
        }
        Ok(ParsedBundlePipeline { config: self.config, parsed, state: StateMutations::default() })
    }
}

// ── Stage 1: parsed (single) ──

/// A parsed single-transaction pipeline. Optionally apply [`with_mutations`],
/// then call `load_accounts` to advance.
///
/// [`with_mutations`]: ParsedPipeline::with_mutations
pub struct ParsedPipeline {
    config: PipelineConfig,
    parsed: ParsedTransaction,
    state: StateMutations,
}

impl ParsedPipeline {
    /// Access the parsed transaction.
    pub fn parsed(&self) -> &ParsedTransaction {
        &self.parsed
    }

    /// Apply mutations. Transaction-level instruction ops are applied to the
    /// parsed transaction *now* — failing fast on invalid ops, before any
    /// account loading — and the account plan is recomputed so `load_accounts`
    /// fetches the post-mutation account set (no separate re-fetch needed).
    /// State-level mutations (funding, overrides, closures, data patches) are
    /// retained and applied to the SVM at execution.
    pub fn with_mutations(mut self, mutations: Mutations) -> Result<Self> {
        if !mutations.transaction.is_empty() {
            apply_tx_mutations(&mut self.parsed.transaction, &mutations)?;
            // The instruction list changed; recompute the now-stale account plan.
            self.parsed.account_plan =
                MessageAccountPlan::from_transaction(&self.parsed.transaction);
        }
        self.state = mutations.state;
        Ok(self)
    }

    /// Fetch all accounts referenced by the (post-mutation) parsed transaction.
    pub fn load_accounts(mut self) -> Result<LoadedPipeline> {
        let mut loader = self.config.build_loader()?;
        let resolved = loader.load_for_transaction(&self.parsed.transaction)?;
        Ok(LoadedPipeline {
            config: self.config,
            parsed: self.parsed,
            loader,
            resolved,
            state: self.state,
        })
    }
}

// ── Stage 1: parsed (bundle) ──

/// A parsed bundle pipeline. Optionally apply [`with_mutations`], then call
/// `load_accounts` to advance.
///
/// [`with_mutations`]: ParsedBundlePipeline::with_mutations
pub struct ParsedBundlePipeline {
    config: PipelineConfig,
    parsed: Vec<ParsedTransaction>,
    state: StateMutations,
}

impl ParsedBundlePipeline {
    /// Apply mutations to every transaction in the bundle. See
    /// [`ParsedPipeline::with_mutations`]; transaction-level ops apply (fail-fast)
    /// to each transaction before loading, state-level mutations are retained.
    pub fn with_mutations(mut self, mutations: Mutations) -> Result<Self> {
        if !mutations.transaction.is_empty() {
            for tx in &mut self.parsed {
                apply_tx_mutations(&mut tx.transaction, &mutations)?;
                tx.account_plan = MessageAccountPlan::from_transaction(&tx.transaction);
            }
        }
        self.state = mutations.state;
        Ok(self)
    }

    /// Fetch all accounts referenced by every (post-mutation) transaction.
    pub fn load_accounts(mut self) -> Result<LoadedBundlePipeline> {
        let mut loader = self.config.build_loader()?;
        let refs: Vec<&VersionedTransaction> = self.parsed.iter().map(|t| &t.transaction).collect();
        let resolved = loader.load_for_transactions(&refs)?;
        Ok(LoadedBundlePipeline {
            config: self.config,
            parsed: self.parsed,
            loader,
            resolved,
            state: self.state,
        })
    }
}

// ── Stage 2: loaded (single) ──

/// A single-transaction pipeline with accounts loaded; call `prepare` (or
/// `execute`) to advance. Apply mutations earlier, at [`ParsedPipeline`].
pub struct LoadedPipeline {
    config: PipelineConfig,
    parsed: ParsedTransaction,
    loader: AccountLoader,
    resolved: ResolvedAccounts,
    state: StateMutations,
}

impl LoadedPipeline {
    /// Access the resolved accounts.
    pub fn resolved(&self) -> &ResolvedAccounts {
        &self.resolved
    }

    /// Resolve state mutations that need loaded context (token funding mints),
    /// advancing to a [`PreparedPipeline`]. Its [`resolved`](PreparedPipeline::resolved)
    /// set and [`parsed`](PreparedPipeline::parsed) transaction reflect the final
    /// post-mutation state — the seam for callers that interleave their own work
    /// (IDL discovery, cache dumping) before execution. `execute` is
    /// `prepare().execute()` for callers that don't.
    pub fn prepare(self) -> Result<PreparedPipeline> {
        let mut loader = self.loader;
        let prepared_fundings = prepare_fundings(&mut loader, &self.resolved, &self.state)?;

        Ok(PreparedPipeline {
            config: self.config,
            parsed: self.parsed,
            loader,
            resolved: self.resolved,
            state: self.state,
            prepared_fundings,
        })
    }

    /// Execute the transaction simulation.
    pub fn execute(self) -> Result<SimulationResult> {
        self.prepare()?.execute()
    }
}

// ── Stage 3: prepared (single) ──

/// A single-transaction pipeline with mutations applied and all accounts
/// resolved; ready to `execute`. Callers may inspect the finalized state
/// (resolved accounts, post-mutation transaction) and use the shared RPC
/// [`provider`](PreparedPipeline::provider) before executing.
pub struct PreparedPipeline {
    config: PipelineConfig,
    parsed: ParsedTransaction,
    loader: AccountLoader,
    resolved: ResolvedAccounts,
    state: StateMutations,
    prepared_fundings: Vec<PreparedTokenFunding>,
}

impl PreparedPipeline {
    /// Access the resolved accounts (post-mutation, including any accounts an
    /// inserted instruction introduced).
    pub fn resolved(&self) -> &ResolvedAccounts {
        &self.resolved
    }

    /// Access the parsed transaction with mutations applied and its account plan
    /// recomputed.
    pub fn parsed(&self) -> &ParsedTransaction {
        &self.parsed
    }

    /// The token fundings prepared for this simulation, with mints and decimals
    /// resolved. Exposed so callers can render exactly what will be funded.
    pub fn token_fundings(&self) -> &[PreparedTokenFunding] {
        &self.prepared_fundings
    }

    /// The RPC account provider backing the pipeline's loader. Lets callers run
    /// extra fetches (e.g. fetching on-chain IDL accounts) against the same
    /// provider — and thus the same in-memory cache — used for account loading.
    pub fn provider(&self) -> Arc<dyn RpcAccountProvider> {
        self.loader.provider()
    }

    /// The RPC batch size configured on the pipeline's loader.
    pub fn rpc_batch_size(&self) -> usize {
        self.loader.rpc_batch_size()
    }

    /// Execute the transaction simulation, returning the high-level
    /// [`SimulationResult`] (with balance changes computed). Use
    /// [`execute_to_result`](Self::execute_to_result) when you need the raw
    /// [`ExecutionResult`] (pre/post account snapshots) instead.
    pub fn execute(self) -> Result<SimulationResult> {
        Ok(SimulationResult::from_execution(self.execute_to_result()?))
    }

    /// Execute the transaction simulation, returning the raw [`ExecutionResult`]
    /// with pre/post account snapshots — for callers that render or inspect the
    /// resulting state directly rather than the summarized [`SimulationResult`].
    pub fn execute_to_result(self) -> Result<ExecutionResult> {
        let mut runner =
            build_runner(&self.config, self.resolved, self.state, self.prepared_fundings)?;
        runner.execute(&self.parsed.transaction)
    }
}

// ── Stage 2: loaded (bundle) ──

/// A bundle pipeline with accounts loaded; call `prepare` (or `execute_bundle`)
/// to advance. Apply mutations earlier, at [`ParsedBundlePipeline`].
pub struct LoadedBundlePipeline {
    config: PipelineConfig,
    parsed: Vec<ParsedTransaction>,
    loader: AccountLoader,
    resolved: ResolvedAccounts,
    state: StateMutations,
}

impl LoadedBundlePipeline {
    /// Access the resolved accounts.
    pub fn resolved(&self) -> &ResolvedAccounts {
        &self.resolved
    }

    /// Resolve state mutations that need loaded context (token funding mints),
    /// advancing to a [`PreparedBundlePipeline`]. See [`LoadedPipeline::prepare`].
    pub fn prepare(self) -> Result<PreparedBundlePipeline> {
        let mut loader = self.loader;
        let prepared_fundings = prepare_fundings(&mut loader, &self.resolved, &self.state)?;

        Ok(PreparedBundlePipeline {
            config: self.config,
            parsed: self.parsed,
            loader,
            resolved: self.resolved,
            state: self.state,
            prepared_fundings,
        })
    }

    /// Execute the bundle of transactions sequentially.
    ///
    /// Returns a [`BundleResult`] containing results for all executed
    /// transactions and the total count. Check [`BundleResult::skipped_count`]
    /// to detect transactions that were never attempted due to a prior failure.
    pub fn execute_bundle(self) -> Result<BundleResult<Result<SimulationResult>>> {
        self.prepare()?.execute_bundle()
    }
}

// ── Stage 3: prepared (bundle) ──

/// A bundle pipeline with mutations applied and all accounts resolved; ready to
/// `execute_bundle`. See [`PreparedPipeline`] for the single-transaction analog.
pub struct PreparedBundlePipeline {
    config: PipelineConfig,
    parsed: Vec<ParsedTransaction>,
    loader: AccountLoader,
    resolved: ResolvedAccounts,
    state: StateMutations,
    prepared_fundings: Vec<PreparedTokenFunding>,
}

impl PreparedBundlePipeline {
    /// Access the resolved accounts (post-mutation).
    pub fn resolved(&self) -> &ResolvedAccounts {
        &self.resolved
    }

    /// Access the parsed transactions with mutations applied and account plans
    /// recomputed.
    pub fn parsed(&self) -> &[ParsedTransaction] {
        &self.parsed
    }

    /// The token fundings prepared for this bundle, with mints and decimals
    /// resolved. See [`PreparedPipeline::token_fundings`].
    pub fn token_fundings(&self) -> &[PreparedTokenFunding] {
        &self.prepared_fundings
    }

    /// The RPC account provider backing the pipeline's loader. See
    /// [`PreparedPipeline::provider`].
    pub fn provider(&self) -> Arc<dyn RpcAccountProvider> {
        self.loader.provider()
    }

    /// The RPC batch size configured on the pipeline's loader.
    pub fn rpc_batch_size(&self) -> usize {
        self.loader.rpc_batch_size()
    }

    /// Execute the bundle of transactions sequentially, returning a
    /// [`BundleResult`] of high-level [`SimulationResult`]s. Use
    /// [`execute_bundle_to_results`](Self::execute_bundle_to_results) for the raw
    /// [`ExecutionResult`]s.
    pub fn execute_bundle(self) -> Result<BundleResult<Result<SimulationResult>>> {
        let bundle = self.execute_bundle_to_results()?;
        let total = bundle.total();
        let mapped = bundle
            .into_executed()
            .into_iter()
            .map(|r| r.map(SimulationResult::from_execution))
            .collect();
        Ok(BundleResult::new(mapped, total))
    }

    /// Execute the bundle of transactions sequentially, returning the raw
    /// [`ExecutionResult`]s with pre/post account snapshots.
    pub fn execute_bundle_to_results(self) -> Result<BundleResult<Result<ExecutionResult>>> {
        let tx_refs: Vec<&VersionedTransaction> =
            self.parsed.iter().map(|t| &t.transaction).collect();
        let mut runner =
            build_runner(&self.config, self.resolved, self.state, self.prepared_fundings)?;
        Ok(runner.execute_bundle(&tx_refs))
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

    #[test]
    fn load_fetches_accounts_introduced_by_inserted_instruction() {
        // Transaction mutations apply at the parsed stage (before loading), so
        // `load_accounts` sees the post-mutation transaction and fetches the
        // accounts an inserted instruction references. Verify those keys are
        // requested from the provider.
        use std::sync::Mutex;

        use solana_account::AccountSharedData;

        use crate::rpc_provider::RpcAccountProvider;

        struct RecordingProvider {
            requested: Arc<Mutex<Vec<Pubkey>>>,
        }
        impl RpcAccountProvider for RecordingProvider {
            fn get_multiple_accounts(
                &self,
                pubkeys: &[Pubkey],
            ) -> Result<Vec<Option<AccountSharedData>>> {
                self.requested.lock().unwrap().extend_from_slice(pubkeys);
                Ok(vec![None; pubkeys.len()])
            }
        }

        let requested = Arc::new(Mutex::new(Vec::new()));
        let provider = Arc::new(RecordingProvider { requested: requested.clone() });

        // Two brand-new keys absent from the original transfer transaction.
        let inserted_program = Pubkey::new_unique();
        let inserted_account = Pubkey::new_unique();
        let insert_ix = solana_instruction::Instruction {
            program_id: inserted_program,
            accounts: vec![solana_instruction::AccountMeta::new_readonly(inserted_account, false)],
            data: vec![],
        };
        let mutations = Mutations::builder()
            .add_instruction_op(crate::types::InstructionOp::Insert {
                position: 0,
                instruction: insert_ix,
            })
            .build();

        // Mutations apply at the parsed stage, so `load_accounts` runs against the
        // post-mutation transaction and fetches the inserted instruction's keys.
        let _ = Pipeline::with_provider(provider)
            .parse(TEST_TX_BASE64)
            .unwrap()
            .with_mutations(mutations)
            .unwrap()
            .load_accounts();

        let requested = requested.lock().unwrap();
        assert!(
            requested.contains(&inserted_program),
            "inserted instruction's program must be fetched after mutation; got: {:?}",
            *requested
        );
        assert!(
            requested.contains(&inserted_account),
            "inserted instruction's account must be fetched after mutation; got: {:?}",
            *requested
        );
    }

    /// A fake provider that returns a funded system-owned account for every key
    /// in the parsed transaction, so the SVM has the state a simple transfer
    /// needs. Built from the parsed account plan rather than hardcoded pubkeys.
    fn funded_provider_for(
        parsed: &ParsedTransaction,
    ) -> Arc<crate::rpc_provider::FakeAccountProvider> {
        use std::collections::HashMap;

        use solana_account::{Account, AccountSharedData};
        use solana_sdk_ids::system_program;

        let mut accounts: HashMap<Pubkey, AccountSharedData> = HashMap::new();
        for key in &parsed.account_plan.static_accounts {
            accounts.insert(
                *key,
                AccountSharedData::from(Account {
                    lamports: 10_000_000_000,
                    data: vec![],
                    owner: system_program::id(),
                    executable: false,
                    rent_epoch: 0,
                }),
            );
        }
        Arc::new(crate::rpc_provider::FakeAccountProvider::new(accounts))
    }

    #[test]
    fn prepare_exposes_complete_state_before_execute() {
        // The prepared stage is the seam for between-load-and-execute work
        // (IDL discovery, cache dumping): it must expose the resolved accounts,
        // the parsed transaction, and the shared provider before `execute` runs.
        let parsed = Pipeline::new("http://localhost:8899".into()).parse(TEST_TX_BASE64).unwrap();
        let keys = parsed.parsed().account_plan.static_accounts.clone();
        let provider = funded_provider_for(parsed.parsed());

        let prepared = Pipeline::with_provider(provider)
            .parse(TEST_TX_BASE64)
            .unwrap()
            .load_accounts()
            .unwrap()
            .prepare()
            .unwrap();

        // The resolved set is inspectable before execution. Builtin programs
        // (e.g. the system program) are intentionally not fetched, but the
        // ordinary accounts the transaction touches — like the fee payer — are.
        let fee_payer = keys[0];
        assert!(
            prepared.resolved().accounts.contains_key(&fee_payer),
            "resolved set should expose the fee payer before execute"
        );
        // Parsed transaction and its account plan are available.
        assert_eq!(prepared.parsed().account_plan.static_accounts, keys);
        // No token fundings were requested.
        assert!(prepared.token_fundings().is_empty());
        // The shared provider is reachable for follow-on fetches.
        let _ = prepared.provider();
        let _ = prepared.rpc_batch_size();

        // And it still executes to completion from the prepared stage.
        let result = prepared.execute().unwrap();
        assert!(result.success, "transfer should succeed: {:?}", result.error);
    }

    #[test]
    fn with_loader_drives_pipeline_through_caller_supplied_loader() {
        let parsed = Pipeline::new("http://localhost:8899".into()).parse(TEST_TX_BASE64).unwrap();
        let loader = AccountLoader::with_provider(funded_provider_for(parsed.parsed()));

        let result = Pipeline::with_loader(loader)
            .parse(TEST_TX_BASE64)
            .unwrap()
            .load_accounts()
            .unwrap()
            .execute()
            .unwrap();
        assert!(result.success, "transfer should succeed: {:?}", result.error);
    }

    #[test]
    fn execute_without_mutations_does_not_refetch_accounts() {
        // When no transaction mutations are present, execute() must not perform
        // any extra fetches (the append-fetch step is skipped entirely).
        use std::sync::Mutex;

        use solana_account::AccountSharedData;

        use crate::rpc_provider::RpcAccountProvider;

        struct CountingProvider {
            requested: Arc<Mutex<Vec<Pubkey>>>,
        }
        impl RpcAccountProvider for CountingProvider {
            fn get_multiple_accounts(
                &self,
                pubkeys: &[Pubkey],
            ) -> Result<Vec<Option<AccountSharedData>>> {
                self.requested.lock().unwrap().extend_from_slice(pubkeys);
                Ok(vec![None; pubkeys.len()])
            }
        }

        let requested = Arc::new(Mutex::new(Vec::new()));
        let provider = Arc::new(CountingProvider { requested: requested.clone() });

        // No mutations: load_accounts fetches the original keys; execute()
        // should not append-fetch anything.
        let pipeline = Pipeline::with_provider(provider)
            .parse(TEST_TX_BASE64)
            .unwrap()
            .load_accounts()
            .unwrap();
        let load_keys = requested.lock().unwrap().clone();
        assert!(!load_keys.is_empty());
        let _ = pipeline.execute();
        let after = requested.lock().unwrap().clone();
        assert_eq!(
            after.len(),
            load_keys.len(),
            "execute() without mutations must not fetch any extra accounts; load fetched {:?}, after execute got {:?}",
            load_keys,
            after
        );
    }
}
