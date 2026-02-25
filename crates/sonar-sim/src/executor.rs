use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use litesvm::LiteSVM;
use log::{info, warn};
use solana_account::{Account, AccountSharedData, ReadableAccount};
use solana_clock::Clock;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;
use solana_sysvar_id::SysvarId;
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction::versioned::VersionedTransaction as LiteVersionedTransaction;

use crate::error::{Result, SonarSimError};
use crate::funding::apply_sol_fundings;
use crate::types::{
    AccountDataPatch, Funding, PreparedTokenFunding, Replacement, ResolvedAccounts, ResolvedLookup,
    ReturnData, SimulationMetadata,
};

// ── Inlined native_ids ──

static NATIVE_PROGRAM_IDS: LazyLock<HashSet<Pubkey>> = LazyLock::new(|| {
    use solana_sdk_ids::*;
    HashSet::from([
        system_program::id(),
        bpf_loader::id(),
        bpf_loader_deprecated::id(),
        bpf_loader_upgradeable::id(),
        vote::id(),
        stake::id(),
        config::id(),
        compute_budget::id(),
        address_lookup_table::id(),
        ed25519_program::id(),
        secp256k1_program::id(),
        sysvar::clock::id(),
        sysvar::rent::id(),
        sysvar::slot_hashes::id(),
        sysvar::epoch_schedule::id(),
        sysvar::instructions::id(),
        sysvar::recent_blockhashes::id(),
    ])
});

pub fn is_native_or_sysvar(pubkey: &Pubkey) -> bool {
    NATIVE_PROGRAM_IDS.contains(pubkey)
}

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
    pub replacements: Vec<Replacement>,
    pub fundings: Vec<Funding>,
    pub token_fundings: Vec<PreparedTokenFunding>,
    pub data_patches: Vec<AccountDataPatch>,
}

