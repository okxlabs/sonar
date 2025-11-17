use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use bincode;
use litesvm::{LiteSVM, types::TransactionMetadata};
use log::info;
use solana_account::{Account, AccountSharedData, WritableAccount};

use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction::versioned::VersionedTransaction as LiteVersionedTransaction;

use crate::{
    account_loader::{ResolvedAccounts, ResolvedLookup},
    cli::{Funding, ProgramReplacement},
    funding::PreparedTokenFunding,
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

        // Apply funding to specified accounts
        for Funding { pubkey, amount_sol } in &fundings {
            let lamports = (amount_sol * 1_000_000_000.0) as u64;
            info!("Funding account {} with {} SOL ({} lamports)", pubkey, amount_sol, lamports);

            // Check if account already exists
            if let Some(existing_account) = svm.get_account(&pubkey) {
                // Update existing account with new balance
                let mut new_account = existing_account.clone();
                new_account.set_lamports(lamports);
                svm.set_account(*pubkey, new_account)?;
            } else {
                // Create new system account with the specified balance
                let system_program_id = solana_sdk_ids::system_program::id();
                let new_account = AccountSharedData::new(lamports, 0, &system_program_id);
                svm.set_account(*pubkey, new_account.into())?;
            }
        }

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
