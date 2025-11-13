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
        let mut registry = Self {
            parsers: HashMap::new(),
        };

        // Register default parsers
        let system_parser = SystemProgramParser::new();
        registry.parsers.insert(*system_parser.program_id(), Box::new(system_parser));

        registry
    }

    /// Registers a new instruction parser
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

/// Parser for the System Program instructions
pub struct SystemProgramParser {
    program_id: Pubkey,
}

impl SystemProgramParser {
    pub fn new() -> Self {
        Self {
            program_id: solana_sdk_ids::system_program::id(),
        }
    }
}

impl Default for SystemProgramParser {
    fn default() -> Self {
        Self::new()
    }
}

impl InstructionParser for SystemProgramParser {
    fn program_id(&self) -> &Pubkey {
        &self.program_id
    }

    fn parse_instruction(
        &self,
        instruction: &InstructionSummary,
    ) -> Result<Option<ParsedInstruction>> {
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

        match instruction_id {
            2 => parse_transfer_instruction(data, instruction),
            0 => parse_create_account_instruction(data, instruction),
            1 => parse_assign_instruction(data, instruction),
            _ => Ok(None), // Unknown system instruction
        }
    }
}

fn parse_transfer_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // System Transfer instruction: 8 bytes lamports (u64)
    if data.len() != 8 {
        return Ok(None);
    }

    let lamports = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);
    let lamports_sol = lamports as f64 / 1_000_000_000.0;

    Ok(Some(ParsedInstruction {
        name: "Transfer".to_string(),
        fields: vec![("lamports".to_string(), format!("{:.9}", lamports_sol))],
        account_names: vec![
            "funding_account".to_string(),
            "recipient_account".to_string(),
        ],
    }))
}

fn parse_create_account_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // System CreateAccount instruction: 8 bytes lamports, 8 bytes space, 32 bytes owner pubkey
    if data.len() != 48 {
        return Ok(None);
    }

    let lamports = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);
    let lamports_sol = lamports as f64 / 1_000_000_000.0;
    let space = u64::from_le_bytes([
        data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
    ]);

    let owner_bytes: [u8; 32] = data[16..48].try_into().unwrap();
    let owner = Pubkey::from(owner_bytes);

    Ok(Some(ParsedInstruction {
        name: "CreateAccount".to_string(),
        fields: vec![
            ("lamports".to_string(), format!("{:.9}", lamports_sol)),
            ("space".to_string(), space.to_string()),
            ("owner".to_string(), owner.to_string()),
        ],
        account_names: vec!["funding_account".to_string(), "new_account".to_string()],
    }))
}

fn parse_assign_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // System Assign instruction: 32 bytes owner pubkey
    if data.len() != 32 {
        return Ok(None);
    }

    let owner_bytes: [u8; 32] = data.try_into().unwrap();
    let owner = Pubkey::from(owner_bytes);

    Ok(Some(ParsedInstruction {
        name: "Assign".to_string(),
        fields: vec![("owner".to_string(), owner.to_string())],
        account_names: vec!["assigned_account".to_string()],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_pubkey::Pubkey;

    #[test]
    fn test_system_transfer_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![
            crate::transaction::AccountReferenceSummary {
                index: 0,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: true,
                writable: true,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            crate::transaction::AccountReferenceSummary {
                index: 1,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: true,
                source: crate::transaction::AccountSourceSummary::Static,
            },
        ];

        // Transfer instruction with 4-byte discriminator (2 = Transfer) + 8 bytes lamports (1,000,000)
        let mut data = vec![2, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(&1_000_000_u64.to_le_bytes()); // 8 bytes lamports
        let instruction = InstructionSummary {
            index: 0,
            program: crate::transaction::AccountReferenceSummary {
                index: 6,
                pubkey: Some(solana_sdk_ids::system_program::id().to_string()),
                signer: false,
                writable: false,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            accounts,
            data: data.into_boxed_slice(),
        };

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "Transfer");
        assert_eq!(parsed.account_names.len(), 2);
        assert_eq!(parsed.account_names[0], "funding_account");
        assert_eq!(parsed.account_names[1], "recipient_account");

        assert!(parsed
            .fields
            .iter()
            .any(|(k, v)| k == "lamports" && v == "0.001000000"));
    }

    #[test]
    fn test_system_create_account_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![
            crate::transaction::AccountReferenceSummary {
                index: 0,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: true,
                writable: true,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            crate::transaction::AccountReferenceSummary {
                index: 1,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: true,
                writable: true,
                source: crate::transaction::AccountSourceSummary::Static,
            },
        ];

        let owner = Pubkey::new_unique();
        // CreateAccount instruction with 4-byte discriminator (0 = CreateAccount)
        let mut data = vec![0, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(&100_000_000_u64.to_le_bytes()); // 8 bytes lamports
        data.extend_from_slice(&256_u64.to_le_bytes()); // 8 bytes space
        data.extend_from_slice(owner.as_ref()); // 32 bytes owner pubkey

        let instruction = InstructionSummary {
            index: 0,
            program: crate::transaction::AccountReferenceSummary {
                index: 6,
                pubkey: Some(solana_sdk_ids::system_program::id().to_string()),
                signer: false,
                writable: false,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            accounts,
            data: data.into_boxed_slice(),
        };

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "CreateAccount");
        assert_eq!(parsed.account_names.len(), 2);
        assert_eq!(parsed.account_names[0], "funding_account");
        assert_eq!(parsed.account_names[1], "new_account");

        assert!(parsed
            .fields
            .iter()
            .any(|(k, v)| k == "lamports" && v == "0.100000000"));
        assert!(parsed
            .fields
            .iter()
            .any(|(k, v)| k == "space" && v == "256"));
        assert!(parsed
            .fields
            .iter()
            .any(|(k, v)| k == "owner" && v == &owner.to_string()));
    }

    #[test]
    fn test_system_assign_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![crate::transaction::AccountReferenceSummary {
            index: 0,
            pubkey: Some(Pubkey::new_unique().to_string()),
            signer: true,
            writable: true,
            source: crate::transaction::AccountSourceSummary::Static,
        }];

        let owner = Pubkey::new_unique();
        // Assign instruction with 4-byte discriminator (1 = Assign)
        let mut data = vec![1, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(owner.as_ref()); // 32 bytes owner pubkey

        let instruction = InstructionSummary {
            index: 0,
            program: crate::transaction::AccountReferenceSummary {
                index: 6,
                pubkey: Some(solana_sdk_ids::system_program::id().to_string()),
                signer: false,
                writable: false,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            accounts,
            data: data.into_boxed_slice(),
        };

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "Assign");
        assert_eq!(parsed.account_names.len(), 1);
        assert_eq!(parsed.account_names[0], "assigned_account");

        assert!(parsed
            .fields
            .iter()
            .any(|(k, v)| k == "owner" && v == &owner.to_string()));
    }
}