/// Simulation execution options passed to `TransactionExecutor::prepare`.
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
    pub fn signature_verification(mut self, sv: SignatureVerification) -> Self {
        self.opts.execution.signature_verification = sv;
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

    pub fn replacements(mut self, replacements: Vec<Replacement>) -> Self {
        self.opts.mutations.replacements = replacements;
        self
    }

    pub fn fundings(mut self, fundings: Vec<Funding>) -> Self {
        self.opts.mutations.fundings = fundings;
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

// ── Pipeline steps ──
//
// Each function performs a single, testable stage of executor preparation.
// `TransactionExecutor::prepare` orchestrates them in order.

/// Load resolved accounts into SVM, ordered so that BPF upgradeable
/// ProgramData accounts are set before Program accounts that reference them.
pub fn load_accounts(svm: &mut LiteSVM, resolved: &ResolvedAccounts) -> Result<()> {
    let mut ordered: Vec<_> = resolved.accounts.iter().collect();
    ordered.sort_by_key(|(_, account)| account_priority(account));
    for (pubkey, account) in ordered {
        svm.set_account(*pubkey, Account::from(account.clone())).map_err(|e| {
            SonarSimError::Svm { reason: format!("Failed to set account {}: {}", pubkey, e) }
        })?;
    }
    Ok(())
}

/// Apply program (.so) and account (.json) replacements into SVM.
pub fn apply_replacements(
    svm: &mut LiteSVM,
    replacements: &[Replacement],
    resolved: &ResolvedAccounts,
) -> Result<()> {
    for replacement in replacements {
        match replacement {
            Replacement::Program { program_id, so_path } => {
                if let Some(existing) = resolved.accounts.get(program_id) {
                    if !existing.executable() {
                        warn!(
                            "--replace target {} does not appear to be a program on-chain. Loading .so file anyway.",
                            program_id
                        );
                    }
                }
                info!("Loading custom program {} => {}", program_id, so_path.display());
                svm.add_program_from_file(*program_id, so_path).map_err(|e| {
                    SonarSimError::Svm {
                        reason: format!(
                            "Failed to load replacement program `{}`, path: {}: {}",
                            program_id,
                            so_path.display(),
                            e
                        ),
                    }
                })?;
            }
            Replacement::Account { pubkey, account, source_path } => {
                if let Some(existing) = resolved.accounts.get(pubkey) {
                    if existing.executable() {
                        warn!(
                            "--replace target {} appears to be a program on-chain, but replacing as a regular account from JSON file.",
                            pubkey
                        );
                    }
                }
                info!("Loading custom account {} => {}", pubkey, source_path.display());
                svm.set_account(*pubkey, account.clone()).map_err(|e| SonarSimError::Svm {
                    reason: format!(
                        "Failed to set replacement account `{}`, path: {}: {}",
                        pubkey,
                        source_path.display(),
                        e
                    ),
                })?;
            }
        }
    }
    Ok(())
}

/// Apply byte-level data patches to accounts already loaded in SVM.
pub fn apply_data_patches(svm: &mut LiteSVM, patches: &[AccountDataPatch]) -> Result<()> {
    for patch in patches {
        let mut account = svm
            .get_account(&patch.pubkey)
            .ok_or(SonarSimError::AccountNotFound { pubkey: patch.pubkey })?;
        let end = patch.offset + patch.data.len();
        if end > account.data.len() {
            return Err(SonarSimError::AccountData {
                pubkey: Some(patch.pubkey),
                reason: format!(
                    "Patch range [{}..{}) exceeds account data length {} for {}",
                    patch.offset,
                    end,
                    account.data.len(),
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
        account.data[patch.offset..end].copy_from_slice(&patch.data);
        svm.set_account(patch.pubkey, account).map_err(|e| SonarSimError::Svm {
            reason: format!("Failed to set patched account `{}`: {}", patch.pubkey, e),
        })?;
    }
    Ok(())
}

/// Warp SVM clock to the given slot number.
pub fn apply_slot(svm: &mut LiteSVM, slot: u64) {
    info!("Warping SVM clock to slot {}", slot);
    svm.warp_to_slot(slot);
}

/// Override the Clock sysvar's `unix_timestamp` field.
pub fn apply_timestamp(svm: &mut LiteSVM, ts: i64) -> Result<()> {
    let clock_id = Clock::id();
    let clock_account = svm.get_account(&clock_id).ok_or_else(|| SonarSimError::Svm {
        reason: "Clock sysvar account not found in SVM".into(),
    })?;
    let mut clock: Clock = bincode::deserialize(&clock_account.data).map_err(|e| {
        SonarSimError::Serialization { reason: format!("Failed to deserialize Clock sysvar: {e}") }
    })?;
    info!("Overriding Clock unix_timestamp: {} -> {}", clock.unix_timestamp, ts);
    clock.unix_timestamp = ts;
    let data = bincode::serialize(&clock).map_err(|e| SonarSimError::Serialization {
        reason: format!("Failed to serialize modified Clock sysvar: {e}"),
    })?;
    let updated_account = Account { data, ..clock_account };
    svm.set_account(clock_id, updated_account).map_err(|e| SonarSimError::Svm {
        reason: format!("Failed to set modified Clock sysvar: {e}"),
    })?;
    Ok(())
}

pub struct TransactionExecutor {
    svm: LiteSVM,
    resolved: ResolvedAccounts,
    replacements: Vec<Replacement>,
    fundings: Vec<Funding>,
    token_fundings: Vec<PreparedTokenFunding>,
    #[allow(dead_code)]
    data_patches: Vec<AccountDataPatch>,
}

impl TransactionExecutor {
    pub fn prepare(resolved: ResolvedAccounts, opts: SimulationOptions) -> Result<Self> {
        let exec = &opts.execution;
        let mutations = &opts.mutations;

        let mut svm = LiteSVM::new()
            .with_log_bytes_limit(Some(1024 * 1024 * 10)) // 10M
            .with_blockhash_check(false)
            .with_sigverify(exec.signature_verification.is_verify());

        load_accounts(&mut svm, &resolved)?;
        apply_replacements(&mut svm, &mutations.replacements, &resolved)?;
        apply_sol_fundings(&mut svm, &mutations.fundings)?;
        apply_data_patches(&mut svm, &mutations.data_patches)?;

        if let Some(slot) = exec.slot {
            apply_slot(&mut svm, slot);
        }
        if let Some(ts) = exec.timestamp {
            apply_timestamp(&mut svm, ts)?;
        }

        Ok(Self {
            svm,
            resolved,
            replacements: opts.mutations.replacements,
            fundings: opts.mutations.fundings,
            token_fundings: opts.mutations.token_fundings,
            data_patches: opts.mutations.data_patches,
        })
    }

    /// Execute a transaction and persist state changes to the SVM.
    ///
    /// Takes pre/post account snapshots so that balance changes can be
    /// computed even when accounts are closed during execution.
    pub fn execute(&mut self, tx: &VersionedTransaction) -> Result<SimulationResult> {
        let account_keys = self.collect_transaction_accounts(tx);
        let pre_accounts = self.snapshot_accounts(&account_keys);

        let lite_tx = convert_versioned_transaction(tx)?;
        let result = self.svm.send_transaction(lite_tx);

        let post_accounts = self.snapshot_accounts(&account_keys);

        let simulation = match result {
            litesvm::types::TransactionResult::Ok(info) => SimulationResult {
                status: ExecutionStatus::Succeeded,
                meta: convert_metadata(&info),
                post_accounts,
                pre_accounts,
            },
            litesvm::types::TransactionResult::Err(failure) => SimulationResult {
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
                snapshot.insert(*key, account.into());
            }
        }
        snapshot
    }

    /// Execute multiple transactions sequentially as a bundle.
    pub fn execute_bundle(&mut self, txs: &[&VersionedTransaction]) -> Vec<SimulationResult> {
        let mut results = Vec::with_capacity(txs.len());

        for tx in txs {
            let result = self.execute(tx).unwrap_or_else(|err| SimulationResult {
                status: ExecutionStatus::Failed(err.to_string()),
                meta: SimulationMetadata::default(),
                post_accounts: HashMap::new(),
                pre_accounts: HashMap::new(),
            });

            let failed = matches!(result.status, ExecutionStatus::Failed(_));
            results.push(result);

            if failed {
                break;
            }
        }

        results
    }

    pub fn resolved_accounts(&self) -> &ResolvedAccounts {
        &self.resolved
    }

    pub fn replacements(&self) -> &[Replacement] {
        &self.replacements
    }

    pub fn fundings(&self) -> &[Funding] {
        &self.fundings
    }

    pub fn token_fundings(&self) -> &[PreparedTokenFunding] {
        &self.token_fundings
    }

    #[allow(dead_code)]
    pub fn data_patches(&self) -> &[AccountDataPatch] {
        &self.data_patches
    }
}

#[derive(Debug)]
pub struct SimulationResult {
    pub status: ExecutionStatus,
    pub meta: SimulationMetadata,
    pub post_accounts: HashMap<Pubkey, AccountSharedData>,
    pub pre_accounts: HashMap<Pubkey, AccountSharedData>,
}

fn convert_metadata(m: &litesvm::types::TransactionMetadata) -> SimulationMetadata {
    SimulationMetadata {
        logs: m.logs.clone(),
        inner_instructions: m.inner_instructions.clone(),
        compute_units_consumed: m.compute_units_consumed,
        return_data: ReturnData {
            program_id: m.return_data.program_id,
            data: m.return_data.data.clone(),
        },
    }
}

#[derive(Debug)]
pub enum ExecutionStatus {
    Succeeded,
    Failed(String),
}

impl ResolvedAccounts {
    pub fn lookup_details(&self) -> &[ResolvedLookup] {
        &self.lookups
    }
}

fn convert_versioned_transaction(tx: &VersionedTransaction) -> Result<LiteVersionedTransaction> {
    let bytes = bincode::serialize(tx).map_err(|err| SonarSimError::Serialization {
        reason: format!("Failed to serialize transaction: {err}"),
    })?;
    bincode::deserialize(&bytes).map_err(|err| SonarSimError::Serialization {
        reason: format!("Failed to convert transaction format: {err}"),
    })
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

        let mut executor = TransactionExecutor::prepare(resolved, SimulationOptions::default())
            .expect("Failed to prepare executor");

        let results = executor.execute_bundle(&tx_refs);

        assert_eq!(results.len(), 2);
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
        let mut executor = TransactionExecutor::prepare(resolved, SimulationOptions::default())
            .expect("prepare should succeed");

        let results = executor.execute_bundle(&tx_refs);

        assert_eq!(results.len(), 1, "bundle should stop after first failure");
        assert!(matches!(results[0].status, ExecutionStatus::Failed(_)));
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
        let mut executor = TransactionExecutor::prepare(resolved, SimulationOptions::default())
            .expect("prepare should succeed");

        let results = executor.execute_bundle(&tx_refs);
        assert_eq!(results.len(), 2);
        assert!(matches!(results[0].status, ExecutionStatus::Succeeded));
        assert!(matches!(results[1].status, ExecutionStatus::Succeeded));
    }

    #[test]
    fn execute_single_transaction_succeeds() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let tx = create_transfer_transaction(&payer, &recipient, 1_000);

        let mut accounts = HashMap::new();
        accounts.insert(payer.pubkey(), shared_system_account(10_000_000_000));

        let resolved = ResolvedAccounts { accounts, lookups: vec![] };
        let mut executor = TransactionExecutor::prepare(resolved, SimulationOptions::default())
            .expect("prepare should succeed");

        let result = executor.execute(&tx).expect("execute should not error");
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
            .fundings(vec![Funding { pubkey: payer.pubkey(), amount_lamports: 10_000_000_000 }])
            .build();

        let mut executor =
            TransactionExecutor::prepare(resolved, opts).expect("prepare should succeed");

        let result = executor.execute(&tx).expect("execute should not error");
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
    fn pipeline_apply_replacements_account() {
        let mut svm = new_svm();
        let key = Pubkey::new_unique();

        let original = AccountSharedData::from(Account {
            lamports: 100,
            data: vec![0],
            owner: solana_sdk_ids::system_program::id(),
            executable: false,
            rent_epoch: 0,
        });
        let replacement_account = Account {
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

        let replacements = vec![Replacement::Account {
            pubkey: key,
            account: replacement_account,
            source_path: std::path::PathBuf::from("test.json"),
        }];
        apply_replacements(&mut svm, &replacements, &resolved).expect("should apply replacement");

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
}
