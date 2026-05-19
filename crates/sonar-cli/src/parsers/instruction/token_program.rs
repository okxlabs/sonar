use anyhow::Result;
use solana_pubkey::Pubkey;
use sonar_idl::IdlValue;

use super::spl_token_common::{
    self, WITHDRAW_EXCESS_LAMPORTS_DISCRIMINATOR, account_names_with_signers,
    generate_generic_account_names, parse_base_instruction, parsed_instruction,
};
use super::{InstructionParser, ParsedField, ParsedInstruction};
use crate::core::transaction::InstructionSummary;

const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const UNWRAP_LAMPORTS_DISCRIMINATOR: u8 = 45;
const BATCH_DISCRIMINATOR: u8 = 255;

define_parser!(TokenProgramParser, TOKEN_PROGRAM_ID);

impl InstructionParser for TokenProgramParser {
    fn program_id(&self) -> &Pubkey {
        &self.program_id
    }

    fn parse_instruction(
        &self,
        instruction: &InstructionSummary,
    ) -> Result<Option<ParsedInstruction>> {
        if instruction.data.is_empty() {
            return Ok(None);
        }

        let instruction_id = instruction.data[0];
        let data = &instruction.data[1..];

        if let Some(parsed) = parse_base_instruction(instruction_id, data, instruction)? {
            return Ok(Some(parsed));
        }

        match instruction_id {
            WITHDRAW_EXCESS_LAMPORTS_DISCRIMINATOR => {
                spl_token_common::parse_withdraw_excess_lamports_instruction(instruction)
            }
            UNWRAP_LAMPORTS_DISCRIMINATOR => parse_unwrap_lamports_instruction(data, instruction),
            BATCH_DISCRIMINATOR => parse_batch_instruction(data, instruction),
            _ => Ok(None),
        }
    }
}

/// Pinocchio SPL Token extension: UnwrapLamports (discriminator 45).
///
/// Body is a 1-byte amount discriminant followed by either nothing
/// (amount = "all") or 8 bytes (amount = u64). Anything else is rejected.
fn parse_unwrap_lamports_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 3 {
        return Ok(None);
    }

    let amount = match data {
        [0] => IdlValue::String("all".to_string()),
        [1, rest @ ..] if rest.len() == 8 => {
            IdlValue::U64(u64::from_le_bytes(rest.try_into().unwrap()))
        }
        _ => return Ok(None),
    };

    Ok(Some(parsed_instruction(
        "UnwrapLamports",
        vec![ParsedField { name: "amount".into(), value: amount }],
        account_names_with_signers(
            &["source", "destination", "authority"],
            instruction.accounts.len(),
        ),
    )))
}

