use solana_message::inner_instruction::InnerInstructionsList;
use solana_pubkey::Pubkey;

/// Backend-agnostic return data from a simulated transaction.
///
/// Wraps the program id that produced the return data and the raw bytes.
/// This is sonar-sim's own type so the public API does not depend on any
/// particular SVM backend crate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReturnData {
    pub program_id: Pubkey,
    pub data: Vec<u8>,
}

/// Backend-agnostic metadata produced by a single transaction simulation.
///
/// Upper layers should use this instead of any backend-specific metadata
/// type (e.g. `litesvm::types::TransactionMetadata`).  The executor's
/// adapter layer converts the backend-native result into this struct.
#[derive(Debug, Clone, Default)]
pub struct SimulationMetadata {
    pub logs: Vec<String>,
    pub inner_instructions: InnerInstructionsList,
    pub compute_units_consumed: u64,
    pub return_data: ReturnData,
}
