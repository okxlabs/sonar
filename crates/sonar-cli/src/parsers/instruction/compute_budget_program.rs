use anyhow::Result;
use solana_pubkey::Pubkey;

use super::fixed_layout::{self, AccountRule, FieldDef, FieldType, InstructionDef};
use super::{InstructionParser, ParsedInstruction};
use crate::core::transaction::InstructionSummary;

const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const UNUSED_DISCRIMINATOR: u8 = 0;
const REQUEST_HEAP_FRAME_DISCRIMINATOR: u8 = 1;
const SET_COMPUTE_UNIT_LIMIT_DISCRIMINATOR: u8 = 2;
const SET_COMPUTE_UNIT_PRICE_DISCRIMINATOR: u8 = 3;
const SET_LOADED_ACCOUNTS_DATA_SIZE_LIMIT_DISCRIMINATOR: u8 = 4;

define_parser!(ComputeBudgetProgramParser, COMPUTE_BUDGET_PROGRAM_ID);

/// Every Compute Budget instruction is a single-byte discriminator followed by
/// a fixed scalar (or nothing), with no accounts — a pure fixed-layout table.
static COMPUTE_BUDGET: &[InstructionDef] = &[
    InstructionDef {
        discriminator: &[UNUSED_DISCRIMINATOR],
        name: "Unused",
        fields: &[],
        account_names: &[],
        account_rule: AccountRule::Verbatim,
    },
    InstructionDef {
        discriminator: &[REQUEST_HEAP_FRAME_DISCRIMINATOR],
        name: "RequestHeapFrame",
        fields: &[FieldDef { name: "bytes", ty: FieldType::U32 }],
        account_names: &[],
        account_rule: AccountRule::Verbatim,
    },
    InstructionDef {
        discriminator: &[SET_COMPUTE_UNIT_LIMIT_DISCRIMINATOR],
        name: "SetComputeUnitLimit",
        fields: &[FieldDef { name: "units", ty: FieldType::U32 }],
        account_names: &[],
        account_rule: AccountRule::Verbatim,
    },
    InstructionDef {
        discriminator: &[SET_COMPUTE_UNIT_PRICE_DISCRIMINATOR],
        name: "SetComputeUnitPrice",
        fields: &[FieldDef { name: "micro_lamports", ty: FieldType::U64 }],
        account_names: &[],
        account_rule: AccountRule::Verbatim,
    },
    InstructionDef {
        discriminator: &[SET_LOADED_ACCOUNTS_DATA_SIZE_LIMIT_DISCRIMINATOR],
        name: "SetLoadedAccountsDataSizeLimit",
        fields: &[FieldDef { name: "bytes", ty: FieldType::U32 }],
        account_names: &[],
        account_rule: AccountRule::Verbatim,
    },
];

impl InstructionParser for ComputeBudgetProgramParser {
    fn program_id(&self) -> &Pubkey {
        &self.program_id
    }

    fn parse_instruction(
        &self,
        instruction: &InstructionSummary,
    ) -> Result<Option<ParsedInstruction>> {
        fixed_layout::parse(COMPUTE_BUDGET, instruction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::transaction::{AccountReferenceSummary, AccountSourceSummary};

    fn create_test_instruction(data: Vec<u8>) -> InstructionSummary {
        InstructionSummary {
            index: 0,
            program: AccountReferenceSummary {
                index: 0,
                pubkey: Some(COMPUTE_BUDGET_PROGRAM_ID.to_string()),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            accounts: vec![],
            data: data.into_boxed_slice(),
        }
    }

    #[test]
    fn test_program_id() {
        let parser = ComputeBudgetProgramParser::new();
        let expected_id = Pubkey::from_str_const(COMPUTE_BUDGET_PROGRAM_ID);
        assert_eq!(parser.program_id(), &expected_id);
    }

    #[test]
    fn test_unused_instruction_parsing() {
        let parser = ComputeBudgetProgramParser::new();
        let instruction = create_test_instruction(vec![UNUSED_DISCRIMINATOR]);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "Unused");
        assert!(parsed.fields.is_empty());
        assert!(parsed.account_names.is_empty());
    }

    #[test]
    fn test_request_heap_frame_instruction_parsing() {
        let parser = ComputeBudgetProgramParser::new();
        let mut data = vec![REQUEST_HEAP_FRAME_DISCRIMINATOR];
        data.extend_from_slice(&64_000_u32.to_le_bytes());
        let instruction = create_test_instruction(data);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "RequestHeapFrame");
        assert!(parsed.fields.iter().any(|field| field.name == "bytes" && field.value == "64000"));
    }

    #[test]
    fn test_set_compute_unit_limit_instruction_parsing() {
        let parser = ComputeBudgetProgramParser::new();
        let mut data = vec![SET_COMPUTE_UNIT_LIMIT_DISCRIMINATOR];
        data.extend_from_slice(&300_000_u32.to_le_bytes());
        let instruction = create_test_instruction(data);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "SetComputeUnitLimit");
        assert!(parsed.fields.iter().any(|field| field.name == "units" && field.value == "300000"));
    }

    #[test]
    fn test_set_compute_unit_price_instruction_parsing() {
        let parser = ComputeBudgetProgramParser::new();
        let mut data = vec![SET_COMPUTE_UNIT_PRICE_DISCRIMINATOR];
        data.extend_from_slice(&5_000_u64.to_le_bytes());
        let instruction = create_test_instruction(data);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "SetComputeUnitPrice");
        assert!(
            parsed
                .fields
                .iter()
                .any(|field| field.name == "micro_lamports" && field.value == "5000")
        );
    }

    #[test]
    fn test_set_loaded_accounts_data_size_limit_instruction_parsing() {
        let parser = ComputeBudgetProgramParser::new();
        let mut data = vec![SET_LOADED_ACCOUNTS_DATA_SIZE_LIMIT_DISCRIMINATOR];
        data.extend_from_slice(&131_072_u32.to_le_bytes());
        let instruction = create_test_instruction(data);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "SetLoadedAccountsDataSizeLimit");
        assert!(parsed.fields.iter().any(|field| field.name == "bytes" && field.value == "131072"));
    }

    #[test]
    fn test_invalid_data_length_returns_none() {
        let parser = ComputeBudgetProgramParser::new();
        let instruction = create_test_instruction(vec![SET_COMPUTE_UNIT_LIMIT_DISCRIMINATOR, 1, 2]);

        let parsed = parser.parse_instruction(&instruction).unwrap();
        assert!(parsed.is_none());
    }

    #[test]
    fn test_unknown_instruction_returns_none() {
        let parser = ComputeBudgetProgramParser::new();
        let instruction = create_test_instruction(vec![99, 1, 2, 3, 4]);

        let parsed = parser.parse_instruction(&instruction).unwrap();
        assert!(parsed.is_none());
    }
}
