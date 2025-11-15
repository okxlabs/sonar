use anyhow::Result;
use solana_pubkey::Pubkey;

use super::{InstructionParser, ParsedField, ParsedInstruction};
use crate::transaction::InstructionSummary;

/// Parser for Compute Budget program instructions
pub struct ComputeBudgetParser {
    program_id: Pubkey,
}

impl ComputeBudgetParser {
    pub fn new() -> Self {
        // Compute Budget Program ID
        Self { program_id: Pubkey::from_str_const("ComputeBudget111111111111111111111111111111") }
    }
}

impl Default for ComputeBudgetParser {
    fn default() -> Self {
        Self::new()
    }
}

impl InstructionParser for ComputeBudgetParser {
    fn program_id(&self) -> &Pubkey {
        &self.program_id
    }

    fn parse_instruction(
        &self,
        instruction: &InstructionSummary,
    ) -> Result<Option<ParsedInstruction>> {
        // Compute Budget uses 1-byte instruction discriminators
        if instruction.data.is_empty() {
            return Ok(None);
        }

        let instruction_id = instruction.data[0];
        let data = &instruction.data[1..];

        // Parse based on instruction discriminator
        match instruction_id {
            0 => parse_request_units_deprecated(data, instruction),
            1 => parse_request_heap_frame(data, instruction),
            2 => parse_set_compute_unit_limit(data, instruction),
            3 => parse_set_compute_unit_price(data, instruction),
            _ => Ok(None), // Unknown instruction
        }
    }
}

/// Parse RequestUnits instruction (deprecated)
/// Instruction data: units (u32, le) + additional_fee (u32, le)
fn parse_request_units_deprecated(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 8 {
        return Ok(None);
    }

    let units = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as u64;
    let additional_fee = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as u64;

    Ok(Some(ParsedInstruction {
        name: "RequestUnitsDeprecated".to_string(),
        fields: vec![
            ParsedField::text("units", units.to_string()),
            ParsedField::text("additional_fee", additional_fee.to_string()),
        ],
        account_names: vec![],
    }))
}

/// Parse RequestHeapFrame instruction
/// Instruction data: bytes (u32, le)
fn parse_request_heap_frame(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 4 {
        return Ok(None);
    }

    let bytes = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);

    Ok(Some(ParsedInstruction {
        name: "RequestHeapFrame".to_string(),
        fields: vec![ParsedField::text("bytes", bytes.to_string())],
        account_names: vec![],
    }))
}

/// Parse SetComputeUnitLimit instruction
/// Instruction data: units (u32, le)
fn parse_set_compute_unit_limit(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 4 {
        return Ok(None);
    }

    let units = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);

    Ok(Some(ParsedInstruction {
        name: "SetComputeUnitLimit".to_string(),
        fields: vec![ParsedField::text("units", units.to_string())],
        account_names: vec![],
    }))
}

/// Parse SetComputeUnitPrice instruction
/// Instruction data: micro_lamports (u64, le)
fn parse_set_compute_unit_price(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 8 {
        return Ok(None);
    }

    let micro_lamports = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);

    Ok(Some(ParsedInstruction {
        name: "SetComputeUnitPrice".to_string(),
        fields: vec![ParsedField::text("micro_lamports", micro_lamports.to_string())],
        account_names: vec![],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{AccountReferenceSummary, AccountSourceSummary};

    fn create_test_instruction(data: Vec<u8>) -> InstructionSummary {
        InstructionSummary {
            index: 0,
            program: AccountReferenceSummary {
                index: 0,
                pubkey: Some(ComputeBudgetParser::new().program_id.to_string()),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            accounts: vec![],
            data: data.into_boxed_slice(),
        }
    }

    #[test]
    fn test_set_compute_unit_limit() {
        let parser = ComputeBudgetParser::new();

        let mut data = vec![2]; // SetComputeUnitLimit instruction ID
        data.extend_from_slice(&500_000_u32.to_le_bytes()); // 500k units

        let instruction = create_test_instruction(data);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "SetComputeUnitLimit");

        assert!(parsed.fields.iter().any(|field| field.name == "units" && field.value == "500000"));
    }

    #[test]
    fn test_set_compute_unit_price() {
        let parser = ComputeBudgetParser::new();

        let mut data = vec![3]; // SetComputeUnitPrice instruction ID
        data.extend_from_slice(&1_500_000_u64.to_le_bytes()); // 1.5 micro-lamports

        let instruction = create_test_instruction(data);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "SetComputeUnitPrice");

        assert!(
            parsed
                .fields
                .iter()
                .any(|field| field.name == "micro_lamports" && field.value == "1500000")
        );
    }

    #[test]
    fn test_request_heap_frame() {
        let parser = ComputeBudgetParser::new();

        let mut data = vec![1]; // RequestHeapFrame instruction ID
        data.extend_from_slice(&32_768_u32.to_le_bytes()); // 32 KB

        let instruction = create_test_instruction(data);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "RequestHeapFrame");

        assert!(parsed.fields.iter().any(|field| field.name == "bytes" && field.value == "32768"));
    }

    #[test]
    fn test_request_units_deprecated() {
        let parser = ComputeBudgetParser::new();

        let mut data = vec![0]; // RequestUnitsDeprecated instruction ID
        data.extend_from_slice(&300_000_u32.to_le_bytes()); // 300k units
        data.extend_from_slice(&0_u32.to_le_bytes()); // 0 additional fee

        let instruction = create_test_instruction(data);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "RequestUnitsDeprecated");

        assert!(parsed.fields.iter().any(|field| field.name == "units" && field.value == "300000"));
        assert!(
            parsed.fields.iter().any(|field| field.name == "additional_fee" && field.value == "0")
        );
    }

    #[test]
    fn test_unknown_instruction() {
        let parser = ComputeBudgetParser::new();

        let data = vec![99]; // Unknown instruction ID

        let instruction = create_test_instruction(data);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_invalid_data_length() {
        let parser = ComputeBudgetParser::new();

        // SetComputeUnitLimit with wrong data length
        let data = vec![2, 1, 2]; // Only 3 bytes after discriminator

        let instruction = create_test_instruction(data);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_none());
    }
}
