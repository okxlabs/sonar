use anyhow::{Context, Result};
use solana_pubkey::Pubkey;
use sonar_idl::IdlValue;

use super::fixed_layout::{self, AccountRule, FieldDef, FieldType, InstructionDef};
use super::{InstructionParser, ParsedField, ParsedInstruction};
use crate::core::transaction::InstructionSummary;
use crate::parsers::binary_reader::{self, BinaryReader};

define_parser!(SystemProgramParser, "11111111111111111111111111111111");

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

        // Fixed-layout instructions (primitive fields, no seeds) are described
        // declaratively in SYSTEM_FIXED; the rest keep bespoke parsers because
        // their data is variable-length (seeds) or absent.
        if let Some(parsed) = fixed_layout::parse(SYSTEM_FIXED, instruction)? {
            return Ok(Some(parsed));
        }

        match instruction_id {
            3 => parse_create_account_with_seed_instruction(data, instruction),
            4 => parse_advance_nonce_account_instruction(data, instruction),
            9 => parse_allocate_with_seed_instruction(data, instruction),
            10 => parse_assign_with_seed_instruction(data, instruction),
            11 => parse_transfer_with_seed_instruction(data, instruction),
            12 => parse_upgrade_nonce_account_instruction(data, instruction),
            _ => Ok(None), // Unknown system instruction
        }
    }
}

/// Fixed-layout System instructions (primitive-field data, fixed account
/// lists). Variable-length instructions — those with bincode seeds (3, 9, 10,
/// 11), the nonce advance/upgrade ops (4, 12) — stay in the bespoke match.
///
/// Discriminators are 4-byte little-endian, so e.g. instruction 13 is
/// `&[13, 0, 0, 0]`. System instructions don't validate account count, hence
/// [`AccountRule::Verbatim`].
static SYSTEM_FIXED: &[InstructionDef] = &[
    InstructionDef {
        discriminator: &[0, 0, 0, 0],
        name: "CreateAccount",
        fields: &[
            FieldDef { name: "lamports", ty: FieldType::U64 },
            FieldDef { name: "space", ty: FieldType::U64 },
            FieldDef { name: "owner", ty: FieldType::Pubkey },
        ],
        account_names: &["funding_account", "new_account"],
        account_rule: AccountRule::Verbatim,
    },
    InstructionDef {
        discriminator: &[1, 0, 0, 0],
        name: "Assign",
        fields: &[FieldDef { name: "owner", ty: FieldType::Pubkey }],
        account_names: &["assigned_account"],
        account_rule: AccountRule::Verbatim,
    },
    InstructionDef {
        discriminator: &[2, 0, 0, 0],
        name: "Transfer",
        fields: &[FieldDef { name: "lamports", ty: FieldType::U64 }],
        account_names: &["funding_account", "recipient_account"],
        account_rule: AccountRule::Verbatim,
    },
    InstructionDef {
        discriminator: &[5, 0, 0, 0],
        name: "WithdrawNonceAccount",
        fields: &[FieldDef { name: "lamports", ty: FieldType::U64 }],
        account_names: &[
            "nonce_account",
            "recipient_account",
            "recent_blockhashes_sysvar",
            "rent_sysvar",
            "nonce_authority",
        ],
        account_rule: AccountRule::Verbatim,
    },
    InstructionDef {
        discriminator: &[6, 0, 0, 0],
        name: "InitializeNonceAccount",
        fields: &[FieldDef { name: "authorized", ty: FieldType::Pubkey }],
        account_names: &["nonce_account", "recent_blockhashes_sysvar", "rent_sysvar"],
        account_rule: AccountRule::Verbatim,
    },
    InstructionDef {
        discriminator: &[7, 0, 0, 0],
        name: "AuthorizeNonceAccount",
        fields: &[FieldDef { name: "new_authorized", ty: FieldType::Pubkey }],
        account_names: &["nonce_account", "nonce_authority"],
        account_rule: AccountRule::Verbatim,
    },
    InstructionDef {
        discriminator: &[8, 0, 0, 0],
        name: "Allocate",
        fields: &[FieldDef { name: "space", ty: FieldType::U64 }],
        account_names: &["allocated_account"],
        account_rule: AccountRule::Verbatim,
    },
    InstructionDef {
        discriminator: &[13, 0, 0, 0],
        name: "CreateAccountAllowPrefund",
        // Same layout as CreateAccount; differs only in account roles.
        fields: &[
            FieldDef { name: "lamports", ty: FieldType::U64 },
            FieldDef { name: "space", ty: FieldType::U64 },
            FieldDef { name: "owner", ty: FieldType::Pubkey },
        ],
        account_names: &["new_account", "(optional) funding_account"],
        account_rule: AccountRule::Verbatim,
    },
];