/// Pinocchio SPL Token extension: Batch (discriminator 255).
///
/// Body is a sequence of `(account_count: u8, data_len: u8, data: [u8; data_len])`
/// tuples. We validate the framing and emit structured sub-instructions so
/// callers don't need to re-parse the raw bytes.
fn parse_batch_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    let mut offset = 0usize;
    let mut sub_instructions: Vec<IdlValue> = Vec::new();
    let mut account_count = 0usize;

    while offset < data.len() {
        if offset + 2 > data.len() {
            return Ok(None);
        }

        let instruction_account_count = data[offset] as usize;
        let instruction_data_len = data[offset + 1] as usize;
        let data_start = offset + 2;
        let data_end = data_start + instruction_data_len;

        if data_end > data.len() {
            return Ok(None);
        }

        sub_instructions.push(IdlValue::Struct(vec![
            (
                "account_count".to_string(),
                IdlValue::U8(instruction_account_count as u8),
            ),
            (
                "data".to_string(),
                IdlValue::String(hex::encode(&data[data_start..data_end])),
            ),
        ]));
        account_count = account_count.saturating_add(instruction_account_count);
        offset = data_end;
    }

    if account_count != instruction.accounts.len() {
        return Ok(None);
    }

    let instruction_count = sub_instructions.len() as u32;

    Ok(Some(parsed_instruction(
        "Batch",
        vec![
            ParsedField {
                name: "instruction_count".into(),
                value: IdlValue::U32(instruction_count),
            },
            ParsedField {
                name: "account_count".into(),
                value: IdlValue::U32(account_count as u32),
            },
            ParsedField {
                name: "instructions".into(),
                value: IdlValue::Array(sub_instructions),
            },
        ],
        generate_generic_account_names(instruction.accounts.len()),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::transaction::{AccountReferenceSummary, AccountSourceSummary};

    fn create_test_instruction(data: Vec<u8>, account_count: usize) -> InstructionSummary {
        InstructionSummary {
            index: 0,
            program: AccountReferenceSummary {
                index: 0,
                pubkey: Some(TOKEN_PROGRAM_ID.to_string()),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            accounts: (0..account_count)
                .map(|index| AccountReferenceSummary {
                    index,
                    pubkey: Some(Pubkey::new_unique().to_string()),
                    signer: index == 0,
                    writable: true,
                    source: AccountSourceSummary::Static,
                })
                .collect(),
            data: data.into_boxed_slice(),
        }
    }

    #[test]
    fn test_program_id() {
        let parser = TokenProgramParser::new();
        let expected_id = Pubkey::from_str_const(TOKEN_PROGRAM_ID);

        assert_eq!(parser.program_id(), &expected_id);
    }

    #[test]
    fn parses_shared_transfer_checked_instruction() {
        let parser = TokenProgramParser::new();
        let mut data = vec![12];
        data.extend_from_slice(&1_000u64.to_le_bytes());
        data.push(6);
        let instruction = create_test_instruction(data, 4);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();

        assert_eq!(parsed.name, "TransferChecked");
        assert_eq!(parsed.account_names, vec!["source", "mint", "destination", "owner"]);
        assert!(parsed.fields.iter().any(|field| field.name == "amount" && field.value == "1000"));
        assert!(parsed.fields.iter().any(|field| field.name == "decimals" && field.value == "6"));
    }

    #[test]
    fn parses_shared_ui_amount_to_amount_instruction() {
        let parser = TokenProgramParser::new();
        let mut data = vec![24];
        data.extend_from_slice(b"12.5");
        let instruction = create_test_instruction(data, 1);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();

        assert_eq!(parsed.name, "UiAmountToAmount");
        assert!(
            parsed.fields.iter().any(|field| field.name == "ui_amount" && field.value == "12.5")
        );
    }

    #[test]
    fn parses_withdraw_excess_lamports_instruction() {
        let parser = TokenProgramParser::new();
        let instruction = create_test_instruction(vec![38], 5);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();

        assert_eq!(parsed.name, "WithdrawExcessLamports");
        assert_eq!(
            parsed.account_names,
            vec![
                "source",
                "destination",
                "authority",
                "additional_signer_1",
                "additional_signer_2"
            ]
        );
    }

    #[test]
    fn parses_unwrap_lamports_all_instruction() {
        let parser = TokenProgramParser::new();
        let instruction = create_test_instruction(vec![45, 0], 3);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();

        assert_eq!(parsed.name, "UnwrapLamports");
        assert!(parsed.fields.iter().any(|field| field.name == "amount" && field.value == "all"));
    }

    #[test]
    fn parses_unwrap_lamports_amount_instruction() {
        let parser = TokenProgramParser::new();
        let mut data = vec![45, 1];
        data.extend_from_slice(&500u64.to_le_bytes());
        let instruction = create_test_instruction(data, 4);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();

        assert_eq!(parsed.name, "UnwrapLamports");
        assert_eq!(
            parsed.account_names,
            vec!["source", "destination", "authority", "additional_signer_1"]
        );
        assert!(parsed.fields.iter().any(|field| field.name == "amount" && field.value == "500"));
    }

    #[test]
    fn rejects_malformed_unwrap_lamports_instruction() {
        let parser = TokenProgramParser::new();
        let instruction = create_test_instruction(vec![45, 1, 1], 3);

        let parsed = parser.parse_instruction(&instruction).unwrap();

        assert!(parsed.is_none());
    }

    #[test]
    fn parses_batch_instruction() {
        let parser = TokenProgramParser::new();
        let instruction = create_test_instruction(vec![255, 1, 1, 17, 2, 1, 5], 3);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();

        assert_eq!(parsed.name, "Batch");
        assert!(
            parsed
                .fields
                .iter()
                .any(|field| field.name == "instruction_count" && field.value == "2")
        );
        assert!(
            parsed.fields.iter().any(|field| field.name == "account_count" && field.value == "3")
        );
        assert!(parsed.fields.iter().any(|field| field.name == "instructions"));
    }

    #[test]
    fn batch_instruction_emits_structured_sub_instructions() {
        let parser = TokenProgramParser::new();
        // Two sub-instructions: (1 account, 1-byte data 0x11), (2 accounts, 1-byte data 0x05)
        let instruction = create_test_instruction(vec![255, 1, 1, 17, 2, 1, 5], 3);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        let instructions_field = parsed
            .fields
            .iter()
            .find(|field| field.name == "instructions")
            .expect("instructions field present");

        let IdlValue::Array(sub_instructions) = &instructions_field.value else {
            panic!("expected Array for instructions field");
        };
        assert_eq!(sub_instructions.len(), 2);

        let IdlValue::Struct(first) = &sub_instructions[0] else {
            panic!("expected struct for sub-instruction 0");
        };
        assert_eq!(first[0].0, "account_count");
        assert_eq!(first[0].1, IdlValue::U8(1));
        assert_eq!(first[1].0, "data");
        assert_eq!(first[1].1, IdlValue::String("11".to_string()));

        let IdlValue::Struct(second) = &sub_instructions[1] else {
            panic!("expected struct for sub-instruction 1");
        };
        assert_eq!(second[0].1, IdlValue::U8(2));
        assert_eq!(second[1].1, IdlValue::String("05".to_string()));
    }

    #[test]
    fn rejects_truncated_batch_instruction() {
        let parser = TokenProgramParser::new();
        let instruction = create_test_instruction(vec![255, 2, 3, 1], 2);

        let parsed = parser.parse_instruction(&instruction).unwrap();

        assert!(parsed.is_none());
    }

    #[test]
    fn rejects_token2022_only_instruction() {
        let parser = TokenProgramParser::new();
        let instruction = create_test_instruction(vec![25, 0], 1);

        let parsed = parser.parse_instruction(&instruction).unwrap();

        assert!(parsed.is_none());
    }
}
