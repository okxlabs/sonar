// Template for adding a new program parser
//
// Instructions for adding a new program parser:
//
// 1. Copy this file to a new file named after your program (e.g., `spl_token.rs`)
// 2. Replace all instances of `TemplateProgramParser` with your parser name (e.g., `SplTokenParser`)
// 3. Set the correct program ID in the `new()` function
// 4. Implement the instruction parsing logic in `parse_instruction()`
// 5. Add the parser to the registry in `mod.rs` (see instructions below)
// 6. Add tests for your parser
//
// To register your parser in `mod.rs`:
//
// a. Add a module declaration at the top (after `mod system_program;`):
//    `mod template;`
//
// b. Add a pub use statement:
//    `pub use template::{TemplateProgramParser};`
//
// c. Register the parser in `ParserRegistry::new()`:
//    ```rust
//    // Example for SPL Token
//    let token_parser = SplTokenParser::new();
//    registry.parsers.insert(*token_parser.program_id(), Box::new(token_parser));
//    ```

use anyhow::Result;
use solana_pubkey::Pubkey;

use crate::transaction::InstructionSummary;
use super::{InstructionParser, ParsedField, ParsedInstruction};

/// Parser for [Your Program Name] instructions
pub struct TemplateProgramParser {
    program_id: Pubkey,
}

impl TemplateProgramParser {
    pub fn new() -> Self {
        // TODO: Replace with your program's public key
        Self { program_id: Pubkey::from_str_const("YourProgramId1111111111111111111111111111111") }
    }
}

impl Default for TemplateProgramParser {
    fn default() -> Self {
        Self::new()
    }
}

impl InstructionParser for TemplateProgramParser {
    fn program_id(&self) -> &Pubkey {
        &self.program_id
    }

    fn parse_instruction(
        &self,
        instruction: &InstructionSummary,
    ) -> Result<Option<ParsedInstruction>> {
        // Most programs use 4-byte instruction discriminators (little-endian)
        if instruction.data.len() < 4 {
            return Ok(None);
        }

        // Read 4-byte instruction discriminator (little-endian)
        let instruction_id = u32::from_le_bytes([
            instruction.data[0],
            instruction.data[1],
            instruction.data[2],
            instruction.data[3],
        ]);
        
        let data = &instruction.data[4..];

        // Parse based on instruction discriminator
        // TODO: Replace with your actual instruction IDs and parsing logic
        match instruction_id {
            0 => parse_example_instruction(data, instruction),
            1 => parse_another_instruction(data, instruction),
            _ => Ok(None), // Unknown instruction
        }
    }
}

// TODO: Implement your actual instruction parsers

fn parse_example_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // Example: instruction with 8 bytes for amount
    if data.len() != 8 {
        return Ok(None);
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);

    Ok(Some(ParsedInstruction {
        name: "ExampleInstruction".to_string(),
        fields: vec![ParsedField::text("amount", amount.to_string())],
        account_names: vec![
            "account1".to_string(),
            "account2".to_string(),
        ],
    }))
}

fn parse_another_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // Example: instruction with no data
    Ok(Some(ParsedInstruction {
        name: "AnotherInstruction".to_string(),
        fields: vec![],
        account_names: vec!["account".to_string()],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_pubkey::Pubkey;
    use crate::transaction::{AccountReferenceSummary, AccountSourceSummary};

    fn create_test_instruction(data: Vec<u8>, accounts: Vec<AccountReferenceSummary>) -> InstructionSummary {
        InstructionSummary {
            index: 0,
            program: AccountReferenceSummary {
                index: 6,
                pubkey: Some(TemplateProgramParser::new().program_id.to_string()),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            accounts,
            data: data.into_boxed_slice(),
        }
    }

    fn create_test_account() -> AccountReferenceSummary {
        AccountReferenceSummary {
            index: 0,
            pubkey: Some(Pubkey::new_unique().to_string()),
            signer: true,
            writable: true,
            source: AccountSourceSummary::Static,
        }
    }

    #[test]
    fn test_example_instruction_parsing() {
        let parser = TemplateProgramParser::new();

        let accounts = vec![
            create_test_account(),
            AccountReferenceSummary {
                index: 1,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: true,
                source: AccountSourceSummary::Static,
            },
        ];

        // Example instruction with 4-byte discriminator (0) + 8 bytes amount
        let mut data = vec![0, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(&1_000_000_u64.to_le_bytes()); // 8 bytes amount

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "ExampleInstruction");
        assert_eq!(parsed.account_names.len(), 2);
        
        assert!(parsed.fields.iter().any(|field| field.name == "amount" && field.value == "1000000"));
    }
}