use std::collections::HashMap;
use std::fmt;

use litesvm::LiteSVM;
use litesvm::types::{TransactionMetadata, TransactionResult};
use log::{info, warn};
use solana_account::{AccountSharedData, ReadableAccount, WritableAccount};
use solana_clock::Clock;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;
use solana_sysvar_id::SysvarId;
use solana_transaction::versioned::VersionedTransaction;

use crate::error::{Result, SonarSimError};
use crate::funding::{apply_sol_fundings, apply_token_fundings};
use crate::svm_backend::SvmBackend;
use crate::types::{
    AccountDataPatch, AccountOverride, PreparedTokenFunding, ResolvedAccounts, ReturnData,
    SimulationMetadata, SolFunding,
};

/// Whether the simulator should verify transaction signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignatureVerification {
    /// Verify all signatures (strict mode).
    Verify,
    /// Skip signature verification (default for local simulation).
    #[default]
    Skip,
}

impl SignatureVerification {
    pub fn is_verify(self) -> bool {
        matches!(self, Self::Verify)
    }
}

impl From<bool> for SignatureVerification {
    fn from(verify: bool) -> Self {
        if verify { Self::Verify } else { Self::Skip }
    }
}

impl fmt::Display for SignatureVerification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verify => f.write_str("verify"),
            Self::Skip => f.write_str("skip"),
        }
    }
}

/// Controls *how* the simulation VM executes the transaction.
#[derive(Debug, Clone, Default)]
pub struct ExecutionOptions {
    pub signature_verification: SignatureVerification,
    pub slot: Option<u64>,
    pub timestamp: Option<i64>,
}

/// Pre-simulation mutations applied to the account set before execution.
#[derive(Debug, Clone, Default)]
pub struct StateMutationOptions {
    pub account_closures: Vec<Pubkey>,
    pub overrides: Vec<AccountOverride>,
    pub sol_fundings: Vec<SolFunding>,
    pub token_fundings: Vec<PreparedTokenFunding>,
    pub data_patches: Vec<AccountDataPatch>,
}

/// Simulation execution options passed to [`PreparedSimulation::prepare`].
///
/// Groups two concerns:
/// - [`ExecutionOptions`]: VM-level knobs (signature verification, slot/time override).
/// - [`StateMutationOptions`]: pre-simulation account mutations (replace, fund, patch).
///
/// Construct via `Default`, struct literal, or the builder returned by
/// [`SimulationOptions::builder`].
#[derive(Debug, Clone, Default)]
pub struct SimulationOptions {
    pub execution: ExecutionOptions,
    pub mutations: StateMutationOptions,
}

/// Incremental builder for [`SimulationOptions`].
#[derive(Debug, Clone, Default)]
pub struct SimulationOptionsBuilder {
    opts: SimulationOptions,
}

impl SimulationOptions {
    pub fn builder() -> SimulationOptionsBuilder {
        SimulationOptionsBuilder::default()
    }
}

impl SimulationOptionsBuilder {
    pub fn signature_verification(mut self, verification: SignatureVerification) -> Self {
        self.opts.execution.signature_verification = verification;
        self
    }

    pub fn slot(mut self, slot: u64) -> Self {
        self.opts.execution.slot = Some(slot);
        self
    }

    pub fn timestamp(mut self, ts: i64) -> Self {
        self.opts.execution.timestamp = Some(ts);
        self
    }

    pub fn account_closures(mut self, account_closures: Vec<Pubkey>) -> Self {
        self.opts.mutations.account_closures = account_closures;
        self
    }

    pub fn overrides(mut self, overrides: Vec<AccountOverride>) -> Self {
        self.opts.mutations.overrides = overrides;
        self
    }

    pub fn sol_fundings(mut self, sol_fundings: Vec<SolFunding>) -> Self {
        self.opts.mutations.sol_fundings = sol_fundings;
        self
    }

    pub fn token_fundings(mut self, token_fundings: Vec<PreparedTokenFunding>) -> Self {
        self.opts.mutations.token_fundings = token_fundings;
        self
    }

    pub fn data_patches(mut self, data_patches: Vec<AccountDataPatch>) -> Self {
        self.opts.mutations.data_patches = data_patches;
        self
    }

    pub fn build(self) -> SimulationOptions {
        self.opts
    }
}

impl StateMutationOptions {
    /// Apply all pre-simulation account mutations in deterministic order.
    pub fn apply<B: SvmBackend + ?Sized>(
        &self,
        svm: &mut B,
        resolved: &ResolvedAccounts,
    ) -> Result<()> {
        apply_account_closures(svm, &self.account_closures)?;
        apply_overrides(svm, &self.overrides, resolved)?;
        apply_sol_fundings(svm, &self.sol_fundings)?;
        apply_data_patches(svm, &self.data_patches)?;
        apply_token_fundings(svm, &self.token_fundings, resolved)?;
        Ok(())
    }
}