/// Read a bincode-encoded seed from the reader: base pubkey (32 bytes) + u64 seed length + seed bytes.
fn read_seed_args(reader: &mut BinaryReader) -> Result<(Pubkey, String)> {
    let base = reader.read_pubkey()?;
    let seed_length = reader.read_u64()? as usize;
    if seed_length > 32 {
        anyhow::bail!("seed length exceeds 32 bytes");
    }
    let seed_bytes = reader.read_exact(seed_length)?;
    let seed = String::from_utf8(seed_bytes.to_vec()).context("invalid utf8 seed")?;
    Ok((base, seed))
}

/// Read a bincode-encoded seed (without base pubkey): u64 seed length + seed bytes.
fn read_seed(reader: &mut BinaryReader) -> Result<String> {
    let seed_length = reader.read_u64()? as usize;
    if seed_length > 32 {
        anyhow::bail!("seed length exceeds 32 bytes");
    }
    let seed_bytes = reader.read_exact(seed_length)?;
    String::from_utf8(seed_bytes.to_vec()).context("invalid utf8 seed")
}

fn parse_create_account_with_seed_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // CreateAccountWithSeed: 32 bytes base, 8 bytes seed length (bincode), seed bytes (up to 32), 8 bytes lamports, 8 bytes space, 32 bytes owner
    if data.len() < 89 {
        // Minimum: 32 + 8 + 1 + 8 + 8 + 32
        return Ok(None);
    }

    binary_reader::try_parse(data, |reader| {
        let (base, seed) = read_seed_args(reader)?;
        let lamports = reader.read_u64()?;
        let space = reader.read_u64()?;
        let owner = reader.read_pubkey()?;
        Ok(ParsedInstruction {
            name: "CreateAccountWithSeed".to_string(),
            fields: vec![
                ParsedField { name: "base".into(), value: IdlValue::Pubkey(base) },
                ParsedField { name: "seed".into(), value: IdlValue::String(seed) },
                ParsedField { name: "lamports".into(), value: IdlValue::U64(lamports) },
                ParsedField { name: "space".into(), value: IdlValue::U64(space) },
                ParsedField { name: "owner".into(), value: IdlValue::Pubkey(owner) },
            ]
            .into(),
            account_names: vec![
                "funding_account".to_string(),
                "created_account".to_string(),
                "base_account".to_string(),
            ],
        })
    })
}

fn parse_advance_nonce_account_instruction(
    _data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // AdvanceNonceAccount has no instruction data
    Ok(Some(ParsedInstruction {
        name: "AdvanceNonceAccount".to_string(),
        fields: vec![].into(),
        account_names: vec![
            "nonce_account".to_string(),
            "recent_blockhashes_sysvar".to_string(),
            "nonce_authority".to_string(),
        ],
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

    binary_reader::try_parse(data, |reader| {
        let (base, seed) = read_seed_args(reader)?;
        let space = reader.read_u64()?;
        let owner = reader.read_pubkey()?;
        Ok(ParsedInstruction {
            name: "AllocateWithSeed".to_string(),
            fields: vec![
                ParsedField { name: "base".into(), value: IdlValue::Pubkey(base) },
                ParsedField { name: "seed".into(), value: IdlValue::String(seed) },
                ParsedField { name: "space".into(), value: IdlValue::U64(space) },
                ParsedField { name: "owner".into(), value: IdlValue::Pubkey(owner) },
            ]
            .into(),
            account_names: vec!["allocated_account".to_string(), "base_account".to_string()],
        })
    })
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

    binary_reader::try_parse(data, |reader| {
        let (base, seed) = read_seed_args(reader)?;
        let owner = reader.read_pubkey()?;
        Ok(ParsedInstruction {
            name: "AssignWithSeed".to_string(),
            fields: vec![
                ParsedField { name: "base".into(), value: IdlValue::Pubkey(base) },
                ParsedField { name: "seed".into(), value: IdlValue::String(seed) },
                ParsedField { name: "owner".into(), value: IdlValue::Pubkey(owner) },
            ]
            .into(),
            account_names: vec!["assigned_account".to_string(), "base_account".to_string()],
        })
    })
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

    binary_reader::try_parse(data, |reader| {
        let lamports = reader.read_u64()?;
        let from_seed = read_seed(reader)?;
        let from_owner = reader.read_pubkey()?;
        Ok(ParsedInstruction {
            name: "TransferWithSeed".to_string(),
            fields: vec![
                ParsedField { name: "lamports".into(), value: IdlValue::U64(lamports) },
                ParsedField { name: "from_seed".into(), value: IdlValue::String(from_seed) },
                ParsedField { name: "from_owner".into(), value: IdlValue::Pubkey(from_owner) },
            ]
            .into(),
            account_names: vec![
                "funding_account".to_string(),
                "base_account".to_string(),
                "recipient_account".to_string(),
            ],
        })
    })
}

fn parse_upgrade_nonce_account_instruction(
    _data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    // UpgradeNonceAccount has no instruction data
    Ok(Some(ParsedInstruction {
        name: "UpgradeNonceAccount".to_string(),
        fields: vec![].into(),
        account_names: vec!["nonce_account".to_string()],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::transaction::{
        AccountReferenceSummary, AccountSourceSummary, InstructionSummary,
    };
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
