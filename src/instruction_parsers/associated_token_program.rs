use anyhow::Result;
use solana_pubkey::Pubkey;

use super::{InstructionParser, ParsedInstruction};
use crate::transaction::InstructionSummary;

const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
const CREATE_DISCRIMINATOR: u8 = 0;
const CREATE_IDEMPOTENT_DISCRIMINATOR: u8 = 1;
const RECOVER_NESTED_DISCRIMINATOR: u8 = 2;

pub struct AssociatedTokenProgramParser {
    program_id: Pubkey,
}

impl AssociatedTokenProgramParser {
    pub fn new() -> Self {
        Self { program_id: Pubkey::from_str_const(ASSOCIATED_TOKEN_PROGRAM_ID) }
    }
}

impl Default for AssociatedTokenProgramParser {
    fn default() -> Self {
        Self::new()
    }
}

impl InstructionParser for AssociatedTokenProgramParser {
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
        if !data.is_empty() {
            return Ok(None);
        }

        match instruction_id {
            CREATE_DISCRIMINATOR => parse_create_instruction("Create", instruction),
            CREATE_IDEMPOTENT_DISCRIMINATOR => {
                parse_create_instruction("CreateIdempotent", instruction)
            }
            RECOVER_NESTED_DISCRIMINATOR => parse_recover_nested_instruction(instruction),
            _ => Ok(None),
        }
    }
}

fn parse_create_instruction(
    instruction_name: &str,
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 6 {
        return Ok(None);
    }

    let mut account_names = vec![
        "funding_account".to_string(),
        "associated_token_account".to_string(),
        "wallet".to_string(),
        "mint".to_string(),
        "system_program".to_string(),
        "token_program".to_string(),
    ];
    append_extra_account_names(&mut account_names, instruction.accounts.len(), 6);

    Ok(Some(ParsedInstruction {
        name: instruction_name.to_string(),
        fields: vec![],
        account_names,
    }))
}

fn parse_recover_nested_instruction(
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 7 {
        return Ok(None);
    }

    let mut account_names = vec![
        "nested_associated_token_account".to_string(),
        "nested_token_mint".to_string(),
        "destination_associated_token_account".to_string(),
        "owner_associated_token_account".to_string(),
        "owner_token_mint".to_string(),
        "wallet".to_string(),
        "token_program".to_string(),
    ];
    append_extra_account_names(&mut account_names, instruction.accounts.len(), 7);

    Ok(Some(ParsedInstruction { name: "RecoverNested".to_string(), fields: vec![], account_names }))
}

fn append_extra_account_names(
    account_names: &mut Vec<String>,
    total_accounts: usize,
    accounted_accounts: usize,
) {
    if total_accounts <= accounted_accounts {
        return;
    }

    account_names.extend(
        (0..(total_accounts - accounted_accounts)).map(|i| format!("additional_account_{}", i + 1)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{AccountReferenceSummary, AccountSourceSummary};

    fn create_test_instruction(
        data: Vec<u8>,
        accounts: Vec<AccountReferenceSummary>,
    ) -> InstructionSummary {
        InstructionSummary {
            index: 0,
            program: AccountReferenceSummary {
                index: 0,
                pubkey: Some(ASSOCIATED_TOKEN_PROGRAM_ID.to_string()),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            accounts,
            data: data.into_boxed_slice(),
        }
    }

    fn create_test_accounts(count: usize) -> Vec<AccountReferenceSummary> {
        (0..count)
            .map(|index| AccountReferenceSummary {
                index,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: index == 0,
                writable: true,
                source: AccountSourceSummary::Static,
            })
            .collect()
    }

    #[test]
    fn test_program_id() {
        let parser = AssociatedTokenProgramParser::new();
        let expected_id = Pubkey::from_str_const(ASSOCIATED_TOKEN_PROGRAM_ID);
        assert_eq!(parser.program_id(), &expected_id);
    }

    #[test]
    fn test_create_instruction_parsing() {
        let parser = AssociatedTokenProgramParser::new();
        let instruction =
            create_test_instruction(vec![CREATE_DISCRIMINATOR], create_test_accounts(6));

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "Create");
        assert_eq!(
            parsed.account_names,
            vec![
                "funding_account",
                "associated_token_account",
                "wallet",
                "mint",
                "system_program",
                "token_program"
            ]
        );
    }

    #[test]
    fn test_create_idempotent_instruction_parsing() {
        let parser = AssociatedTokenProgramParser::new();
        let instruction =
            create_test_instruction(vec![CREATE_IDEMPOTENT_DISCRIMINATOR], create_test_accounts(6));

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "CreateIdempotent");
        assert_eq!(parsed.account_names.len(), 6);
    }

    #[test]
    fn test_recover_nested_instruction_parsing() {
        let parser = AssociatedTokenProgramParser::new();
        let instruction =
            create_test_instruction(vec![RECOVER_NESTED_DISCRIMINATOR], create_test_accounts(7));

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "RecoverNested");
        assert_eq!(parsed.account_names.len(), 7);
        assert_eq!(parsed.account_names[0], "nested_associated_token_account");
        assert_eq!(parsed.account_names[6], "token_program");
    }

    #[test]
    fn test_create_instruction_insufficient_accounts_returns_none() {
        let parser = AssociatedTokenProgramParser::new();
        let instruction =
            create_test_instruction(vec![CREATE_DISCRIMINATOR], create_test_accounts(5));

        let parsed = parser.parse_instruction(&instruction).unwrap();
        assert!(parsed.is_none());
    }

    #[test]
    fn test_recover_nested_instruction_insufficient_accounts_returns_none() {
        let parser = AssociatedTokenProgramParser::new();
        let instruction =
            create_test_instruction(vec![RECOVER_NESTED_DISCRIMINATOR], create_test_accounts(6));

        let parsed = parser.parse_instruction(&instruction).unwrap();
        assert!(parsed.is_none());
    }

    #[test]
    fn test_invalid_payload_returns_none() {
        let parser = AssociatedTokenProgramParser::new();
        let instruction =
            create_test_instruction(vec![CREATE_DISCRIMINATOR, 1], create_test_accounts(6));

        let parsed = parser.parse_instruction(&instruction).unwrap();
        assert!(parsed.is_none());
    }

    #[test]
    fn test_unknown_instruction_returns_none() {
        let parser = AssociatedTokenProgramParser::new();
        let instruction = create_test_instruction(vec![99], create_test_accounts(7));

        let parsed = parser.parse_instruction(&instruction).unwrap();
        assert!(parsed.is_none());
    }
}