// ── Pipeline steps ──
//
// Each function performs a single, testable stage of preparation.
// `PreparedSimulation::prepare` orchestrates them in order.

/// Load resolved accounts into SVM, ordered so that BPF upgradeable
/// ProgramData accounts are set before Program accounts that reference them.
pub fn load_accounts<B: SvmBackend + ?Sized>(
    svm: &mut B,
    resolved: &ResolvedAccounts,
) -> Result<()> {
    let mut ordered: Vec<_> = resolved.accounts.iter().collect();
    ordered.sort_by_key(|(_, account)| account_priority(account));
    for (pubkey, account) in ordered {
        svm.set_account(*pubkey, account.clone()).map_err(|e| SonarSimError::Svm {
            reason: format!("Failed to set account {}: {}", pubkey, e),
        })?;
    }
    Ok(())
}

/// Close accounts by setting them to zero lamports.
///
/// On LiteSVM, zero-lamport accounts are removed from its internal
/// `AccountsDb`, so `get_account` returns `None` afterwards.
/// Other `SvmBackend` implementations may retain the zero-lamport account.
pub fn apply_account_closures<B: SvmBackend + ?Sized>(
    svm: &mut B,
    closures: &[Pubkey],
) -> Result<()> {
    for pubkey in closures {
        if svm.get_account(pubkey).is_none() {
            warn!("Account closure target {} does not exist in SVM; skipping.", pubkey);
            continue;
        }
        info!("Closing account {} (setting to zero lamports)", pubkey);
        svm.set_account(*pubkey, AccountSharedData::default()).map_err(|e| SonarSimError::Svm {
            reason: format!("Failed to close account `{}`: {}", pubkey, e),
        })?;
    }
    Ok(())
}

/// Apply program (.so) and account (.json) overrides into SVM.
pub fn apply_overrides<B: SvmBackend + ?Sized>(
    svm: &mut B,
    overrides: &[AccountOverride],
    resolved: &ResolvedAccounts,
) -> Result<()> {
    for entry in overrides {
        match entry {
            AccountOverride::Program { program_id, so_path } => {
                if let Some(existing) = resolved.accounts.get(program_id) {
                    if !existing.executable() {
                        warn!(
                            "--override target {} does not appear to be a program on-chain. Loading .so file anyway.",
                            program_id
                        );
                    }
                }
                info!("Loading custom program {} => {}", program_id, so_path.display());
                svm.add_program_from_file(*program_id, so_path).map_err(|e| {
                    SonarSimError::Svm {
                        reason: format!(
                            "Failed to load override program `{}`, path: {}: {}",
                            program_id,
                            so_path.display(),
                            e
                        ),
                    }
                })?;
            }
            AccountOverride::Account { pubkey, account, source_path } => {
                if let Some(existing) = resolved.accounts.get(pubkey) {
                    if existing.executable() {
                        warn!(
                            "--override target {} appears to be a program on-chain, but overriding as a regular account from JSON file.",
                            pubkey
                        );
                    }
                }
                info!("Loading custom account {} => {}", pubkey, source_path.display());
                svm.set_account(*pubkey, AccountSharedData::from(account.clone())).map_err(
                    |e| SonarSimError::Svm {
                        reason: format!(
                            "Failed to set override account `{}`, path: {}: {}",
                            pubkey,
                            source_path.display(),
                            e
                        ),
                    },
                )?;
            }
        }
    }
    Ok(())
}

/// Apply byte-level data patches to accounts already loaded in SVM.
pub fn apply_data_patches<B: SvmBackend + ?Sized>(
    svm: &mut B,
    patches: &[AccountDataPatch],
) -> Result<()> {
    for patch in patches {
        let mut account = svm
            .get_account(&patch.pubkey)
            .ok_or(SonarSimError::AccountNotFound { pubkey: patch.pubkey })?;
        let end = patch.offset + patch.data.len();
        if end > account.data().len() {
            return Err(SonarSimError::AccountData {
                pubkey: Some(patch.pubkey),
                reason: format!(
                    "Patch range [{}..{}) exceeds account data length {} for {}",
                    patch.offset,
                    end,
                    account.data().len(),
                    patch.pubkey
                ),
            });
        }
        info!(
            "Patching account {} data[{}..{}] ({} bytes)",
            patch.pubkey,
            patch.offset,
            end,
            patch.data.len()
        );
        account.data_as_mut_slice()[patch.offset..end].copy_from_slice(&patch.data);
        svm.set_account(patch.pubkey, account).map_err(|e| SonarSimError::Svm {
            reason: format!("Failed to set patched account `{}`: {}", patch.pubkey, e),
        })?;
    }
    Ok(())
}

