use anyhow::Result;
use solana_pubkey::Pubkey;

use super::{InstructionParser, ParsedField, ParsedInstruction};
use crate::core::transaction::InstructionSummary;

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
        fields: vec![ParsedField::text("lamports", lamports.to_string())],
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
            ParsedField::text("lamports", lamports.to_string()),
            ParsedField::text("space", space.to_string()),
            ParsedField::text("owner", owner.to_string()),
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
        fields: vec![ParsedField::text("owner", owner.to_string())],
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
            ParsedField::text("base", base.to_string()),
            ParsedField::text("seed", seed),
            ParsedField::text("lamports", lamports.to_string()),
            ParsedField::text("space", space.to_string()),
            ParsedField::text("owner", owner.to_string()),
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
        fields: vec![ParsedField::text("lamports", lamports.to_string())],
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
        fields: vec![ParsedField::text("authorized", authorized.to_string())],
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
        fields: vec![ParsedField::text("new_authorized", authorized.to_string())],
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
        fields: vec![ParsedField::text("space", space.to_string())],
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
            ParsedField::text("base", base.to_string()),
            ParsedField::text("seed", seed),
            ParsedField::text("space", space.to_string()),
            ParsedField::text("owner", owner.to_string()),
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
            ParsedField::text("base", base.to_string()),
            ParsedField::text("seed", seed),
            ParsedField::text("owner", owner.to_string()),
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
            ParsedField::text("lamports", lamports.to_string()),
            ParsedField::text("from_seed", from_seed),
            ParsedField::text("from_owner", from_owner.to_string()),
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
            ParsedField::text("lamports", lamports.to_string()),
            ParsedField::text("space", space.to_string()),
            ParsedField::text("owner", owner.to_string()),
        ],
        account_names: vec!["new_account".to_string(), "(optional) funding_account".to_string()],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::transaction::{AccountReferenceSummary, AccountSourceSummary, InstructionSummary};
    use solana_pubkey::Pubkey;

    fn create_test_instruction(
        data: Vec<u8>,
        accounts: Vec<AccountReferenceSummary>,
    ) -> InstructionSummary {
        InstructionSummary {
            index: 0,
            program: AccountReferenceSummary {
                index: 6,
                pubkey: Some(solana_sdk_ids::system_program::id().to_string()),
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
    fn test_system_transfer_parsing() {
        let parser = SystemProgramParser::new();

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

        // Transfer instruction with 4-byte discriminator (2 = Transfer) + 8 bytes lamports (1,000,000)
        let mut data = vec![2, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(&1_000_000_u64.to_le_bytes()); // 8 bytes lamports
        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "Transfer");
        assert_eq!(parsed.account_names.len(), 2);
        assert_eq!(parsed.account_names[0], "funding_account");
        assert_eq!(parsed.account_names[1], "recipient_account");

        assert!(
            parsed.fields.iter().any(|field| field.name == "lamports" && field.value == "1000000")
        );
    }

    #[test]
    fn test_system_create_account_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![
            create_test_account(),
            AccountReferenceSummary {
                index: 1,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: true,
                writable: true,
                source: AccountSourceSummary::Static,
            },
        ];

        let owner = Pubkey::new_unique();
        // CreateAccount instruction with 4-byte discriminator (0 = CreateAccount)
        let mut data = vec![0, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(&100_000_000_u64.to_le_bytes()); // 8 bytes lamports
        data.extend_from_slice(&256_u64.to_le_bytes()); // 8 bytes space
        data.extend_from_slice(owner.as_ref()); // 32 bytes owner pubkey

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "CreateAccount");
        assert_eq!(parsed.account_names.len(), 2);
        assert_eq!(parsed.account_names[0], "funding_account");
        assert_eq!(parsed.account_names[1], "new_account");

        assert!(
            parsed
                .fields
                .iter()
                .any(|field| field.name == "lamports" && field.value == "100000000")
        );
        assert!(parsed.fields.iter().any(|field| field.name == "space" && field.value == "256"));
        assert!(
            parsed
                .fields
                .iter()
                .any(|field| field.name == "owner" && field.value == owner.to_string())
        );
    }

    #[test]
    fn test_system_assign_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![create_test_account()];

        let owner = Pubkey::new_unique();
        // Assign instruction with 4-byte discriminator (1 = Assign)
        let mut data = vec![1, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(owner.as_ref()); // 32 bytes owner pubkey

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "Assign");
        assert_eq!(parsed.account_names.len(), 1);
        assert_eq!(parsed.account_names[0], "assigned_account");

        assert!(
            parsed
                .fields
                .iter()
                .any(|field| field.name == "owner" && field.value == owner.to_string())
        );
    }

    #[test]
    fn test_system_create_account_with_seed_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![
            create_test_account(),
            AccountReferenceSummary {
                index: 1,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: true,
                source: AccountSourceSummary::Static,
            },
            AccountReferenceSummary {
                index: 2,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: true,
                writable: false,
                source: AccountSourceSummary::Static,
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

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "CreateAccountWithSeed");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "funding_account");
        assert_eq!(parsed.account_names[1], "created_account");
        assert_eq!(parsed.account_names[2], "base_account");

        assert!(
            parsed
                .fields
                .iter()
                .any(|field| field.name == "base" && field.value == base.to_string())
        );
        assert!(parsed.fields.iter().any(|field| field.name == "seed" && field.value == seed));
        assert!(
            parsed.fields.iter().any(|field| field.name == "lamports" && field.value == "50000000")
        );
        assert!(parsed.fields.iter().any(|field| field.name == "space" && field.value == "128"));
        assert!(
            parsed
                .fields
                .iter()
                .any(|field| field.name == "owner" && field.value == owner.to_string())
        );
    }

    #[test]
    fn test_system_advance_nonce_account_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![
            AccountReferenceSummary {
                index: 0,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: true,
                source: AccountSourceSummary::Static,
            },
            AccountReferenceSummary {
                index: 1,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            create_test_account(),
        ];

        // AdvanceNonceAccount instruction with 4-byte discriminator (4 = AdvanceNonceAccount)
        let data = vec![4, 0, 0, 0]; // 4-byte little-endian discriminator, no instruction data

        let instruction = create_test_instruction(data, accounts);

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
            AccountReferenceSummary {
                index: 0,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: true,
                source: AccountSourceSummary::Static,
            },
            AccountReferenceSummary {
                index: 1,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: true,
                source: AccountSourceSummary::Static,
            },
            AccountReferenceSummary {
                index: 2,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            AccountReferenceSummary {
                index: 3,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            create_test_account(),
        ];

        // WithdrawNonceAccount instruction with 4-byte discriminator (5 = WithdrawNonceAccount)
        let mut data = vec![5, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(&25_000_000_u64.to_le_bytes()); // 8 bytes lamports

        let instruction = create_test_instruction(data, accounts);

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

        assert!(
            parsed.fields.iter().any(|field| field.name == "lamports" && field.value == "25000000")
        );
    }

    #[test]
    fn test_system_initialize_nonce_account_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![
            AccountReferenceSummary {
                index: 0,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: true,
                source: AccountSourceSummary::Static,
            },
            AccountReferenceSummary {
                index: 1,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            AccountReferenceSummary {
                index: 2,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
        ];

        let authorized = Pubkey::new_unique();
        // InitializeNonceAccount instruction with 4-byte discriminator (6 = InitializeNonceAccount)
        let mut data = vec![6, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(authorized.as_ref()); // 32 bytes authorized pubkey

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "InitializeNonceAccount");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "nonce_account");
        assert_eq!(parsed.account_names[1], "recent_blockhashes_sysvar");
        assert_eq!(parsed.account_names[2], "rent_sysvar");

        assert!(
            parsed
                .fields
                .iter()
                .any(|field| field.name == "authorized" && field.value == authorized.to_string())
        );
    }

    #[test]
    fn test_system_authorize_nonce_account_parsing() {
        let parser = SystemProgramParser::new();

        let accounts = vec![
            AccountReferenceSummary {
                index: 0,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: false,
                writable: true,
                source: AccountSourceSummary::Static,
            },
            create_test_account(),
        ];

        let authorized = Pubkey::new_unique();
        // AuthorizeNonceAccount instruction with 4-byte discriminator (7 = AuthorizeNonceAccount)
        let mut data = vec![7, 0, 0, 0]; // 4-byte little-endian discriminator
        data.extend_from_slice(authorized.as_ref()); // 32 bytes authorized pubkey

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "AuthorizeNonceAccount");
        assert_eq!(parsed.account_names.len(), 2);
        assert_eq!(parsed.account_names[0], "nonce_account");
        assert_eq!(parsed.account_names[1], "nonce_authority");

        assert!(
            parsed.fields.iter().any(
                |field| field.name == "new_authorized" && field.value == authorized.to_string()
            )
        );
    }
}
