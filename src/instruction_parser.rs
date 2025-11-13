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
        let mut registry = Self { parsers: HashMap::new() };

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
        Self { program_id: solana_sdk_ids::system_program::id() }
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
            0 => parse_create_account_instruction(data, instruction),
            1 => parse_assign_instruction(data, instruction),
            2 => parse_transfer_instruction(data, instruction),
            3 => parse_create_account_with_seed_instruction(data, instruction),
            4 => parse_advance_nonce_account_instruction(data, instruction),
            5 => parse_withdraw_nonce_account_instruction(data, instruction),
            6 => parse_initialize_nonce_account_instruction(data, instruction),
            7 => parse_authorize_nonce_account_instruction(data, instruction),
            8 => parse_allocate_instruction(data, instruction),
            9 => parse_allocate_with_seed_instruction(data, instruction),
            10 => parse_assign_with_seed_instruction(data, instruction),
            11 => parse_transfer_with_seed_instruction(data, instruction),
            12 => parse_upgrade_nonce_account_instruction(data, instruction),
            13 => parse_create_account_allow_prefund_instruction(data, instruction),
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

    Ok(Some(ParsedInstruction {
        name: "Transfer".to_string(),
        fields: vec![("lamports".to_string(), lamports.to_string())],
        account_names: vec!["funding_account".to_string(), "recipient_account".to_string()],
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
    let space = u64::from_le_bytes([
        data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
    ]);

    let owner_bytes: [u8; 32] = data[16..48].try_into().unwrap();
    let owner = Pubkey::from(owner_bytes);

    Ok(Some(ParsedInstruction {
        name: "CreateAccount".to_string(),
        fields: vec![
            ("lamports".to_string(), lamports.to_string()),
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

fn parse_create_account_with_seed_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // CreateAccountWithSeed: 32 bytes base, 8 bytes seed length (bincode), seed bytes (up to 32), 8 bytes lamports, 8 bytes space, 32 bytes owner
    // bincode serializes strings with 8-byte length prefix + bytes
    if data.len() < 89 {
        // Minimum: 32 + 8 + 1 + 8 + 8 + 32
        return Ok(None);
    }

    let base_bytes: [u8; 32] = data[0..32].try_into().unwrap();
    let base = Pubkey::from(base_bytes);

    // Seed length is a u64 (8 bytes) per bincode spec
    let seed_length = u64::from_le_bytes([
        data[32], data[33], data[34], data[35], data[36], data[37], data[38], data[39],
    ]) as usize;

    // Validate seed length (max 32 bytes, cannot exceed data length)
    if seed_length > 32 || data.len() < 40 + seed_length + 8 + 8 + 32 {
        return Ok(None);
    }

    let seed_bytes = &data[40..40 + seed_length];
    let seed = String::from_utf8_lossy(seed_bytes).into_owned();

    let lamports_offset = 40 + seed_length;
    let lamports = u64::from_le_bytes([
        data[lamports_offset],
        data[lamports_offset + 1],
        data[lamports_offset + 2],
        data[lamports_offset + 3],
        data[lamports_offset + 4],
        data[lamports_offset + 5],
        data[lamports_offset + 6],
        data[lamports_offset + 7],
    ]);

    let space_offset = lamports_offset + 8;
    let space = u64::from_le_bytes([
        data[space_offset],
        data[space_offset + 1],
        data[space_offset + 2],
        data[space_offset + 3],
        data[space_offset + 4],
        data[space_offset + 5],
        data[space_offset + 6],
        data[space_offset + 7],
    ]);

    let owner_offset = space_offset + 8;
    let owner_bytes: [u8; 32] = data[owner_offset..owner_offset + 32].try_into().unwrap();
    let owner = Pubkey::from(owner_bytes);

    Ok(Some(ParsedInstruction {
        name: "CreateAccountWithSeed".to_string(),
        fields: vec![
            ("base".to_string(), base.to_string()),
            ("seed".to_string(), seed),
            ("lamports".to_string(), lamports.to_string()),
            ("space".to_string(), space.to_string()),
            ("owner".to_string(), owner.to_string()),
        ],
        account_names: vec![
            "funding_account".to_string(),
            "created_account".to_string(),
            "base_account".to_string(),
        ],
    }))
}

fn parse_advance_nonce_account_instruction(
    _data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // AdvanceNonceAccount has no instruction data
    Ok(Some(ParsedInstruction {
        name: "AdvanceNonceAccount".to_string(),
        fields: vec![],
        account_names: vec![
            "nonce_account".to_string(),
            "recent_blockhashes_sysvar".to_string(),
            "nonce_authority".to_string(),
        ],
    }))
}

fn parse_withdraw_nonce_account_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // WithdrawNonceAccount: 8 bytes lamports
    if data.len() != 8 {
        return Ok(None);
    }

    let lamports = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);

    Ok(Some(ParsedInstruction {
        name: "WithdrawNonceAccount".to_string(),
        fields: vec![("lamports".to_string(), lamports.to_string())],
        account_names: vec![
            "nonce_account".to_string(),
            "recipient_account".to_string(),
            "recent_blockhashes_sysvar".to_string(),
            "rent_sysvar".to_string(),
            "nonce_authority".to_string(),
        ],
    }))
}

fn parse_initialize_nonce_account_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // InitializeNonceAccount: 32 bytes authorized pubkey
    if data.len() != 32 {
        return Ok(None);
    }

    let authorized_bytes: [u8; 32] = data.try_into().unwrap();
    let authorized = Pubkey::from(authorized_bytes);

    Ok(Some(ParsedInstruction {
        name: "InitializeNonceAccount".to_string(),
        fields: vec![("authorized".to_string(), authorized.to_string())],
        account_names: vec![
            "nonce_account".to_string(),
            "recent_blockhashes_sysvar".to_string(),
            "rent_sysvar".to_string(),
        ],
    }))
}

fn parse_authorize_nonce_account_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // AuthorizeNonceAccount: 32 bytes new authorized pubkey
    if data.len() != 32 {
        return Ok(None);
    }

    let authorized_bytes: [u8; 32] = data.try_into().unwrap();
    let authorized = Pubkey::from(authorized_bytes);

    Ok(Some(ParsedInstruction {
        name: "AuthorizeNonceAccount".to_string(),
        fields: vec![("new_authorized".to_string(), authorized.to_string())],
        account_names: vec!["nonce_account".to_string(), "nonce_authority".to_string()],
    }))
}

fn parse_allocate_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // Allocate: 8 bytes space
    if data.len() != 8 {
        return Ok(None);
    }

    let space = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);

    Ok(Some(ParsedInstruction {
        name: "Allocate".to_string(),
        fields: vec![("space".to_string(), space.to_string())],
        account_names: vec!["allocated_account".to_string()],
    }))
}