/// Warp SVM clock to the given slot number.
pub fn apply_slot<B: SvmBackend + ?Sized>(svm: &mut B, slot: u64) {
    info!("Warping SVM clock to slot {}", slot);
    svm.warp_to_slot(slot);
}

/// Override the Clock sysvar's `unix_timestamp` field.
pub fn apply_timestamp<B: SvmBackend + ?Sized>(svm: &mut B, ts: i64) -> Result<()> {
    let clock_id = Clock::id();
    let mut clock_account = svm.get_account(&clock_id).ok_or_else(|| SonarSimError::Svm {
        reason: "Clock sysvar account not found in SVM".into(),
    })?;
    let mut clock: Clock = bincode::deserialize(clock_account.data()).map_err(|e| {
        SonarSimError::Serialization { reason: format!("Failed to deserialize Clock sysvar: {e}") }
    })?;
    info!("Overriding Clock unix_timestamp: {} -> {}", clock.unix_timestamp, ts);
    clock.unix_timestamp = ts;
    let data = bincode::serialize(&clock).map_err(|e| SonarSimError::Serialization {
        reason: format!("Failed to serialize modified Clock sysvar: {e}"),
    })?;
    clock_account.set_data_from_slice(&data);
    svm.set_account(clock_id, clock_account).map_err(|e| SonarSimError::Svm {
        reason: format!("Failed to set modified Clock sysvar: {e}"),
    })?;
    Ok(())
}

/// Immutable prepared simulation state.
///
/// This type captures the pre-execution snapshot after loading accounts,
/// applying mutations, and optional slot/timestamp overrides.
/// Convert into [`SimulationRunner`] to execute transactions.
pub struct PreparedSimulation<B: SvmBackend = LiteSVM> {
    svm: B,
    resolved: ResolvedAccounts,
    record: StateMutationOptions,
}

impl PreparedSimulation<LiteSVM> {
    pub fn prepare(resolved: ResolvedAccounts, opts: SimulationOptions) -> Result<Self> {
        let signature_verification = opts.execution.signature_verification;
        let svm = LiteSVM::new()
            .with_log_bytes_limit(Some(1024 * 1024 * 10)) // 10M
            .with_blockhash_check(false)
            .with_sigverify(signature_verification.is_verify());

        Self::prepare_with_backend(svm, resolved, opts)
    }
}

impl<B: SvmBackend> PreparedSimulation<B> {
    pub fn prepare_with_backend(
        mut svm: B,
        resolved: ResolvedAccounts,
        opts: SimulationOptions,
    ) -> Result<Self> {
        let SimulationOptions { execution, mutations } = opts;

        load_accounts(&mut svm, &resolved)?;
        mutations.apply(&mut svm, &resolved)?;
        let record = mutations;

        if let Some(slot) = execution.slot {
            apply_slot(&mut svm, slot);
        }
        if let Some(ts) = execution.timestamp {
            apply_timestamp(&mut svm, ts)?;
        }

        Ok(Self { svm, resolved, record })
    }

    pub fn into_runner(self) -> SimulationRunner<B> {
        SimulationRunner { svm: self.svm, resolved: self.resolved, record: self.record }
    }

    pub fn resolved_accounts(&self) -> &ResolvedAccounts {
        &self.resolved
    }

    pub fn account_closures(&self) -> &[Pubkey] {
        &self.record.account_closures
    }

    pub fn overrides(&self) -> &[AccountOverride] {
        &self.record.overrides
    }

    pub fn sol_fundings(&self) -> &[SolFunding] {
        &self.record.sol_fundings
    }

    pub fn token_fundings(&self) -> &[PreparedTokenFunding] {
        &self.record.token_fundings
    }

    pub fn data_patches(&self) -> &[AccountDataPatch] {
        &self.record.data_patches
    }
}

/// Result of a bundle execution with fail-fast semantics.
///
/// The `executed` vec may be shorter than `total` if a transaction failed
/// and remaining transactions were skipped.
#[derive(Debug, Clone)]
pub struct BundleResult<T> {
    executed: Vec<T>,
    total: usize,
}

impl<T> BundleResult<T> {
    pub(crate) fn new(executed: Vec<T>, total: usize) -> Self {
        Self { executed, total }
    }

    /// Results for transactions that were actually executed.
    pub fn executed(&self) -> &[T] {
        &self.executed
    }

    /// Consume self and return the executed results.
    pub fn into_executed(self) -> Vec<T> {
        self.executed
    }

    /// Total number of transactions in the bundle input.
    pub fn total(&self) -> usize {
        self.total
    }

