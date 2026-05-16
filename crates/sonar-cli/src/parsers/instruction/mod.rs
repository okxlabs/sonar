use anyhow::Result;
use solana_pubkey::Pubkey;

use crate::core::transaction::InstructionSummary;

// ── Types ──

mod types;
pub use types::*;

// ── Helpers ──

/// Append numbered account names for accounts beyond the named set.
///
/// For example, if an instruction has 5 accounts but only 3 are named,
/// this appends `{prefix}1` and `{prefix}2` for the remaining accounts.
pub(crate) fn append_extra_account_names(
    account_names: &mut Vec<String>,
    total_accounts: usize,
    named_accounts: usize,
    prefix: &str,
) {
    for i in 0..(total_accounts.saturating_sub(named_accounts)) {
        account_names.push(format!("{}{}", prefix, i + 1));
    }
}

/// Generate parser struct boilerplate: struct definition, `new()`, and `Default`.
///
/// Usage: `define_parser!(MyProgramParser, "ProgramId1111...");`
/// Then implement `InstructionParser` manually (the struct has a `program_id` field).
macro_rules! define_parser {
    ($name:ident, $program_id:expr) => {
        pub struct $name {
            program_id: Pubkey,
        }

        impl $name {
            pub fn new() -> Self {
                Self { program_id: Pubkey::from_str_const($program_id) }
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

// ── Trait ──

/// Trait for parsing instructions of specific Solana programs
pub trait InstructionParser: Send + Sync {
    /// Returns the program ID this parser handles
    fn program_id(&self) -> &Pubkey;

    /// Attempts to parse the given instruction
    /// Returns Ok(Some(parsed)) if this parser can handle the instruction
    /// Returns Ok(None) if the instruction is not recognized by this parser
    /// Returns Err if parsing fails due to invalid data
    fn parse_instruction(
        &self,
        instruction: &InstructionSummary,
    ) -> Result<Option<ParsedInstruction>>;

    /// Attempt to parse a CPI event if this parser supports it
    fn parse_cpi_event(
        &self,
        _instruction: &InstructionSummary,
        _program_id: &Pubkey,
    ) -> Result<Option<ParsedInstruction>> {
        Ok(None) // Default: not supported
    }
}

// ── Program parsers ──

mod system_program;
pub use system_program::SystemProgramParser;

mod token2022_program;
pub use token2022_program::Token2022ProgramParser;

mod compute_budget_program;
pub use compute_budget_program::ComputeBudgetProgramParser;

mod associated_token_program;
pub use associated_token_program::AssociatedTokenProgramParser;

mod memo_program;
pub use memo_program::MemoProgramParser;

pub mod anchor_idl;

// ── Registry ──

mod registry;
pub use registry::ParserRegistry;