fn parse_allocate_with_seed_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // AllocateWithSeed: 32 bytes base, 8 bytes seed length (bincode), seed bytes (up to 32), 8 bytes space, 32 bytes owner
    if data.len() < 81 {
        // Minimum: 32 + 8 + 1 + 8 + 32
        return Ok(None);
    }

    let base_bytes: [u8; 32] = data[0..32].try_into().unwrap();
    let base = Pubkey::from(base_bytes);

    // Seed length is a u64 (8 bytes) per bincode spec
    let seed_length = u64::from_le_bytes([
        data[32], data[33], data[34], data[35], data[36], data[37], data[38], data[39],
    ]) as usize;

    // Validate seed length (max 32 bytes, cannot exceed data length)
    if seed_length > 32 || data.len() < 40 + seed_length + 8 + 32 {
        return Ok(None);
    }

    let seed_bytes = &data[40..40 + seed_length];
    let seed = String::from_utf8_lossy(seed_bytes).into_owned();

    let space_offset = 40 + seed_length;
    let space = u64::from_le_bytes([
        data[space_offset],
        data[space_offset + 1],
        data[space_offset + 2],
        data[space_offset + 3],
        data[space_offset + 4],
        data[space_offset + 5],
        data[space_offset + 6],
        data[space_offset + 7],
    ]);

    let owner_offset = space_offset + 8;
    let owner_bytes: [u8; 32] = data[owner_offset..owner_offset + 32].try_into().unwrap();
    let owner = Pubkey::from(owner_bytes);

    Ok(Some(ParsedInstruction {
        name: "AllocateWithSeed".to_string(),
        fields: vec![
            ("base".to_string(), base.to_string()),
            ("seed".to_string(), seed),
            ("space".to_string(), space.to_string()),
            ("owner".to_string(), owner.to_string()),
        ],
        account_names: vec!["allocated_account".to_string(), "base_account".to_string()],
    }))
}

