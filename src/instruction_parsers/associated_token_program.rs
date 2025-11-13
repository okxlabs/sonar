use anyhow::Result;
use solana_pubkey::Pubkey;

use super::{InstructionParser, ParsedInstruction};
use crate::transaction::InstructionSummary;

///Parser for the Associated Token Program instructions
///This program creates and manages associated token accounts for SPL tokens
///Program ID: ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL
pub struct AssociatedTokenProgramParser {
    program_id: Pubkey,
}

impl AssociatedTokenProgramParser {
    pub fn new() -> Self {
        Self { program_id: Pubkey::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL") }
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
        // Associated Token Program instruction discriminators:
        // - Create: No data (empty)
        // - CreateIdempotent: 1 byte [1]
        // - RecoverNested: 1 byte [2]

        if instruction.data.is_empty() {
            // Empty data indicates the Create instruction
            return parse_create(&[], instruction);
        }

        if instruction.data.len() < 1 {
            return Ok(None);
        }

        let instruction_id = instruction.data[0];
        let data = &instruction.data[1..];

        match instruction_id {
            1 => parse_create_idempotent(data, instruction),
            2 => parse_recover_nested(data, instruction),
            _ => Ok(None),
        }
    }
}

fn parse_create(
    _data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // Create Associated Token Account: No instruction data beyond discriminator
    // Accounts: [funding_account, associated_token_account, wallet, mint, system_program, token_program]

    Ok(Some(ParsedInstruction {
        name: "Create".to_string(),
        fields: vec![],
        account_names: vec![
            "funding_account".to_string(),
            "associated_token_account".to_string(),
            "wallet_address".to_string(),
            "token_mint".to_string(),
            "system_program".to_string(),
            "token_program".to_string(),
        ],
    }))
}

fn parse_create_idempotent(
    _data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // Create Associated Token Account Idempotent: No instruction data beyond discriminator
    // Accounts: [funding_account, associated_token_account, wallet, mint, system_program, token_program]

    Ok(Some(ParsedInstruction {
        name: "CreateIdempotent".to_string(),
        fields: vec![],
        account_names: vec![
            "funding_account".to_string(),
            "associated_token_account".to_string(),
            "wallet_address".to_string(),
            "token_mint".to_string(),
            "system_program".to_string(),
            "token_program".to_string(),
        ],
    }))
}

fn parse_recover_nested(
    _data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // Recover Nested: No instruction data beyond discriminator
    // Accounts: [nested_associated_token_account, owner_associated_token_account, owner_wallet_address,
    //            nested_token_mint, token_program_1, token_program_2, wallet_address]

    Ok(Some(ParsedInstruction {
        name: "RecoverNested".to_string(),
        fields: vec![],
        account_names: vec![
            "nested_associated_token_account".to_string(),
            "owner_associated_token_account".to_string(),
            "owner_wallet_address".to_string(),
            "nested_token_mint".to_string(),
            "token_program_1".to_string(),
            "token_program_2".to_string(),
            "wallet_address".to_string(),
        ],
    }))
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
                index: 6,
                pubkey: Some(AssociatedTokenProgramParser::new().program_id.to_string()),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            accounts,
            data: data.into_boxed_slice(),
        }
    }

    fn create_test_account(index: u8, signer: bool, writable: bool) -> AccountReferenceSummary {
        AccountReferenceSummary {
            index: index as usize,
            pubkey: Some(Pubkey::new_unique().to_string()),
            signer,
            writable,
            source: AccountSourceSummary::Static,
        }
    }

    #[test]
    fn test_create_instruction_parsing() {
        let parser = AssociatedTokenProgramParser::new();

        let accounts = vec![
            create_test_account(0, true, true),
            create_test_account(1, false, true),
            create_test_account(2, false, false),
            create_test_account(3, false, false),
            AccountReferenceSummary {
                index: 4,
                pubkey: Some(
                    Pubkey::from_str_const("11111111111111111111111111111111").to_string(),
                ),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            AccountReferenceSummary {
                index: 5,
                pubkey: Some(
                    Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
                        .to_string(),
                ),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
        ];

        // Create instruction has no data (empty)
        let data = vec![]; // Empty data for Create instruction

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "Create");
        assert_eq!(parsed.account_names.len(), 6);
        assert_eq!(parsed.account_names[0], "funding_account");
        assert_eq!(parsed.account_names[1], "associated_token_account");
        assert_eq!(parsed.account_names[2], "wallet_address");
        assert_eq!(parsed.account_names[3], "token_mint");
        assert_eq!(parsed.account_names[4], "system_program");
        assert_eq!(parsed.account_names[5], "token_program");
    }

    #[test]
    fn test_create_idempotent_instruction_parsing() {
        let parser = AssociatedTokenProgramParser::new();

        let accounts = vec![
            create_test_account(0, true, true),
            create_test_account(1, false, true),
            create_test_account(2, false, false),
            create_test_account(3, false, false),
            AccountReferenceSummary {
                index: 4,
                pubkey: Some(
                    Pubkey::from_str_const("11111111111111111111111111111111").to_string(),
                ),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            AccountReferenceSummary {
                index: 5,
                pubkey: Some(
                    Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
                        .to_string(),
                ),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
        ];

        // CreateIdempotent instruction with 1-byte discriminator (1)
        let data = vec![1]; // 1-byte discriminator

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "CreateIdempotent");
        assert_eq!(parsed.account_names.len(), 6);
    }

    #[test]
    fn test_recover_nested_instruction_parsing() {
        let parser = AssociatedTokenProgramParser::new();

        let accounts = vec![
            create_test_account(0, false, true),
            create_test_account(1, false, true),
            create_test_account(2, false, false),
            create_test_account(3, false, false),
            AccountReferenceSummary {
                index: 4,
                pubkey: Some(
                    Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
                        .to_string(),
                ),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            AccountReferenceSummary {
                index: 5,
                pubkey: Some(
                    Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
                        .to_string(),
                ),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            create_test_account(6, false, false),
        ];

        // RecoverNested instruction with 1-byte discriminator (2)
        let data = vec![2]; // 1-byte discriminator

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "RecoverNested");
        assert_eq!(parsed.account_names.len(), 7);
        assert_eq!(parsed.account_names[0], "nested_associated_token_account");
        assert_eq!(parsed.account_names[1], "owner_associated_token_account");
        assert_eq!(parsed.account_names[2], "owner_wallet_address");
        assert_eq!(parsed.account_names[3], "nested_token_mint");
        assert_eq!(parsed.account_names[4], "token_program_1");
        assert_eq!(parsed.account_names[5], "token_program_2");
        assert_eq!(parsed.account_names[6], "wallet_address");
    }

    #[test]
    fn test_unknown_instruction() {
        let parser = AssociatedTokenProgramParser::new();

        let accounts = vec![create_test_account(0, true, true)];

        // Unknown instruction discriminator (99)
        let data = vec![99];

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_create_with_old_discriminator_no_longer_works() {
        let parser = AssociatedTokenProgramParser::new();

        let accounts = vec![
            create_test_account(0, true, true),
            create_test_account(1, false, true),
            create_test_account(2, false, false),
            create_test_account(3, false, false),
        ];

        // Old incorrect format with discriminator 0 should NOT be recognized as Create
        let data = vec![0]; // Old incorrect discriminator

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        // Discriminator 0 is not recognized for any instruction, so should return None
        assert!(result.is_none());
    }
}
