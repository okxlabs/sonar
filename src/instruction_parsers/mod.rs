use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use solana_pubkey::Pubkey;

use crate::transaction::InstructionSummary;

/// Represents a parsed instruction with human-readable data and account names
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedInstruction {
    /// The instruction name (e.g., "Transfer", "CreateAccount")
    pub name: String,
    /// Vector of (field_name, field_value) pairs for display
    pub fields: Vec<(String, String)>,
    /// Human-readable names for each account in the instruction
    pub account_names: Vec<String>,
}

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
}

/// Registry of instruction parsers for well-known programs
pub struct ParserRegistry {
    parsers: HashMap<Pubkey, Box<dyn InstructionParser>>,
}

impl ParserRegistry {
    /// Creates a new parser registry with default parsers
    pub fn new() -> Self {
        let mut registry = Self { parsers: HashMap::new() };

        // Register default parsers
        let system_parser = SystemProgramParser::new();
        registry.parsers.insert(*system_parser.program_id(), Box::new(system_parser));

        // Register Token Program parser
        let token_parser = TokenProgramParser::new();
        registry.parsers.insert(*token_parser.program_id(), Box::new(token_parser));

        // Register Compute Budget parser
        let compute_budget_parser = ComputeBudgetParser::new();
        registry
            .parsers
            .insert(*compute_budget_parser.program_id(), Box::new(compute_budget_parser));

        // Register Token2022 parser
        let token2022_parser = Token2022ProgramParser::new();
        registry.parsers.insert(*token2022_parser.program_id(), Box::new(token2022_parser));

        registry
    }

    /// Registers a new instruction parser
    #[allow(dead_code)]
    pub fn register(&mut self, parser: Box<dyn InstructionParser>) {
        let program_id = *parser.program_id();
        self.parsers.insert(program_id, parser);
    }

    /// Attempts to parse an instruction using registered parsers
    /// Returns the first successful parse result
    pub fn parse_instruction(
        &self,
        instruction: &InstructionSummary,
        program_id: &Pubkey,
    ) -> Option<ParsedInstruction> {
        if let Some(parser) = self.parsers.get(program_id) {
            match parser.parse_instruction(instruction) {
                Ok(parsed) => parsed,
                Err(err) => {
                    log::warn!("Instruction parsing failed: {}", err);
                    None
                }
            }
        } else {
            None
        }
    }
}

impl Default for ParserRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_pubkey::Pubkey;

    #[test]
    fn test_token2022_parser_registration() {
        let registry = ParserRegistry::new();
        let token2022_id = Pubkey::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

        // The Token2022 parser should be registered
        assert!(registry.parsers.contains_key(&token2022_id));
    }

    #[test]
    fn test_token2022_parser_program_id() {
        let parser = Token2022ProgramParser::new();
        let expected_id = Pubkey::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

        assert_eq!(*parser.program_id(), expected_id);
    }
}

mod system_program;
pub use system_program::SystemProgramParser;

mod token_program;
pub use token_program::TokenProgramParser;

mod compute_budget;
pub use compute_budget::ComputeBudgetParser;

mod token2022_program;
pub use token2022_program::Token2022ProgramParser;
