// crates/sonar-sim/src/result.rs

//! Public simulation result types with auto-computed balance changes.

use solana_message::inner_instruction::InnerInstructionsList;

pub use crate::balance_changes::{SolBalanceChange, TokenBalanceChange};
use crate::balance_changes::{
    compute_sol_changes, compute_token_changes, extract_mint_decimals_combined,
};
use crate::executor::{ExecutionResult, ExecutionStatus};

/// Result of a single transaction simulation.
///
/// Balance changes are computed automatically from pre/post account snapshots.
#[derive(Debug, Clone)]
pub struct SimulationResult {
    /// Whether the transaction executed successfully.
    pub success: bool,
    /// Error message if the transaction failed.
    pub error: Option<String>,
    /// Transaction execution logs.
    pub logs: Vec<String>,
    /// Compute units consumed.
    pub compute_units: u64,
    /// Return data as `(program_id, data)`, if any.
    pub return_data: Option<(String, Vec<u8>)>,
    /// Inner instructions emitted during execution.
    pub inner_instructions: InnerInstructionsList,
    /// SOL balance changes (sorted by absolute magnitude).
    pub sol_changes: Vec<SolBalanceChange>,
    /// Token balance changes (sorted by absolute magnitude).
    pub token_changes: Vec<TokenBalanceChange>,
}

impl SimulationResult {
    pub(crate) fn from_execution(exec: ExecutionResult) -> Self {
        // Compute balance changes from pre/post snapshots
        let sol_changes = compute_sol_changes(&exec.pre_accounts, &exec.post_accounts);
        let mint_decimals = extract_mint_decimals_combined(&exec.pre_accounts, &exec.post_accounts);
        let token_changes =
            compute_token_changes(&exec.pre_accounts, &exec.post_accounts, &mint_decimals);

        let (success, error) = match exec.status {
            ExecutionStatus::Succeeded => (true, None),
            ExecutionStatus::Failed(e) => (false, Some(e)),
        };

        let return_data = if exec.meta.return_data.data.is_empty() {
            None
        } else {
            Some((exec.meta.return_data.program_id.to_string(), exec.meta.return_data.data))
        };

        Self {
            success,
            error,
            logs: exec.meta.logs,
            compute_units: exec.meta.compute_units_consumed,
            return_data,
            inner_instructions: exec.meta.inner_instructions,
            sol_changes,
            token_changes,
        }
    }
}