    /// How many transactions were never attempted due to a prior failure.
    pub fn skipped_count(&self) -> usize {
        self.total.saturating_sub(self.executed.len())
    }

    /// True if all transactions were attempted (even if some failed).
    pub fn is_complete(&self) -> bool {
        self.executed.len() == self.total
    }
}

/// Mutable simulation engine that executes transactions on prepared state.
pub struct SimulationRunner<B: SvmBackend = LiteSVM> {
    svm: B,
    resolved: ResolvedAccounts,
    record: StateMutationOptions,
}

impl<B: SvmBackend> SimulationRunner<B> {
    /// Execute a transaction and persist state changes to the SVM.
    ///
    /// Takes pre/post account snapshots so that balance changes can be
    /// computed even when accounts are closed during execution.
    pub fn execute(&mut self, tx: &VersionedTransaction) -> Result<ExecutionResult> {
        let account_keys = self.collect_transaction_accounts(tx);
        let pre_accounts = self.snapshot_accounts(&account_keys);

        let result = self.svm.send_transaction(tx.clone());

        let post_accounts = self.snapshot_accounts(&account_keys);

        let simulation = match result {
            TransactionResult::Ok(info) => ExecutionResult {
                status: ExecutionStatus::Succeeded,
                meta: convert_metadata(&info),
                post_accounts,
                pre_accounts,
            },
            TransactionResult::Err(failure) => ExecutionResult {
                status: ExecutionStatus::Failed(failure.err.to_string()),
                meta: convert_metadata(&failure.meta),
                post_accounts,
                pre_accounts,
            },
        };

        Ok(simulation)
    }

    fn collect_transaction_accounts(&self, tx: &VersionedTransaction) -> Vec<Pubkey> {
        let mut keys: Vec<Pubkey> = tx.message.static_account_keys().to_vec();

        if let Some(lookups) = tx.message.address_table_lookups() {
            for lookup in lookups {
                for resolved_lookup in &self.resolved.lookups {
                    if resolved_lookup.account_key == lookup.account_key {
                        keys.extend(resolved_lookup.writable_addresses.iter().cloned());
                        keys.extend(resolved_lookup.readonly_addresses.iter().cloned());
                    }
                }
            }
        }

        let mut seen = std::collections::HashSet::new();
        keys.retain(|k| seen.insert(*k));
        keys
    }

    fn snapshot_accounts(&self, keys: &[Pubkey]) -> HashMap<Pubkey, AccountSharedData> {
        let mut snapshot = HashMap::new();
        for key in keys {
            if let Some(account) = self.svm.get_account(key) {
                snapshot.insert(*key, account);
            }
        }
        snapshot
    }

    /// Execute multiple transactions sequentially as a bundle.
    ///
    /// Uses fail-fast semantics: stops executing remaining transactions after
    /// the first failure. Use [`BundleResult::skipped_count`] to detect how
    /// many transactions were never attempted.
    pub fn execute_bundle(
        &mut self,
        txs: &[&VersionedTransaction],
    ) -> BundleResult<Result<ExecutionResult>> {
        let mut results = Vec::with_capacity(txs.len());

        for tx in txs {
            let result = self.execute(tx);
            let should_stop = match &result {
                Ok(simulation) => matches!(simulation.status, ExecutionStatus::Failed(_)),
                Err(_) => true,
            };
            results.push(result);

            if should_stop {
                break;
            }
        }

        BundleResult::new(results, txs.len())
    }

    pub fn resolved_accounts(&self) -> &ResolvedAccounts {
        &self.resolved
    }

    pub fn account_closures(&self) -> &[Pubkey] {
        &self.record.account_closures
    }

    pub fn overrides(&self) -> &[AccountOverride] {
        &self.record.overrides
    }

    pub fn sol_fundings(&self) -> &[SolFunding] {
        &self.record.sol_fundings
    }

    pub fn token_fundings(&self) -> &[PreparedTokenFunding] {
        &self.record.token_fundings
    }

    pub fn data_patches(&self) -> &[AccountDataPatch] {
        &self.record.data_patches
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub status: ExecutionStatus,
    pub meta: SimulationMetadata,
    pub post_accounts: HashMap<Pubkey, AccountSharedData>,
    pub pre_accounts: HashMap<Pubkey, AccountSharedData>,
}

fn convert_metadata(meta: &TransactionMetadata) -> SimulationMetadata {
    SimulationMetadata {
        logs: meta.logs.clone(),
        inner_instructions: meta.inner_instructions.clone(),
        compute_units_consumed: meta.compute_units_consumed,
        return_data: ReturnData {
            program_id: meta.return_data.program_id,
            data: meta.return_data.data.clone(),
        },
    }
}

#[derive(Debug, Clone)]
pub enum ExecutionStatus {
    Succeeded,
    Failed(String),
}

impl fmt::Display for ExecutionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Succeeded => f.write_str("succeeded"),
            Self::Failed(reason) => write!(f, "failed: {reason}"),
        }
    }
}