fn parse_assign_with_seed_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // AssignWithSeed: 32 bytes base, 8 bytes seed length (bincode), seed bytes (up to 32), 32 bytes owner
    if data.len() < 73 {
        // Minimum: 32 + 8 + 1 + 32
        return Ok(None);
    }

    let base_bytes: [u8; 32] = data[0..32].try_into().unwrap();
    let base = Pubkey::from(base_bytes);

    // Seed length is a u64 (8 bytes) per bincode spec
    let seed_length = u64::from_le_bytes([
        data[32], data[33], data[34], data[35], data[36], data[37], data[38], data[39],
    ]) as usize;

    // Validate seed length (max 32 bytes, cannot exceed data length)
    if seed_length > 32 || data.len() < 40 + seed_length + 32 {
        return Ok(None);
    }

    let seed_bytes = &data[40..40 + seed_length];
    let seed = String::from_utf8_lossy(seed_bytes).into_owned();

    let owner_offset = 40 + seed_length;
    let owner_bytes: [u8; 32] = data[owner_offset..owner_offset + 32].try_into().unwrap();
    let owner = Pubkey::from(owner_bytes);

    Ok(Some(ParsedInstruction {
        name: "AssignWithSeed".to_string(),
        fields: vec![
            ("base".to_string(), base.to_string()),
            ("seed".to_string(), seed),
            ("owner".to_string(), owner.to_string()),
        ],
        account_names: vec!["assigned_account".to_string(), "base_account".to_string()],
    }))
}

fn parse_transfer_with_seed_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // TransferWithSeed: 8 bytes lamports, 8 bytes seed length (bincode), seed bytes (up to 32), 32 bytes from_owner
    if data.len() < 49 {
        // Minimum: 8 + 8 + 1 + 32
        return Ok(None);
    }

    let lamports = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);

    // Seed length is a u64 (8 bytes) per bincode spec
    let seed_length = u64::from_le_bytes([
        data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
    ]) as usize;

    // Validate seed length (max 32 bytes, cannot exceed data length)
    if seed_length > 32 || data.len() < 16 + seed_length + 32 {
        return Ok(None);
    }

    let seed_bytes = &data[16..16 + seed_length];
    let from_seed = String::from_utf8_lossy(seed_bytes).into_owned();

    let from_owner_offset = 16 + seed_length;
    let from_owner_bytes: [u8; 32] =
        data[from_owner_offset..from_owner_offset + 32].try_into().unwrap();
    let from_owner = Pubkey::from(from_owner_bytes);

    Ok(Some(ParsedInstruction {
        name: "TransferWithSeed".to_string(),
        fields: vec![
            ("lamports".to_string(), lamports.to_string()),
            ("from_seed".to_string(), from_seed),
            ("from_owner".to_string(), from_owner.to_string()),
        ],
        account_names: vec![
            "funding_account".to_string(),
            "base_account".to_string(),
            "recipient_account".to_string(),
        ],
    }))
}

fn parse_upgrade_nonce_account_instruction(
    _data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // UpgradeNonceAccount has no instruction data
    Ok(Some(ParsedInstruction {
        name: "UpgradeNonceAccount".to_string(),
        fields: vec![],
        account_names: vec!["nonce_account".to_string()],
    }))
}

