use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use anyhow::{Context, Result, anyhow};
use litesvm::{LiteSVM, types::TransactionMetadata};
use log::{info, warn};
use solana_account::{Account, AccountSharedData};
use solana_clock::Clock;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;
use solana_sysvar_id::SysvarId;
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction::versioned::VersionedTransaction as LiteVersionedTransaction;

use crate::funding::apply_sol_fundings;
use crate::types::{
    AccountDataPatch, Funding, PreparedTokenFunding, Replacement, ResolvedAccounts, ResolvedLookup,
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

/// Simulation execution options passed to `TransactionExecutor::prepare`.
#[derive(Default)]
pub struct SimulationOptions {
    pub replacements: Vec<Replacement>,
    pub fundings: Vec<Funding>,
    pub token_fundings: Vec<PreparedTokenFunding>,
    pub data_patches: Vec<AccountDataPatch>,
    pub verify_signatures: bool,
    pub slot: Option<u64>,
    pub timestamp: Option<i64>,
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
        let SimulationOptions {
            replacements,
            fundings,
            token_fundings,
            data_patches,
            verify_signatures,
            slot,
            timestamp,
        } = opts;
        let mut svm = LiteSVM::new()
            .with_log_bytes_limit(Some(1024 * 1024 * 10)) // 10M
            .with_blockhash_check(false)
            .with_sigverify(verify_signatures);

        let mut ordered_accounts: Vec<_> = resolved.accounts.iter().collect();
        ordered_accounts.sort_by_key(|(_, account)| account_priority(account));

        for (pubkey, account) in ordered_accounts {
            svm.set_account(*pubkey, account.clone())?;
        }

        for replacement in &replacements {
            match replacement {
                Replacement::Program { program_id, so_path } => {
                    if let Some(existing) = resolved.accounts.get(program_id) {
                        if !existing.executable {
                            warn!(
                                "--replace target {} does not appear to be a program on-chain. Loading .so file anyway.",
                                program_id
                            );
                        }
                    }
                    info!("Loading custom program {} => {}", program_id, so_path.display());
                    svm.add_program_from_file(*program_id, so_path).with_context(|| {
                        format!(
                            "Failed to load replacement program `{}`, path: {}",
                            program_id,
                            so_path.display()
                        )
                    })?;
                }
                Replacement::Account { pubkey, account, source_path } => {
                    if let Some(existing) = resolved.accounts.get(pubkey) {
                        if existing.executable {
                            warn!(
                                "--replace target {} appears to be a program on-chain, but replacing as a regular account from JSON file.",
                                pubkey
                            );
                        }
                    }
                    info!("Loading custom account {} => {}", pubkey, source_path.display());
                    svm.set_account(*pubkey, account.clone()).with_context(|| {
                        format!(
                            "Failed to set replacement account `{}`, path: {}",
                            pubkey,
                            source_path.display()
                        )
                    })?;
                }
            }
        }

        apply_sol_fundings(&mut svm, &fundings)?;

        // Apply data patches (byte-level writes to account data)
        for patch in &data_patches {
            let mut account = svm.get_account(&patch.pubkey).ok_or_else(|| {
                anyhow!("--patch-account-data target {} not found in SVM", patch.pubkey)
            })?;
            let end = patch.offset + patch.data.len();
            if end > account.data.len() {
                return Err(anyhow!(
                    "Patch range [{}..{}) exceeds account data length {} for {}",
                    patch.offset,
                    end,
                    account.data.len(),
                    patch.pubkey
                ));
            }
            info!(
                "Patching account {} data[{}..{}] ({} bytes)",
                patch.pubkey,
                patch.offset,
                end,
                patch.data.len()
            );
            account.data[patch.offset..end].copy_from_slice(&patch.data);
            svm.set_account(patch.pubkey, account)
                .with_context(|| format!("Failed to set patched account `{}`", patch.pubkey))?;
        }

        // Apply slot override (warp SVM clock to specified slot)
        if let Some(slot) = slot {
            info!("Warping SVM clock to slot {}", slot);
            svm.warp_to_slot(slot);
        }

        // Apply timestamp override (modify Clock sysvar's unix_timestamp)
        if let Some(ts) = timestamp {
            let clock_id = Clock::id();
            let clock_account = svm
                .get_account(&clock_id)
                .ok_or_else(|| anyhow!("Clock sysvar account not found in SVM"))?;
            let mut clock: Clock = bincode::deserialize(&clock_account.data)
                .context("Failed to deserialize Clock sysvar")?;
            info!("Overriding Clock unix_timestamp: {} -> {}", clock.unix_timestamp, ts);
            clock.unix_timestamp = ts;
            let data =
                bincode::serialize(&clock).context("Failed to serialize modified Clock sysvar")?;
            let updated_account = Account { data, ..clock_account };
            svm.set_account(clock_id, updated_account)
                .context("Failed to set modified Clock sysvar")?;
        }

        Ok(Self { svm, resolved, replacements, fundings, token_fundings, data_patches })
    }

    pub fn simulate(&mut self, tx: &VersionedTransaction) -> Result<SimulationResult> {
        let lite_tx = convert_versioned_transaction(tx)?;

        let outcome = self.svm.simulate_transaction(lite_tx);

        let simulation = match outcome {
            Ok(info) => SimulationResult {
                status: ExecutionStatus::Succeeded,
                meta: info.meta.clone(),
                post_accounts: info.post_accounts.into_iter().collect(),
                pre_accounts: HashMap::new(),
            },
            Err(failure) => SimulationResult {
                status: ExecutionStatus::Failed(failure.err.to_string()),
                meta: failure.meta.clone(),
                post_accounts: HashMap::new(),
                pre_accounts: HashMap::new(),
            },
        };

        Ok(simulation)
    }

    /// Execute a transaction and persist state changes to the SVM.
    /// Used for bundle simulation where tx1's effects should influence tx2.
    pub fn execute(&mut self, tx: &VersionedTransaction) -> Result<SimulationResult> {
        let account_keys = self.collect_transaction_accounts(tx);
        let pre_accounts = self.snapshot_accounts(&account_keys);

        let lite_tx = convert_versioned_transaction(tx)?;
        let result = self.svm.send_transaction(lite_tx);

        let post_accounts = self.snapshot_accounts(&account_keys);

        let simulation = match result {
            litesvm::types::TransactionResult::Ok(info) => SimulationResult {
                status: ExecutionStatus::Succeeded,
                meta: info.clone(),
                post_accounts,
                pre_accounts,
            },
            litesvm::types::TransactionResult::Err(failure) => SimulationResult {
                status: ExecutionStatus::Failed(failure.err.to_string()),
                meta: failure.meta.clone(),
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
                meta: TransactionMetadata::default(),
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
    pub meta: TransactionMetadata,
    pub post_accounts: HashMap<Pubkey, AccountSharedData>,
    pub pre_accounts: HashMap<Pubkey, AccountSharedData>,
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
    let bytes =
        bincode::serialize(tx).map_err(|err| anyhow!("Failed to serialize transaction: {err}"))?;
    bincode::deserialize(&bytes)
        .map_err(|err| anyhow!("Failed to convert transaction format: {err}"))
}

fn account_priority(account: &Account) -> u8 {
    if account.owner == bpf_loader_upgradeable::id() {
        if let Ok(state) = bincode::deserialize::<UpgradeableLoaderState>(account.data.as_slice()) {
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

    #[test]
    fn test_execute_bundle_returns_results_for_each_transaction() {
        let payer = Keypair::new();
        let recipient1 = Pubkey::new_unique();
        let recipient2 = Pubkey::new_unique();

        let tx1 = create_transfer_transaction(&payer, &recipient1, 1000);
        let tx2 = create_transfer_transaction(&payer, &recipient2, 2000);

        let tx_refs: Vec<&VersionedTransaction> = vec![&tx1, &tx2];

        let mut accounts = HashMap::new();
        accounts.insert(
            payer.pubkey(),
            Account {
                lamports: 10_000_000_000,
                data: vec![],
                owner: solana_sdk_ids::system_program::id(),
                executable: false,
                rent_epoch: 0,
            },
        );

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
        accounts.insert(
            payer.pubkey(),
            Account {
                lamports: 1_000_000,
                data: vec![],
                owner: solana_sdk_ids::system_program::id(),
                executable: false,
                rent_epoch: 0,
            },
        );

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
        accounts.insert(
            payer.pubkey(),
            Account {
                lamports: 10_000_000_000,
                data: vec![],
                owner: solana_sdk_ids::system_program::id(),
                executable: false,
                rent_epoch: 0,
            },
        );

        let resolved = ResolvedAccounts { accounts, lookups: vec![] };
        let mut executor = TransactionExecutor::prepare(resolved, SimulationOptions::default())
            .expect("prepare should succeed");

        let results = executor.execute_bundle(&tx_refs);
        assert_eq!(results.len(), 2);
        assert!(matches!(results[0].status, ExecutionStatus::Succeeded));
        assert!(matches!(results[1].status, ExecutionStatus::Succeeded));
    }

    #[test]
    fn simulate_single_transaction_succeeds() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let tx = create_transfer_transaction(&payer, &recipient, 1_000);

        let mut accounts = HashMap::new();
        accounts.insert(
            payer.pubkey(),
            Account {
                lamports: 10_000_000_000,
                data: vec![],
                owner: solana_sdk_ids::system_program::id(),
                executable: false,
                rent_epoch: 0,
            },
        );

        let resolved = ResolvedAccounts { accounts, lookups: vec![] };
        let mut executor = TransactionExecutor::prepare(resolved, SimulationOptions::default())
            .expect("prepare should succeed");

        let result = executor.simulate(&tx).expect("simulate should not error");
        assert!(matches!(result.status, ExecutionStatus::Succeeded));
    }

    #[test]
    fn simulate_with_fundings() {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();

        let tx = create_transfer_transaction(&payer, &recipient, 1_000);

        let accounts = HashMap::new();
        let resolved = ResolvedAccounts { accounts, lookups: vec![] };

        let opts = SimulationOptions {
            fundings: vec![Funding { pubkey: payer.pubkey(), amount_lamports: 10_000_000_000 }],
            ..Default::default()
        };

        let mut executor =
            TransactionExecutor::prepare(resolved, opts).expect("prepare should succeed");

        let result = executor.simulate(&tx).expect("simulate should not error");
        assert!(matches!(result.status, ExecutionStatus::Succeeded));
    }
}
