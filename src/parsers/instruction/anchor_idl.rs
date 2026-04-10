//! Adapter layer that bridges `sonar_idl` (pure IDL parsing) with the
//! CLI's `InstructionParser` / `ParserRegistry` abstractions.
//!
//! All IDL model types, discriminator computation, and binary parsing live
//! in the `sonar_idl` crate; this module re-exports the public API and
//! provides the `AnchorIdlParser` that implements `InstructionParser`.

use anyhow::Result;
use solana_pubkey::Pubkey;
use sonar_idl::{IdlInstructionFields, IdlParsedInstruction};

use crate::core::transaction::InstructionSummary;
use crate::parsers::instruction::{InstructionParser, ParsedField, ParsedInstruction};

// ── Re-exports from sonar-idl ──

pub use sonar_idl::IndexedIdl;

// ── Adapter: IDL model → CLI model ──

fn to_parsed_instruction(idl_parsed: IdlParsedInstruction) -> ParsedInstruction {
    let fields = match idl_parsed.fields {
        IdlInstructionFields::Parsed(fields) => {
            fields.into_iter().map(|field| ParsedField::json(field.name, field.value)).collect()
        }
        IdlInstructionFields::Unparsed(raw_args_hex) => {
            vec![ParsedField::text("__raw_hex__", raw_args_hex)]
        }
        IdlInstructionFields::Empty => Vec::new(),
    };

    ParsedInstruction { name: idl_parsed.name, fields, account_names: idl_parsed.account_names }
}

// ── AnchorIdlParser ──

/// Parser for Anchor programs using IDL data.
///
/// Implements the `InstructionParser` trait so it can be registered in
/// `ParserRegistry` alongside built-in program parsers.
pub struct AnchorIdlParser {
    program_id: Pubkey,
    pub(crate) idl: IndexedIdl,
}

impl AnchorIdlParser {
    pub fn new(program_id: Pubkey, idl: IndexedIdl) -> Self {
        Self { program_id, idl }
    }
}

impl InstructionParser for AnchorIdlParser {
    fn program_id(&self) -> &Pubkey {
        &self.program_id
    }

    fn parse_instruction(
        &self,
        instruction: &InstructionSummary,
    ) -> Result<Option<ParsedInstruction>> {
        let parsed = self.idl.parse_instruction(&instruction.data)?;
        Ok(parsed.map(to_parsed_instruction))
    }

    fn parse_cpi_event(
        &self,
        instruction: &InstructionSummary,
        _program_id: &Pubkey,
    ) -> Result<Option<ParsedInstruction>> {
        let parsed = self.idl.parse_cpi_event_data(&instruction.data)?;
        Ok(parsed.map(to_parsed_instruction))
    }
}

/// Check if an inner instruction is an Anchor CPI event.
pub fn is_anchor_cpi_event(instruction: &InstructionSummary) -> bool {
    sonar_idl::is_cpi_event_data(&instruction.data)
}
