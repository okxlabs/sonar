use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use bincode;
use litesvm::{LiteSVM, types::TransactionMetadata};
use log::info;
use solana_account::{Account, AccountSharedData};

use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction::versioned::VersionedTransaction as LiteVersionedTransaction;

use crate::{
    account_loader::{ResolvedAccounts, ResolvedLookup},
    cli::{Funding, ProgramReplacement},
    funding::{PreparedTokenFunding, apply_sol_fundings},
};

pub struct TransactionExecutor {
    svm: LiteSVM,
    resolved: ResolvedAccounts,
    replacements: Vec<ProgramReplacement>,
    fundings: Vec<Funding>,
    token_fundings: Vec<PreparedTokenFunding>,
}

impl TransactionExecutor {
    pub fn prepare(
        resolved: ResolvedAccounts,
        replacements: Vec<ProgramReplacement>,
        fundings: Vec<Funding>,
        token_fundings: Vec<PreparedTokenFunding>,
        verify_signatures: bool,
    ) -> Result<Self> {
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
            info!(
                "Loading custom program {} => {}",
                replacement.program_id,
                replacement.so_path.display()
            );
            svm.add_program_from_file(replacement.program_id, &replacement.so_path).with_context(
                || {
                    format!(
                        "Failed to load replacement program `{}`, path: {}",
                        replacement.program_id,
                        replacement.so_path.display()
                    )
                },
            )?;
        }

        apply_sol_fundings(&mut svm, &fundings)?;

        Ok(Self { svm, resolved, replacements, fundings, token_fundings })
    }

    pub fn simulate(&mut self, tx: &VersionedTransaction) -> Result<SimulationResult> {
        let lite_tx = convert_versioned_transaction(tx)?;

        let outcome = self.svm.simulate_transaction(lite_tx);

        let simulation = match outcome {
            Ok(info) => SimulationResult {
                status: ExecutionStatus::Succeeded,
                meta: info.meta.clone(),
                post_accounts: info.post_accounts.into_iter().collect(),
            },
            Err(failure) => SimulationResult {
                status: ExecutionStatus::Failed(failure.err.to_string()),
                meta: failure.meta.clone(),
                post_accounts: HashMap::new(),
            },
        };

        Ok(simulation)
    }

    /// Execute a transaction and persist state changes to the SVM.
    /// Used for bundle simulation where tx1's effects should influence tx2.
    pub fn execute(&mut self, tx: &VersionedTransaction) -> Result<SimulationResult> {
        let lite_tx = convert_versioned_transaction(tx)?;

        let result = self.svm.send_transaction(lite_tx);

        let simulation = match result {
            litesvm::types::TransactionResult::Ok(info) => SimulationResult {
                status: ExecutionStatus::Succeeded,
                meta: info.clone(),
                post_accounts: HashMap::new(), // post_accounts not available from send_transaction
            },
            litesvm::types::TransactionResult::Err(failure) => SimulationResult {
                status: ExecutionStatus::Failed(failure.err.to_string()),
                meta: failure.meta.clone(),
                post_accounts: HashMap::new(),
            },
        };

        Ok(simulation)
    }

    /// Execute multiple transactions sequentially as a bundle.
    /// Each transaction's side effects persist and influence subsequent transactions.
    /// Stops execution immediately if any transaction fails (fail-fast).
    pub fn execute_bundle(&mut self, txs: &[&VersionedTransaction]) -> Vec<SimulationResult> {
        let mut results = Vec::with_capacity(txs.len());

        for tx in txs {
            let result = self.execute(tx).unwrap_or_else(|err| SimulationResult {
                status: ExecutionStatus::Failed(err.to_string()),
                meta: TransactionMetadata::default(),
                post_accounts: HashMap::new(),
            });

            let failed = matches!(result.status, ExecutionStatus::Failed(_));
            results.push(result);

            if failed {
                break; // Stop on first failure
            }
        }

        results
    }

    pub fn resolved_accounts(&self) -> &ResolvedAccounts {
        &self.resolved
    }

    pub fn replacements(&self) -> &[ProgramReplacement] {
        &self.replacements
    }

    pub fn fundings(&self) -> &[Funding] {
        &self.fundings
    }

    pub fn token_fundings(&self) -> &[PreparedTokenFunding] {
        &self.token_fundings
    }
}

#[derive(Debug)]
pub struct SimulationResult {
    pub status: ExecutionStatus,
    pub meta: TransactionMetadata,
    pub post_accounts: HashMap<Pubkey, AccountSharedData>,
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
        // Create a simple executor with minimal setup
        let payer = Keypair::new();
        let recipient1 = Pubkey::new_unique();
        let recipient2 = Pubkey::new_unique();

        // Create two transfer transactions
        let tx1 = create_transfer_transaction(&payer, &recipient1, 1000);
        let tx2 = create_transfer_transaction(&payer, &recipient2, 2000);

        let tx_refs: Vec<&VersionedTransaction> = vec![&tx1, &tx2];

        // Create a minimal resolved accounts structure
        let mut accounts = HashMap::new();
        // Add payer account with enough SOL
        accounts.insert(
            payer.pubkey(),
            Account {
                lamports: 10_000_000_000, // 10 SOL
                data: vec![],
                owner: solana_sdk_ids::system_program::id(),
                executable: false,
                rent_epoch: 0,
            },
        );

        let resolved = ResolvedAccounts { accounts, lookups: vec![] };

        let mut executor = TransactionExecutor::prepare(
            resolved,
            vec![],
            vec![],
            vec![],
            false, // don't verify signatures for test
        )
        .expect("Failed to prepare executor");

        let results = executor.execute_bundle(&tx_refs);

        // Should have 2 results for 2 transactions
        assert_eq!(results.len(), 2);
    }
}