fn parse_create_account_allow_prefund_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // CreateAccountAllowPrefund: Same as CreateAccount - 8 bytes lamports, 8 bytes space, 32 bytes owner
    if data.len() != 48 {
        return Ok(None);
    }

    let lamports = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);
    let space = u64::from_le_bytes([
        data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
    ]);

    let owner_bytes: [u8; 32] = data[16..48].try_into().unwrap();
    let owner = Pubkey::from(owner_bytes);

    Ok(Some(ParsedInstruction {
        name: "CreateAccountAllowPrefund".to_string(),
        fields: vec![
            ("lamports".to_string(), lamports.to_string()),
            ("space".to_string(), space.to_string()),
            ("owner".to_string(), owner.to_string()),
        ],
        account_names: vec!["new_account".to_string(), "(optional) funding_account".to_string()],
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

        assert!(parsed.fields.iter().any(|(k, v)| k == "lamports" && v == "1000000"));
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

        assert!(parsed.fields.iter().any(|(k, v)| k == "lamports" && v == "100000000"));
        assert!(parsed.fields.iter().any(|(k, v)| k == "space" && v == "256"));
        assert!(parsed.fields.iter().any(|(k, v)| k == "owner" && v == &owner.to_string()));
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

        assert!(parsed.fields.iter().any(|(k, v)| k == "owner" && v == &owner.to_string()));
    }

    #[test]
    fn test_system_create_account_with_seed_parsing() {
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
            crate::transaction::AccountReferenceSummary {
                index: 2,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: true,
                writable: false,
                source: crate::transaction::AccountSourceSummary::Static,
            },
        ];

        let base = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let seed = "test_seed";
        // CreateAccountWithSeed instruction with 4-byte discriminator (3 = CreateAccountWithSeed)
        let mut data = vec![3, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(base.as_ref()); // 32 bytes base address
        // bincode encoding: 8 bytes length prefix + seed bytes
        data.extend_from_slice(&(seed.len() as u64).to_le_bytes());
        data.extend_from_slice(seed.as_bytes()); // seed string
        data.extend_from_slice(&50_000_000_u64.to_le_bytes()); // 8 bytes lamports
        data.extend_from_slice(&128_u64.to_le_bytes()); // 8 bytes space
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
        assert_eq!(parsed.name, "CreateAccountWithSeed");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "funding_account");
        assert_eq!(parsed.account_names[1], "created_account");
        assert_eq!(parsed.account_names[2], "base_account");

        assert!(parsed.fields.iter().any(|(k, v)| k == "base" && v == &base.to_string()));
        assert!(parsed.fields.iter().any(|(k, v)| k == "seed" && v == seed));
        assert!(parsed.fields.iter().any(|(k, v)| k == "lamports" && v == "50000000"));
        assert!(parsed.fields.iter().any(|(k, v)| k == "space" && v == "128"));
        assert!(parsed.fields.iter().any(|(k, v)| k == "owner" && v == &owner.to_string()));
    }

    #[test]
    fn test_system_advance_nonce_account_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![
            crate::transaction::AccountReferenceSummary {
                index: 0,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: true,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            crate::transaction::AccountReferenceSummary {
                index: 1,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: false,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            crate::transaction::AccountReferenceSummary {
                index: 2,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: true,
                writable: false,
                source: crate::transaction::AccountSourceSummary::Static,
            },
        ];

        // AdvanceNonceAccount instruction with 4-byte discriminator (4 = AdvanceNonceAccount)
        let data = vec![4, 0, 0, 0]; // 4-byte little-endian discriminator, no instruction data

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
        assert_eq!(parsed.name, "AdvanceNonceAccount");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "nonce_account");
        assert_eq!(parsed.account_names[1], "recent_blockhashes_sysvar");
        assert_eq!(parsed.account_names[2], "nonce_authority");
        assert_eq!(parsed.fields.len(), 0);
    }

    #[test]
    fn test_system_withdraw_nonce_account_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![
            crate::transaction::AccountReferenceSummary {
                index: 0,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
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
            crate::transaction::AccountReferenceSummary {
                index: 2,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: false,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            crate::transaction::AccountReferenceSummary {
                index: 3,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: false,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            crate::transaction::AccountReferenceSummary {
                index: 4,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: true,
                writable: false,
                source: crate::transaction::AccountSourceSummary::Static,
            },
        ];

        // WithdrawNonceAccount instruction with 4-byte discriminator (5 = WithdrawNonceAccount)
        let mut data = vec![5, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(&25_000_000_u64.to_le_bytes()); // 8 bytes lamports

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
        assert_eq!(parsed.name, "WithdrawNonceAccount");
        assert_eq!(parsed.account_names.len(), 5);
        assert_eq!(parsed.account_names[0], "nonce_account");
        assert_eq!(parsed.account_names[1], "recipient_account");
        assert_eq!(parsed.account_names[2], "recent_blockhashes_sysvar");
        assert_eq!(parsed.account_names[3], "rent_sysvar");
        assert_eq!(parsed.account_names[4], "nonce_authority");

        assert!(parsed.fields.iter().any(|(k, v)| k == "lamports" && v == "25000000"));
    }

    #[test]
    fn test_system_initialize_nonce_account_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![
            crate::transaction::AccountReferenceSummary {
                index: 0,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: true,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            crate::transaction::AccountReferenceSummary {
                index: 1,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: false,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            crate::transaction::AccountReferenceSummary {
                index: 2,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: false,
                source: crate::transaction::AccountSourceSummary::Static,
            },
        ];

        let authorized = Pubkey::new_unique();
        // InitializeNonceAccount instruction with 4-byte discriminator (6 = InitializeNonceAccount)
        let mut data = vec![6, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(authorized.as_ref()); // 32 bytes authorized pubkey

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
        assert_eq!(parsed.name, "InitializeNonceAccount");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "nonce_account");
        assert_eq!(parsed.account_names[1], "recent_blockhashes_sysvar");
        assert_eq!(parsed.account_names[2], "rent_sysvar");

        assert!(
            parsed.fields.iter().any(|(k, v)| k == "authorized" && v == &authorized.to_string())
        );
    }

    #[test]
    fn test_system_authorize_nonce_account_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![
            crate::transaction::AccountReferenceSummary {
                index: 0,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: true,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            crate::transaction::AccountReferenceSummary {
                index: 1,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: true,
                writable: false,
                source: crate::transaction::AccountSourceSummary::Static,
            },
        ];

        let new_authorized = Pubkey::new_unique();
        // AuthorizeNonceAccount instruction with 4-byte discriminator (7 = AuthorizeNonceAccount)
        let mut data = vec![7, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(new_authorized.as_ref()); // 32 bytes new authorized pubkey

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
        assert_eq!(parsed.name, "AuthorizeNonceAccount");
        assert_eq!(parsed.account_names.len(), 2);
        assert_eq!(parsed.account_names[0], "nonce_account");
        assert_eq!(parsed.account_names[1], "nonce_authority");

        assert!(
            parsed
                .fields
                .iter()
                .any(|(k, v)| k == "new_authorized" && v == &new_authorized.to_string())
        );
    }

    #[test]
    fn test_system_allocate_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![crate::transaction::AccountReferenceSummary {
            index: 0,
            pubkey: Some(Pubkey::new_unique().to_string()),
            signer: true,
            writable: true,
            source: crate::transaction::AccountSourceSummary::Static,
        }];

        // Allocate instruction with 4-byte discriminator (8 = Allocate)
        let mut data = vec![8, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(&1024_u64.to_le_bytes()); // 8 bytes space

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
        assert_eq!(parsed.name, "Allocate");
        assert_eq!(parsed.account_names.len(), 1);
        assert_eq!(parsed.account_names[0], "allocated_account");

        assert!(parsed.fields.iter().any(|(k, v)| k == "space" && v == "1024"));
    }

    #[test]
    fn test_system_allocate_with_seed_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![
            crate::transaction::AccountReferenceSummary {
                index: 0,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: true,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            crate::transaction::AccountReferenceSummary {
                index: 1,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: true,
                writable: false,
                source: crate::transaction::AccountSourceSummary::Static,
            },
        ];

        let base = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let seed = "allocate_seed";
        // AllocateWithSeed instruction with 4-byte discriminator (9 = AllocateWithSeed)
        let mut data = vec![9, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(base.as_ref()); // 32 bytes base address
        // bincode encoding: 8 bytes length prefix + seed bytes
        data.extend_from_slice(&(seed.len() as u64).to_le_bytes());
        data.extend_from_slice(seed.as_bytes()); // seed string
        data.extend_from_slice(&512_u64.to_le_bytes()); // 8 bytes space
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
        assert_eq!(parsed.name, "AllocateWithSeed");
        assert_eq!(parsed.account_names.len(), 2);
        assert_eq!(parsed.account_names[0], "allocated_account");
        assert_eq!(parsed.account_names[1], "base_account");

        assert!(parsed.fields.iter().any(|(k, v)| k == "base" && v == &base.to_string()));
        assert!(parsed.fields.iter().any(|(k, v)| k == "seed" && v == seed));
        assert!(parsed.fields.iter().any(|(k, v)| k == "space" && v == "512"));
        assert!(parsed.fields.iter().any(|(k, v)| k == "owner" && v == &owner.to_string()));
    }

    #[test]
    fn test_system_assign_with_seed_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![
            crate::transaction::AccountReferenceSummary {
                index: 0,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: true,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            crate::transaction::AccountReferenceSummary {
                index: 1,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: true,
                writable: false,
                source: crate::transaction::AccountSourceSummary::Static,
            },
        ];

        let base = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let seed = "assign_seed";
        // AssignWithSeed instruction with 4-byte discriminator (10 = AssignWithSeed)
        let mut data = vec![10, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(base.as_ref()); // 32 bytes base address
        // bincode encoding: 8 bytes length prefix + seed bytes
        data.extend_from_slice(&(seed.len() as u64).to_le_bytes());
        data.extend_from_slice(seed.as_bytes()); // seed string
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
        assert_eq!(parsed.name, "AssignWithSeed");
        assert_eq!(parsed.account_names.len(), 2);
        assert_eq!(parsed.account_names[0], "assigned_account");
        assert_eq!(parsed.account_names[1], "base_account");

        assert!(parsed.fields.iter().any(|(k, v)| k == "base" && v == &base.to_string()));
        assert!(parsed.fields.iter().any(|(k, v)| k == "seed" && v == seed));
        assert!(parsed.fields.iter().any(|(k, v)| k == "owner" && v == &owner.to_string()));
    }

    #[test]
    fn test_system_transfer_with_seed_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![
            crate::transaction::AccountReferenceSummary {
                index: 0,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: true,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            crate::transaction::AccountReferenceSummary {
                index: 1,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: true,
                writable: false,
                source: crate::transaction::AccountSourceSummary::Static,
            },
            crate::transaction::AccountReferenceSummary {
                index: 2,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: true,
                source: crate::transaction::AccountSourceSummary::Static,
            },
        ];

        let from_owner = Pubkey::new_unique();
        let from_seed = "transfer_seed";
        // TransferWithSeed instruction with 4-byte discriminator (11 = TransferWithSeed)
        let mut data = vec![11, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(&75_000_000_u64.to_le_bytes()); // 8 bytes lamports
        // bincode encoding: 8 bytes length prefix + seed bytes
        data.extend_from_slice(&(from_seed.len() as u64).to_le_bytes());
        data.extend_from_slice(from_seed.as_bytes()); // seed string
        data.extend_from_slice(from_owner.as_ref()); // 32 bytes from_owner pubkey

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
        assert_eq!(parsed.name, "TransferWithSeed");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "funding_account");
        assert_eq!(parsed.account_names[1], "base_account");
        assert_eq!(parsed.account_names[2], "recipient_account");

        assert!(parsed.fields.iter().any(|(k, v)| k == "lamports" && v == "75000000"));
        assert!(parsed.fields.iter().any(|(k, v)| k == "from_seed" && v == from_seed));
        assert!(
            parsed.fields.iter().any(|(k, v)| k == "from_owner" && v == &from_owner.to_string())
        );
    }

    #[test]
    fn test_system_upgrade_nonce_account_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![crate::transaction::AccountReferenceSummary {
            index: 0,
            pubkey: Some(Pubkey::new_unique().to_string()),
            signer: false,
            writable: true,
            source: crate::transaction::AccountSourceSummary::Static,
        }];

        // UpgradeNonceAccount instruction with 4-byte discriminator (12 = UpgradeNonceAccount)
        let data = vec![12, 0, 0, 0]; // 4-byte little-endian discriminator, no instruction data

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
        assert_eq!(parsed.name, "UpgradeNonceAccount");
        assert_eq!(parsed.account_names.len(), 1);
        assert_eq!(parsed.account_names[0], "nonce_account");
        assert_eq!(parsed.fields.len(), 0);
    }

    #[test]
    fn test_system_create_account_allow_prefund_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![crate::transaction::AccountReferenceSummary {
            index: 0,
            pubkey: Some(Pubkey::new_unique().to_string()),
            signer: true,
            writable: true,
            source: crate::transaction::AccountSourceSummary::Static,
        }];

        let owner = Pubkey::new_unique();
        // CreateAccountAllowPrefund instruction with 4-byte discriminator (13 =
        // CreateAccountAllowPrefund)
        let mut data = vec![13, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(&200_000_000_u64.to_le_bytes()); // 8 bytes lamports
        data.extend_from_slice(&768_u64.to_le_bytes()); // 8 bytes space
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
        assert_eq!(parsed.name, "CreateAccountAllowPrefund");
        assert_eq!(parsed.account_names.len(), 2);
        assert_eq!(parsed.account_names[0], "new_account");
        assert_eq!(parsed.account_names[1], "(optional) funding_account");

        assert!(parsed.fields.iter().any(|(k, v)| k == "lamports" && v == "200000000"));
        assert!(parsed.fields.iter().any(|(k, v)| k == "space" && v == "768"));
        assert!(parsed.fields.iter().any(|(k, v)| k == "owner" && v == &owner.to_string()));
    }
}
