use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use bincode;
use litesvm::{types::TransactionMetadata, LiteSVM};
use log::info;
use solana_account::{Account, AccountSharedData};

use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey as LitePubkey;
use solana_sdk::transaction::VersionedTransaction;
use solana_sdk_ids::bpf_loader_upgradeable;
use solana_transaction::versioned::VersionedTransaction as LiteVersionedTransaction;

use crate::{
    account_loader::{ResolvedAccounts, ResolvedLookup},
    cli::ProgramReplacement,
};

pub struct TransactionExecutor {
    svm: LiteSVM,
    resolved: ResolvedAccounts,
    replacements: Vec<ProgramReplacement>,
}

impl TransactionExecutor {
    pub fn prepare(
        resolved: ResolvedAccounts,
        replacements: Vec<ProgramReplacement>,
        verify_signatures: bool,
    ) -> Result<Self> {
        let mut svm = LiteSVM::new()
            .with_log_bytes_limit(Some(1024 * 1024 * 10)) // 10M
            .with_blockhash_check(false)
            .with_sigverify(verify_signatures);

        let mut ordered_accounts: Vec<_> = resolved.accounts.iter().collect();
        ordered_accounts.sort_by_key(|(_, account)| account_priority(account));

        for (pubkey, account) in ordered_accounts {
            let lite_pubkey = LitePubkey::from(pubkey.to_bytes());
            set_account(&mut svm, lite_pubkey, account.clone())?;
        }

        for replacement in &replacements {
            info!(
                "Loading custom program {} => {}",
                replacement.program_id,
                replacement.so_path.display()
            );
            let program_pubkey = LitePubkey::from(replacement.program_id.to_bytes());
            svm.add_program_from_file(program_pubkey, &replacement.so_path)
                .with_context(|| {
                    format!(
                        "Failed to load replacement program `{}`, path: {}",
                        replacement.program_id,
                        replacement.so_path.display()
                    )
                })?;
        }

        Ok(Self {
            svm,
            resolved,
            replacements,
        })
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
}

fn set_account(svm: &mut LiteSVM, pubkey: LitePubkey, account: Account) -> Result<()> {
    svm.set_account(pubkey, account)
        .map_err(|err| anyhow!("Failed to write account `{pubkey}` to LiteSVM: {err}"))
}

#[derive(Debug)]
pub struct SimulationResult {
    pub status: ExecutionStatus,
    pub meta: TransactionMetadata,
    pub post_accounts: HashMap<LitePubkey, AccountSharedData>,
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
    let bpf_loader_id =
        solana_sdk::pubkey::Pubkey::new_from_array(bpf_loader_upgradeable::id().to_bytes());
    if sdk_pubkey_from_lite(&account.owner) == bpf_loader_id {
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

fn sdk_pubkey_from_lite(pubkey: &LitePubkey) -> solana_sdk::pubkey::Pubkey {
    solana_sdk::pubkey::Pubkey::new_from_array(pubkey.to_bytes())
}