fn account_priority(account: &AccountSharedData) -> u8 {
    if *account.owner() == bpf_loader_upgradeable::id() {
        if let Ok(state) = bincode::deserialize::<UpgradeableLoaderState>(account.data()) {
            return match state {
                UpgradeableLoaderState::ProgramData { .. } => 0,
                UpgradeableLoaderState::Program { .. } => 2,
                _ => 1,
            };
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::MockSvm;
    use solana_account::Account;
    use solana_hash::Hash;
    use solana_keypair::Keypair;
    use solana_message::Message;
    use solana_signer::Signer;
    use solana_system_interface::instruction as system_instruction;
    use solana_transaction::Transaction;

    fn create_transfer_transaction(
        payer: &Keypair,
        recipient: &Pubkey,
        lamports: u64,
    ) -> VersionedTransaction {
        let blockhash = Hash::new_unique();
        let instruction = system_instruction::transfer(&payer.pubkey(), recipient, lamports);
        let message = Message::new(&[instruction], Some(&payer.pubkey()));
        let transaction = Transaction::new(&[payer], message, blockhash);
        VersionedTransaction::from(transaction)
    }

    fn shared_system_account(lamports: u64) -> AccountSharedData {
        AccountSharedData::from(Account {
            lamports,
            data: vec![],
            owner: solana_sdk_ids::system_program::id(),
            executable: false,
            rent_epoch: 0,
        })
    }

    #[test]
    fn test_execute_bundle_returns_results_for_each_transaction() {
        let payer = Keypair::new();
        let recipient1 = Pubkey::new_unique();
        let recipient2 = Pubkey::new_unique();

        let tx1 = create_transfer_transaction(&payer, &recipient1, 1000);
        let tx2 = create_transfer_transaction(&payer, &recipient2, 2000);

        let tx_refs: Vec<&VersionedTransaction> = vec![&tx1, &tx2];

        let mut accounts = HashMap::new();
        accounts.insert(payer.pubkey(), shared_system_account(10_000_000_000));

        let resolved = ResolvedAccounts { accounts, lookups: vec![] };

        let mut runner = PreparedSimulation::prepare(resolved, SimulationOptions::default())
            .expect("Failed to prepare simulation")
            .into_runner();

        let results = runner.execute_bundle(&tx_refs);

        assert_eq!(results.executed().len(), 2);
        assert!(results.executed().iter().all(|r| r.is_ok()));
        assert!(results.is_complete());
    }

    #[test]
    fn bundle_fail_fast_stops_on_first_failure() {
        let payer = Keypair::new();
        let recipient1 = Pubkey::new_unique();
        let recipient2 = Pubkey::new_unique();

        let tx1 = create_transfer_transaction(&payer, &recipient1, 999_999_999_999);
        let tx2 = create_transfer_transaction(&payer, &recipient2, 1000);

        let tx_refs: Vec<&VersionedTransaction> = vec![&tx1, &tx2];

        let mut accounts = HashMap::new();
        accounts.insert(payer.pubkey(), shared_system_account(1_000_000));

        let resolved = ResolvedAccounts { accounts, lookups: vec![] };
        let mut runner = PreparedSimulation::prepare(resolved, SimulationOptions::default())
            .expect("prepare should succeed")
            .into_runner();

        let results = runner.execute_bundle(&tx_refs);

        assert_eq!(results.executed().len(), 1, "bundle should stop after first failure");
        assert!(matches!(
            results.executed()[0].as_ref().map(|r| &r.status),
            Ok(&ExecutionStatus::Failed(_))
        ));
        assert_eq!(results.skipped_count(), 1);
        assert!(!results.is_complete());
    }

    #[test]
    fn bundle_sequential_state_propagation() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let tx1 = create_transfer_transaction(&payer, &recipient, 1_000);
        let tx2 = create_transfer_transaction(&payer, &recipient, 2_000);

        let tx_refs: Vec<&VersionedTransaction> = vec![&tx1, &tx2];

        let mut accounts = HashMap::new();
        accounts.insert(payer.pubkey(), shared_system_account(10_000_000_000));

        let resolved = ResolvedAccounts { accounts, lookups: vec![] };
        let mut runner = PreparedSimulation::prepare(resolved, SimulationOptions::default())
            .expect("prepare should succeed")
            .into_runner();

        let results = runner.execute_bundle(&tx_refs);
        assert_eq!(results.executed().len(), 2);
        assert!(matches!(
            results.executed()[0].as_ref().map(|r| &r.status),
            Ok(&ExecutionStatus::Succeeded)
        ));
        assert!(matches!(
            results.executed()[1].as_ref().map(|r| &r.status),
            Ok(&ExecutionStatus::Succeeded)
        ));
        assert!(results.is_complete());
    }

    #[test]
    fn execute_single_transaction_succeeds() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let tx = create_transfer_transaction(&payer, &recipient, 1_000);

        let mut accounts = HashMap::new();
        accounts.insert(payer.pubkey(), shared_system_account(10_000_000_000));

        let resolved = ResolvedAccounts { accounts, lookups: vec![] };
        let mut runner = PreparedSimulation::prepare(resolved, SimulationOptions::default())
            .expect("prepare should succeed")
            .into_runner();

        let result = runner.execute(&tx).expect("execute should not error");
        assert!(matches!(result.status, ExecutionStatus::Succeeded));
        assert!(!result.pre_accounts.is_empty());
        assert!(!result.post_accounts.is_empty());
    }

    #[test]
    fn execute_with_fundings() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let tx = create_transfer_transaction(&payer, &recipient, 1_000);

        let accounts = HashMap::new();
        let resolved = ResolvedAccounts { accounts, lookups: vec![] };

        let opts = SimulationOptions::builder()
            .sol_fundings(vec![SolFunding {
                pubkey: payer.pubkey(),
                amount_lamports: 10_000_000_000,
            }])
            .build();

        let mut runner = PreparedSimulation::prepare(resolved, opts)
            .expect("prepare should succeed")
            .into_runner();

        let result = runner.execute(&tx).expect("execute should not error");
        assert!(matches!(result.status, ExecutionStatus::Succeeded));
    }

    #[test]
    fn signature_verification_display() {
        assert_eq!(SignatureVerification::Verify.to_string(), "verify");
        assert_eq!(SignatureVerification::Skip.to_string(), "skip");
    }

    #[test]
    fn execution_status_display() {
        assert_eq!(ExecutionStatus::Succeeded.to_string(), "succeeded");
        assert_eq!(
            ExecutionStatus::Failed("insufficient funds".to_string()).to_string(),
            "failed: insufficient funds"
        );
    }

    #[test]
    fn prepared_simulation_into_runner_executes() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();
        let tx = create_transfer_transaction(&payer, &recipient, 1_000);

        let mut accounts = HashMap::new();
        accounts.insert(payer.pubkey(), shared_system_account(10_000_000_000));
        let resolved = ResolvedAccounts { accounts, lookups: vec![] };

        let prepared =
            PreparedSimulation::prepare(resolved, SimulationOptions::default()).expect("prepare");
        let mut runner = prepared.into_runner();
        let result = runner.execute(&tx).expect("execute should not error");
        assert!(matches!(result.status, ExecutionStatus::Succeeded));
    }

    // ── Pipeline step tests ──

    fn new_svm() -> LiteSVM {
        LiteSVM::new().with_blockhash_check(false).with_sigverify(false)
    }

    #[test]
    fn pipeline_load_accounts_sets_all_accounts() {
        let mut svm = new_svm();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();

        let mut accounts = HashMap::new();
        accounts.insert(
            key1,
            AccountSharedData::from(Account {
                lamports: 100,
                data: vec![1, 2],
                owner: solana_sdk_ids::system_program::id(),
                executable: false,
                rent_epoch: 0,
            }),
        );
        accounts.insert(
            key2,
            AccountSharedData::from(Account {
                lamports: 200,
                data: vec![3, 4],
                owner: solana_sdk_ids::system_program::id(),
                executable: false,
                rent_epoch: 0,
            }),
        );
        let resolved = ResolvedAccounts { accounts, lookups: vec![] };

        load_accounts(&mut svm, &resolved).expect("load_accounts should succeed");

        let loaded1 = svm.get_account(&key1).expect("key1 should exist");
        assert_eq!(loaded1.lamports, 100);
        assert_eq!(loaded1.data, vec![1, 2]);

        let loaded2 = svm.get_account(&key2).expect("key2 should exist");
        assert_eq!(loaded2.lamports, 200);
    }

    #[test]
    fn pipeline_apply_overrides_account() {
        let mut svm = new_svm();
        let key = Pubkey::new_unique();

        let original = AccountSharedData::from(Account {
            lamports: 100,
            data: vec![0],
            owner: solana_sdk_ids::system_program::id(),
            executable: false,
            rent_epoch: 0,
        });
        let override_account = Account {
            lamports: 999,
            data: vec![42],
            owner: solana_sdk_ids::system_program::id(),
            executable: false,
            rent_epoch: 0,
        };

        let mut accounts = HashMap::new();
        accounts.insert(key, original);
        let resolved = ResolvedAccounts { accounts, lookups: vec![] };

        load_accounts(&mut svm, &resolved).unwrap();

        let overrides = vec![AccountOverride::Account {
            pubkey: key,
            account: override_account,
            source_path: std::path::PathBuf::from("test.json"),
        }];
        apply_overrides(&mut svm, &overrides, &resolved).expect("should apply override");

        let loaded = svm.get_account(&key).expect("key should exist");
        assert_eq!(loaded.lamports, 999);
        assert_eq!(loaded.data, vec![42]);
    }

    #[test]
    fn pipeline_apply_data_patches_success() {
        let mut svm = new_svm();
        let key = Pubkey::new_unique();

        let account = Account {
            lamports: 100,
            data: vec![0, 0, 0, 0, 0],
            owner: solana_sdk_ids::system_program::id(),
            executable: false,
            rent_epoch: 0,
        };
        svm.set_account(key, account).unwrap();

        let patches = vec![AccountDataPatch { pubkey: key, offset: 1, data: vec![0xAA, 0xBB] }];
        apply_data_patches(&mut svm, &patches).expect("patches should apply");

        let loaded = svm.get_account(&key).expect("key should exist");
        assert_eq!(loaded.data, vec![0, 0xAA, 0xBB, 0, 0]);
    }

    #[test]
    fn pipeline_apply_data_patches_out_of_bounds() {
        let mut svm = new_svm();
        let key = Pubkey::new_unique();

        let account = Account {
            lamports: 100,
            data: vec![0, 0],
            owner: solana_sdk_ids::system_program::id(),
            executable: false,
            rent_epoch: 0,
        };
        svm.set_account(key, account).unwrap();

        let patches =
            vec![AccountDataPatch { pubkey: key, offset: 1, data: vec![0xAA, 0xBB, 0xCC] }];
        let err = apply_data_patches(&mut svm, &patches).unwrap_err();
        assert!(err.to_string().contains("exceeds account data length"));
    }

    #[test]
    fn pipeline_apply_data_patches_missing_account() {
        let mut svm = new_svm();
        let key = Pubkey::new_unique();

        let patches = vec![AccountDataPatch { pubkey: key, offset: 0, data: vec![1] }];
        let err = apply_data_patches(&mut svm, &patches).unwrap_err();
        assert!(err.to_string().contains("Account not found"));
    }

    #[test]
    fn pipeline_apply_slot() {
        let mut svm = new_svm();
        apply_slot(&mut svm, 12345);

        let clock_account = svm.get_account(&Clock::id()).expect("Clock sysvar should exist");
        let clock: Clock = bincode::deserialize(&clock_account.data).expect("valid Clock");
        assert_eq!(clock.slot, 12345);
    }

    #[test]
    fn pipeline_apply_timestamp() {
        let mut svm = new_svm();
        let target_ts: i64 = 1_700_000_000;

        apply_timestamp(&mut svm, target_ts).expect("apply_timestamp should succeed");

        let clock_account = svm.get_account(&Clock::id()).expect("Clock sysvar should exist");
        let clock: Clock = bincode::deserialize(&clock_account.data).expect("valid Clock");
        assert_eq!(clock.unix_timestamp, target_ts);
    }

    #[test]
    fn pipeline_slot_then_timestamp() {
        let mut svm = new_svm();
        apply_slot(&mut svm, 500);
        apply_timestamp(&mut svm, 1_600_000_000).expect("timestamp after slot should work");

        let clock_account = svm.get_account(&Clock::id()).expect("Clock sysvar should exist");
        let clock: Clock = bincode::deserialize(&clock_account.data).expect("valid Clock");
        assert_eq!(clock.slot, 500);
        assert_eq!(clock.unix_timestamp, 1_600_000_000);
    }

    // ── MockSvm-based error-path & round-trip tests ──

    #[test]
    fn load_accounts_propagates_set_account_error() {
        let key = Pubkey::new_unique();
        let mut accounts = HashMap::new();
        accounts.insert(
            key,
            AccountSharedData::from(Account {
                lamports: 100,
                data: vec![],
                owner: solana_sdk_ids::system_program::id(),
                executable: false,
                rent_epoch: 0,
            }),
        );
        let resolved = ResolvedAccounts { accounts, lookups: vec![] };

        let mut svm = MockSvm::new();
        svm.fail_set_account = Some("disk full".into());

        let err = load_accounts(&mut svm, &resolved).unwrap_err();
        assert!(err.to_string().contains("disk full"));
    }

    #[test]
    fn apply_data_patches_on_mock_writes_exact_bytes() {
        let key = Pubkey::new_unique();
        let account = Account {
            lamports: 100,
            data: vec![0, 0, 0, 0, 0],
            owner: solana_sdk_ids::system_program::id(),
            executable: false,
            rent_epoch: 0,
        };
        let mut svm = MockSvm::new().with_account(key, account);

        let patches = vec![AccountDataPatch { pubkey: key, offset: 1, data: vec![0xAA, 0xBB] }];
        apply_data_patches(&mut svm, &patches).expect("should apply");

        let loaded = svm.get_account(&key).unwrap();
        assert_eq!(loaded.data(), &[0, 0xAA, 0xBB, 0, 0]);
    }

    #[test]
    fn apply_timestamp_on_mock_round_trips_clock() {
        let clock = Clock {
            slot: 0,
            epoch_start_timestamp: 0,
            epoch: 0,
            leader_schedule_epoch: 0,
            unix_timestamp: 1_000_000,
        };
        let clock_data = bincode::serialize(&clock).unwrap();
        let clock_account = Account {
            lamports: 1,
            data: clock_data,
            owner: solana_sdk_ids::sysvar::id(),
            executable: false,
            rent_epoch: 0,
        };
        let mut svm = MockSvm::new().with_account(Clock::id(), clock_account);

        apply_timestamp(&mut svm, 1_700_000_000).unwrap();

        let updated = svm.get_account(&Clock::id()).unwrap();
        let updated_clock: Clock = bincode::deserialize(updated.data()).unwrap();
        assert_eq!(updated_clock.unix_timestamp, 1_700_000_000);
    }

    #[test]
    fn warp_to_slot_on_mock_updates_slot() {
        let mut svm = MockSvm::new();
        apply_slot(&mut svm, 42);
        assert_eq!(svm.slot, 42);
    }

    // ── Account closure tests ──

    #[test]
    fn pipeline_close_account_removes_from_litesvm() {
        let mut svm = new_svm();
        let key = Pubkey::new_unique();

        let account = Account {
            lamports: 1_000_000,
            data: vec![1, 2, 3],
            owner: solana_sdk_ids::system_program::id(),
            executable: false,
            rent_epoch: 0,
        };
        svm.set_account(key, account).unwrap();
        assert!(svm.get_account(&key).is_some());

        apply_account_closures(&mut svm, &[key]).expect("closure should succeed");
        assert!(svm.get_account(&key).is_none(), "account should no longer exist");
    }

    #[test]
    fn pipeline_close_nonexistent_account_is_noop() {
        let mut svm = new_svm();
        let key = Pubkey::new_unique();

        // Should not error, just warn
        apply_account_closures(&mut svm, &[key])
            .expect("closure of missing account should succeed");
        assert!(svm.get_account(&key).is_none());
    }

    #[test]
    fn pipeline_close_multiple_accounts() {
        let mut svm = new_svm();
        let k1 = Pubkey::new_unique();
        let k2 = Pubkey::new_unique();

        for key in [k1, k2] {
            svm.set_account(
                key,
                Account {
                    lamports: 100,
                    data: vec![],
                    owner: solana_sdk_ids::system_program::id(),
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        }

        apply_account_closures(&mut svm, &[k1, k2]).expect("closures should succeed");
        assert!(svm.get_account(&k1).is_none());
        assert!(svm.get_account(&k2).is_none());
    }

    #[test]
    fn pipeline_close_then_fund_creates_fresh_account() {
        let mut svm = new_svm();
        let key = Pubkey::new_unique();

        let account = Account {
            lamports: 1_000_000,
            data: vec![1, 2, 3, 4],
            owner: Pubkey::new_unique(),
            executable: false,
            rent_epoch: 0,
        };
        svm.set_account(key, account).unwrap();

        // Close first, then fund — mimics the mutation pipeline order
        apply_account_closures(&mut svm, &[key]).unwrap();
        assert!(svm.get_account(&key).is_none());

        apply_sol_fundings(&mut svm, &[SolFunding { pubkey: key, amount_lamports: 500 }]).unwrap();
        let created = svm.get_account(&key).expect("account should exist after funding");
        assert_eq!(created.lamports, 500);
        assert!(created.data.is_empty(), "newly funded account should have empty data");
    }

    #[test]
    fn close_account_on_mock_removes_account() {
        let key = Pubkey::new_unique();
        let account = Account {
            lamports: 100,
            data: vec![42],
            owner: solana_sdk_ids::system_program::id(),
            executable: false,
            rent_epoch: 0,
        };
        let mut svm = MockSvm::new().with_account(key, account);

        apply_account_closures(&mut svm, &[key]).expect("should close");
        // MockSvm stores the zero-lamport account (doesn't auto-remove like LiteSVM)
        let closed = svm.get_account(&key).unwrap();
        assert_eq!(closed.lamports(), 0);
    }
}
