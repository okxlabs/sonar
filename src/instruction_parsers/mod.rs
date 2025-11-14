use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
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

    /// Downcast to Any for type-specific operations
    fn as_any(&self) -> &dyn std::any::Any {
        panic!("as_any not implemented for this parser")
    }
}

/// Registry of instruction parsers for well-known programs
pub struct ParserRegistry {
    parsers: HashMap<Pubkey, Box<dyn InstructionParser>>,
    /// Optional IDL directory path for lazy loading IDL parsers
    idl_directory: Option<std::path::PathBuf>,
}

impl ParserRegistry {
    /// Creates a new parser registry with default parsers
    pub fn new(idl_directory: Option<std::path::PathBuf>) -> Self {
        let mut registry = Self { parsers: HashMap::new(), idl_directory };

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

        // Register Associated Token Program parser
        let associated_token_parser = AssociatedTokenProgramParser::new();
        registry
            .parsers
            .insert(*associated_token_parser.program_id(), Box::new(associated_token_parser));

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
        Self::new(None)
    }
}

impl ParserRegistry {
    /// Register IDL-based parsers from the IDL registry
    pub fn register_idl_parsers(
        &mut self,
        idl_registry: &crate::instruction_parsers::anchor_idl::IdlRegistry,
    ) {
        let parsers =
            crate::instruction_parsers::anchor_idl::create_parsers_from_idl_registry(idl_registry);
        for parser in parsers {
            let program_id = *parser.program_id();
            self.parsers.insert(program_id, parser);
        }
    }

    /// Get the number of registered parsers
    pub fn parser_count(&self) -> usize {
        self.parsers.len()
    }

    /// Load IDL parser for a specific program ID if needed
    pub fn load_idl_parser_if_needed(&mut self, program_id: &Pubkey) -> Result<bool> {
        // If parser already exists, return early
        if self.parsers.contains_key(program_id) {
            return Ok(false);
        }

        // If no IDL directory is configured, return early
        let idl_dir = match &self.idl_directory {
            Some(dir) => dir,
            None => return Ok(false),
        };

        // Look for IDL file matching the program ID
        let idl_file_path = idl_dir.join(format!("{}.json", program_id));
        if !idl_file_path.exists() {
            return Ok(false);
        }

        log::info!("Lazy-loading IDL for program: {}", program_id);

        // Load the specific IDL file
        let idl = std::fs::read_to_string(&idl_file_path)
            .with_context(|| format!("Failed to read IDL file: {}", idl_file_path.display()))?;

        let idl_data: crate::instruction_parsers::anchor_idl::CompleteIdl =
            serde_json::from_str(&idl).with_context(|| {
                format!("Failed to parse IDL JSON: {}", idl_file_path.display())
            })?;

        // Create parser from the IDL
        // Also create an IDL registry to store this IDL for CPI event parsing
        let mut idl_registry = crate::instruction_parsers::anchor_idl::IdlRegistry::new();
        // The IdlRegistry needs to be populated - use a temporary approach
        // by creating a wrapper that contains both the IDL and an empty registry
        let parser = Box::new(crate::instruction_parsers::anchor_idl::AnchorIdlParser::new(
            *program_id,
            idl_data.clone(), // Clone for the parser
            // Create a registry with just this IDL for event lookup
            {
                let mut registry = crate::instruction_parsers::anchor_idl::IdlRegistry::new();
                // We need to populate the registry with this IDL
                // Since IdlRegistry doesn't have a simple insert method,
                // we'll create a minimal one
                use solana_pubkey::Pubkey;
                use std::collections::HashMap;
                use std::sync::Arc;

                // Create a minimal registry with just this IDL
                let mut inner = crate::instruction_parsers::anchor_idl::IdlRegistryInner {
                    idls: HashMap::new(),
                    types_by_program_and_name: HashMap::new(),
                };
                inner.idls.insert(*program_id, idl_data.clone());

                // Add types if they exist
                if let Some(types) = &idl_data.types {
                    for type_def in types {
                        inner
                            .types_by_program_and_name
                            .insert((*program_id, type_def.name.clone()), type_def.clone());
                    }
                }

                crate::instruction_parsers::anchor_idl::IdlRegistry { inner: Arc::new(inner) }
            },
        ));

        self.parsers.insert(*program_id, parser);
        Ok(true)
    }

    /// Load IDL parsers for all program IDs used in a transaction
    pub fn load_idl_parsers_for_programs(&mut self, program_ids: Vec<Pubkey>) -> Result<usize> {
        let mut loaded_count = 0;
        for program_id in program_ids {
            if self.load_idl_parser_if_needed(&program_id)? {
                loaded_count += 1;
            }
        }
        Ok(loaded_count)
    }

