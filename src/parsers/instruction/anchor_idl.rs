//! Adapter layer that bridges `sonar_idl` (pure IDL parsing) with the
//! CLI's `InstructionParser` / `ParserRegistry` abstractions.
//!
//! All IDL model types, discriminator computation, and binary parsing live
//! in the `sonar_idl` crate; this module re-exports the public API and
//! provides the `AnchorIdlParser` that implements `InstructionParser`.

use anyhow::Result;
use serde_json::Value;
use solana_pubkey::Pubkey;
use sonar_idl::{IdlInstructionFields, IdlParsedInstruction, IdlValue};

use crate::core::transaction::InstructionSummary;
use crate::parsers::instruction::{
    InstructionParser, ParsedField, ParsedInstruction, ParsedInstructionFields,
};

// ── Re-exports from sonar-idl ──

pub use sonar_idl::IndexedIdl;

// ── Adapter: IDL model → CLI model ──

fn to_parsed_instruction(idl_parsed: IdlParsedInstruction) -> ParsedInstruction {
    let fields = match idl_parsed.fields {
        IdlInstructionFields::Parsed(fields) => fields
            .into_iter()
            .map(|field| ParsedField::json(field.name, idl_value_to_json(field.value)))
            .collect::<Vec<_>>()
            .into(),
        IdlInstructionFields::Unparsed(raw_args_hex) => {
            ParsedInstructionFields::RawHex(raw_args_hex)
        }
    };

    ParsedInstruction { name: idl_parsed.name, fields, account_names: idl_parsed.account_names }
}

/// Convert an `IdlValue` to `serde_json::Value` for the CLI output layer.
///
/// - `Uint`/`Int` emit as JSON numbers when the value fits in u64/i64,
///   falling back to a string for values that exceed JSON's range.
/// - `Bytes` emits as a JSON array of numbers.
/// - `Struct` emits as a JSON object preserving field order.
pub(crate) fn idl_value_to_json(value: IdlValue) -> Value {
    match value {
        IdlValue::Uint(n) => match u64::try_from(n) {
            Ok(v) => Value::Number(v.into()),
            Err(_) => Value::String(n.to_string()),
        },
        IdlValue::Int(n) => match i64::try_from(n) {
            Ok(v) => Value::Number(v.into()),
            Err(_) => Value::String(n.to_string()),
        },
        IdlValue::Bool(b) => Value::Bool(b),
        IdlValue::String(s) => Value::String(s),
        IdlValue::Bytes(bytes) => {
            Value::Array(bytes.into_iter().map(|b| Value::Number((b as u64).into())).collect())
        }
        IdlValue::Struct(fields) => {
            let map: serde_json::Map<String, Value> =
                fields.into_iter().map(|(k, v)| (k, idl_value_to_json(v))).collect();
            Value::Object(map)
        }
        IdlValue::Array(values) => {
            Value::Array(values.into_iter().map(idl_value_to_json).collect())
        }
        IdlValue::Null => Value::Null,
    }
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