    /// Try to parse an Anchor CPI event, loading IDL if needed
    pub fn parse_cpi_event(
        &mut self,
        instruction: &crate::transaction::InstructionSummary,
        program_id: &Pubkey,
        message: &solana_message::VersionedMessage,
        account_plan: &crate::transaction::MessageAccountPlan,
        lookup_locations: &[crate::transaction::LookupLocation],
    ) -> Option<ParsedInstruction> {
        use crate::instruction_parsers::anchor_idl;

        // Check if this is a CPI event
        if !anchor_idl::is_anchor_cpi_event(instruction) {
            log::debug!("Not a CPI event for program: {}", program_id);
            return None;
        }

        log::debug!("Detected CPI event for program: {}", program_id);

        // Try to load IDL if we haven't already (even though we loaded it earlier, double check)
        if !self.parsers.contains_key(program_id) {
            log::debug!("Loading IDL for CPI event parsing for: {}", program_id);
            if let Err(err) = self.load_idl_parser_if_needed(program_id) {
                log::warn!("Failed to load IDL for CPI event parsing: {:?}", err);
                return None;
            }
        }

        // If we have an Anchor IDL parser, try to parse the CPI event
        if let Some(parser) = self.parsers.get(program_id) {
            // Check if this is an AnchorIdlParser by attempting to downcast
            if let Some(anchor_parser) = parser
                .as_any()
                .downcast_ref::<crate::instruction_parsers::anchor_idl::AnchorIdlParser>(
            ) {
                log::debug!("Found Anchor IDL parser with IDL for CPI event: {}", program_id);
                log::debug!(
                    "IDL has {} events",
                    anchor_parser.idl.events.as_ref().map(|e| e.len()).unwrap_or(0)
                );
                // Use the IDL from the parser to parse the CPI event
                match anchor_idl::parse_anchor_cpi_event(
                    instruction,
                    &anchor_parser.registry, // Use the parser's registry
                    program_id,
                ) {
                    Ok(Some(parsed)) => {
                        log::debug!("Successfully parsed CPI event: {}", parsed.name);
                        return Some(parsed);
                    }
                    Ok(None) => {
                        log::debug!("parse_anchor_cpi_event returned None");
                    }
                    Err(e) => {
                        log::debug!("parse_anchor_cpi_event returned error: {:?}", e);
                    }
                }
            } else {
                log::debug!("Parser exists but couldn't downcast to AnchorIdlParser");
            }
        } else {
            log::debug!("No parser found for program: {}", program_id);
        }

        None
    }
}

/// Load IDL-based parsers from the default IDL directory
pub fn load_idl_parsers() -> Result<IdlRegistry, anyhow::Error> {
    use crate::instruction_parsers::anchor_idl;
    let registry = anchor_idl::load_idls_from_default_dir()?;
    Ok(registry)
}

/// Load IDL-based parsers from a specified directory
pub fn load_idl_parsers_from_path(idl_path: &Path) -> Result<IdlRegistry, anyhow::Error> {
    use crate::instruction_parsers::anchor_idl;
    use anyhow::Context;
    let mut registry = anchor_idl::IdlRegistry::new();

    log::debug!(
        "Looking for IDL directory at: {}",
        idl_path.canonicalize().unwrap_or(idl_path.to_path_buf()).display()
    );

    if idl_path.exists() && idl_path.is_dir() {
        log::info!("Loading IDLs from: {}", idl_path.display());
        registry.load_idls(idl_path).with_context(|| {
            format!("Failed to load IDLs from directory: {}", idl_path.display())
        })?;
    } else {
        return Err(anyhow::anyhow!("IDL directory does not exist: {}", idl_path.display()));
    }

    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_pubkey::Pubkey;

    #[test]
    fn test_token2022_parser_registration() {
        let registry = ParserRegistry::new(None);
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

    #[test]
    fn test_associated_token_parser_registration() {
        let registry = ParserRegistry::new(None);
        let associated_token_id =
            Pubkey::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

        // The Associated Token Program parser should be registered
        assert!(registry.parsers.contains_key(&associated_token_id));
    }

    #[test]
    fn test_associated_token_parser_program_id() {
        let parser = AssociatedTokenProgramParser::new();
        let expected_id = Pubkey::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

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

mod associated_token_program;
pub use associated_token_program::AssociatedTokenProgramParser;

pub mod anchor_idl;
pub use anchor_idl::IdlRegistry;
