use anyhow::Result;
use solana_pubkey::Pubkey;
use sonar_idl::IdlValue;

fn option_pubkey_string(pubkey: Option<Pubkey>) -> String {
    pubkey.map_or_else(|| "none".to_string(), |pk| pk.to_string())
}

use super::spl_token_common::{
    self, WITHDRAW_EXCESS_LAMPORTS_DISCRIMINATOR, account_names_with_signers,
    extend_numbered_account_names, generate_generic_account_names, owned_account_names,
    parse_base_instruction, parsed_instruction,
};
use super::{InstructionParser, ParsedField, ParsedInstruction};
use crate::core::transaction::InstructionSummary;
use crate::parsers::binary_reader::{self, BinaryReader};

const TOKEN2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

// Token-2022-only discriminators (those not shared with Legacy SPL Token).
const INITIALIZE_MINT_CLOSE_AUTHORITY_DISCRIMINATOR: u8 = 25;
const TRANSFER_FEE_EXTENSION_DISCRIMINATOR: u8 = 26;
const REALLOCATE_DISCRIMINATOR: u8 = 29;
const CREATE_NATIVE_MINT_DISCRIMINATOR: u8 = 31;
const INITIALIZE_PERMANENT_DELEGATE_DISCRIMINATOR: u8 = 35;
const SCALED_UI_AMOUNT_EXTENSION_DISCRIMINATOR: u8 = 43;

/// Token-2022-only "extension prefix" instructions: discriminator selects the
/// extension family; sub-tag selects the specific instruction inside that
/// family. We don't decode the inner payload here — just label it.
struct ExtensionPrefixDef {
    discriminator: u8,
    name: &'static str,
}

static EXTENSION_PREFIX_INSTRUCTIONS: &[ExtensionPrefixDef] = &[
    ExtensionPrefixDef { discriminator: 27, name: "ConfidentialTransferExtension" },
    ExtensionPrefixDef { discriminator: 28, name: "DefaultAccountStateExtension" },
    ExtensionPrefixDef { discriminator: 30, name: "MemoTransferExtension" },
    ExtensionPrefixDef { discriminator: 33, name: "InterestBearingMintExtension" },
    ExtensionPrefixDef { discriminator: 34, name: "CpiGuardExtension" },
    ExtensionPrefixDef { discriminator: 36, name: "TransferHookExtension" },
    ExtensionPrefixDef { discriminator: 37, name: "ConfidentialTransferFeeExtension" },
    ExtensionPrefixDef { discriminator: 39, name: "MetadataPointerExtension" },
    ExtensionPrefixDef { discriminator: 40, name: "GroupPointerExtension" },
    ExtensionPrefixDef { discriminator: 41, name: "GroupMemberPointerExtension" },
    ExtensionPrefixDef { discriminator: 42, name: "ConfidentialMintBurnExtension" },
    ExtensionPrefixDef { discriminator: 44, name: "PausableExtension" },
];

const INITIALIZE_NON_TRANSFERABLE_MINT_DISCRIMINATOR: u8 = 32;

define_parser!(Token2022ProgramParser, TOKEN2022_PROGRAM_ID);

impl InstructionParser for Token2022ProgramParser {
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

        if let Some(def) =
            EXTENSION_PREFIX_INSTRUCTIONS.iter().find(|d| d.discriminator == instruction_id)
        {
            return parse_extension_prefix_instruction(def.name, data, instruction);
        }

        match instruction_id {
            INITIALIZE_MINT_CLOSE_AUTHORITY_DISCRIMINATOR => {
                parse_initialize_mint_close_authority_instruction(data, instruction)
            }
            TRANSFER_FEE_EXTENSION_DISCRIMINATOR => {
                parse_transfer_fee_extension_instruction(data, instruction)
            }
            REALLOCATE_DISCRIMINATOR => parse_reallocate_instruction(data, instruction),
            CREATE_NATIVE_MINT_DISCRIMINATOR => parse_create_native_mint_instruction(instruction),
            INITIALIZE_PERMANENT_DELEGATE_DISCRIMINATOR => {
                parse_initialize_permanent_delegate_instruction(data, instruction)
            }
            SCALED_UI_AMOUNT_EXTENSION_DISCRIMINATOR => {
                parse_scaled_ui_amount_extension_instruction(data, instruction)
            }
            INITIALIZE_NON_TRANSFERABLE_MINT_DISCRIMINATOR => {
                parse_initialize_non_transferable_mint_instruction(instruction)
            }
            WITHDRAW_EXCESS_LAMPORTS_DISCRIMINATOR => {
                spl_token_common::parse_withdraw_excess_lamports_instruction(instruction)
            }
            _ => Ok(None),
        }
    }
}

fn parse_initialize_non_transferable_mint_instruction(
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 1 {
        return Ok(None);
    }
    Ok(Some(parsed_instruction(
        "InitializeNonTransferableMint",
        vec![],
        owned_account_names(&["mint"]),
    )))
}

fn parse_initialize_mint_close_authority_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 1 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let close_authority = reader.read_option_pubkey()?;
        Ok(parsed_instruction(
            "InitializeMintCloseAuthority",
            vec![ParsedField {
                name: "close_authority".into(),
                value: IdlValue::String(option_pubkey_string(close_authority)),
            }],
            owned_account_names(&["mint"]),
        ))
    })
}

fn parse_transfer_fee_extension_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.is_empty() {
        return Ok(None);
    }

    let (sub_tag, payload) = data.split_first().unwrap();
    match sub_tag {
        0 => parse_transfer_fee_initialize_instruction(payload, instruction),
        1 => parse_transfer_checked_with_fee_instruction(payload, instruction),
        2 => parse_transfer_fee_withdraw_from_mint_instruction(instruction),
        3 => parse_transfer_fee_withdraw_from_accounts_instruction(payload, instruction),
        4 => parse_transfer_fee_harvest_to_mint_instruction(instruction),
        5 => parse_transfer_fee_set_fee_instruction(payload, instruction),
        unknown => Ok(Some(parsed_instruction(
            &format!("TransferFeeExtension({unknown})"),
            vec![ParsedField {
                name: "raw_extension_data".into(),
                value: IdlValue::String(hex::encode(payload)),
            }],
            generate_generic_account_names(instruction.accounts.len()),
        ))),
    }
}

fn parse_transfer_fee_initialize_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 1 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let config_authority = reader.read_option_pubkey()?;
        let withdraw_authority = reader.read_option_pubkey()?;
        let bps = reader.read_u16()?;
        let maximum_fee = reader.read_u64()?;
        Ok(parsed_instruction(
            "InitializeTransferFeeConfig",
            vec![
                ParsedField {
                    name: "transfer_fee_config_authority".into(),
                    value: IdlValue::String(option_pubkey_string(config_authority)),
                },
                ParsedField {
                    name: "withdraw_withheld_authority".into(),
                    value: IdlValue::String(option_pubkey_string(withdraw_authority)),
                },
                ParsedField { name: "transfer_fee_basis_points".into(), value: IdlValue::U16(bps) },
                ParsedField { name: "maximum_fee".into(), value: IdlValue::U64(maximum_fee) },
            ],
            owned_account_names(&["mint"]),
        ))
    })
}

fn parse_transfer_checked_with_fee_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 3 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let amount = reader.read_u64()?;
        let decimals = reader.read_u8()?;
        let fee = reader.read_u64()?;
        let base_account_names: &[&str] = if instruction.accounts.len() > 3 {
            &["source", "mint", "destination", "authority"]
        } else {
            &["source", "mint", "destination"]
        };

        Ok(parsed_instruction(
            "TransferCheckedWithFee",
            vec![
                ParsedField { name: "amount".into(), value: IdlValue::U64(amount) },
                ParsedField { name: "decimals".into(), value: IdlValue::U8(decimals) },
                ParsedField { name: "fee".into(), value: IdlValue::U64(fee) },
            ],
            account_names_with_signers(base_account_names, instruction.accounts.len()),
        ))
    })
}

fn parse_transfer_fee_withdraw_from_mint_instruction(
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 2 {
        return Ok(None);
    }

    let base_account_names: &[&str] = if instruction.accounts.len() >= 3 {
        &["mint", "destination", "authority"]
    } else {
        &["mint", "destination"]
    };

    Ok(Some(parsed_instruction(
        "WithdrawWithheldTokensFromMint",
        vec![],
        account_names_with_signers(base_account_names, instruction.accounts.len()),
    )))
}

fn parse_transfer_fee_withdraw_from_accounts_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.is_empty() || instruction.accounts.len() < 3 {
        return Ok(None);
    }
    let mut reader = BinaryReader::new(data);
    let num_token_accounts = reader.read_u8().unwrap_or(0) as usize;
    if instruction.accounts.len() < 3 + num_token_accounts {
        return Ok(None);
    }

    let mut account_names = owned_account_names(&["mint", "destination", "authority"]);
    let signers_count = instruction.accounts.len().saturating_sub(3 + num_token_accounts);
    extend_numbered_account_names(&mut account_names, "additional_signer_", 1, signers_count);
    extend_numbered_account_names(&mut account_names, "source_account_", 1, num_token_accounts);

    Ok(Some(parsed_instruction(
        "WithdrawWithheldTokensFromAccounts",
        vec![ParsedField {
            name: "num_token_accounts".into(),
            value: IdlValue::U32(num_token_accounts as u32),
        }],
        account_names,
    )))
}

fn parse_transfer_fee_harvest_to_mint_instruction(
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.is_empty() {
        return Ok(None);
    }

    let mut account_names = owned_account_names(&["mint"]);
    extend_numbered_account_names(
        &mut account_names,
        "source_account_",
        1,
        instruction.accounts.len() - 1,
    );

    Ok(Some(parsed_instruction("HarvestWithheldTokensToMint", vec![], account_names)))
}

fn parse_transfer_fee_set_fee_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.is_empty() {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let transfer_fee_basis_points = reader.read_u16()?;
        let maximum_fee = reader.read_u64()?;
        let base_account_names: &[&str] =
            if instruction.accounts.len() >= 2 { &["mint", "authority"] } else { &["mint"] };

        Ok(parsed_instruction(
            "SetTransferFee",
            vec![
                ParsedField {
                    name: "transfer_fee_basis_points".into(),
                    value: IdlValue::String(transfer_fee_basis_points.to_string()),
                },
                ParsedField { name: "maximum_fee".into(), value: IdlValue::U64(maximum_fee) },
            ],
            account_names_with_signers(base_account_names, instruction.accounts.len()),
        ))
    })
}

fn parse_reallocate_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() % 2 != 0 {
        return Ok(None);
    }

    let mut reader = BinaryReader::new(data);
    let mut extension_types = Vec::new();
    while reader.has_remaining() {
        if let Ok(ext_type) = reader.read_u16() {
            extension_types.push(ext_type.to_string());
        }
    }

    let named = ["account", "payer", "system_program", "owner_or_delegate"];
    let base_account_names = &named[..instruction.accounts.len().min(named.len())];

    Ok(Some(parsed_instruction(
        "Reallocate",
        vec![ParsedField {
            name: "extension_types".into(),
            value: IdlValue::String(if extension_types.is_empty() {
                "none".to_string()
            } else {
                extension_types.join(",")
            }),
        }],
        account_names_with_signers(base_account_names, instruction.accounts.len()),
    )))
}

fn parse_create_native_mint_instruction(
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 3 {
        return Ok(None);
    }

    Ok(Some(parsed_instruction(
        "CreateNativeMint",
        vec![],
        owned_account_names(&["funding_account", "native_mint", "system_program"]),
    )))
}

fn parse_initialize_permanent_delegate_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 1 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let delegate = reader.read_pubkey_as_string()?;
        Ok(parsed_instruction(
            "InitializePermanentDelegate",
            vec![ParsedField { name: "delegate".into(), value: IdlValue::String(delegate) }],
            owned_account_names(&["mint"]),
        ))
    })
}

/// `ScaledUiAmountMint` extension (discriminator 43). The first payload byte is
/// a sub-tag selecting `Initialize` (0) or `UpdateMultiplier` (1).
fn parse_scaled_ui_amount_extension_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.is_empty() {
        return Ok(None);
    }

    let (sub_tag, payload) = data.split_first().unwrap();
    match sub_tag {
        0 => parse_scaled_ui_amount_initialize_instruction(payload, instruction),
        1 => parse_scaled_ui_amount_update_multiplier_instruction(payload, instruction),
        unknown => Ok(Some(parsed_instruction(
            &format!("ScaledUiAmountExtension({unknown})"),
            vec![ParsedField {
                name: "raw_extension_data".into(),
                value: IdlValue::String(hex::encode(payload)),
            }],
            generate_generic_account_names(instruction.accounts.len()),
        ))),
    }
}

fn parse_scaled_ui_amount_initialize_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 1 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let authority = reader.read_optional_non_zero_pubkey()?;
        let multiplier = reader.read_f64()?;
        Ok(parsed_instruction(
            "InitializeScaledUiAmountConfig",
            vec![
                ParsedField {
                    name: "authority".into(),
                    value: IdlValue::String(option_pubkey_string(authority)),
                },
                ParsedField {
                    name: "multiplier".into(),
                    value: IdlValue::String(multiplier.to_string()),
                },
            ],
            owned_account_names(&["mint"]),
        ))
    })
}

fn parse_scaled_ui_amount_update_multiplier_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.is_empty() {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let multiplier = reader.read_f64()?;
        let effective_timestamp = reader.read_i64()?;
        let base_account_names: &[&str] =
            if instruction.accounts.len() >= 2 { &["mint", "authority"] } else { &["mint"] };
        Ok(parsed_instruction(
            "UpdateMultiplier",
            vec![
                ParsedField {
                    name: "multiplier".into(),
                    value: IdlValue::String(multiplier.to_string()),
                },
                ParsedField {
                    name: "effective_timestamp".into(),
                    value: IdlValue::I64(effective_timestamp),
                },
            ],
            account_names_with_signers(base_account_names, instruction.accounts.len()),
        ))
    })
}

fn parse_extension_prefix_instruction(
    name: &str,
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    let mut fields = Vec::new();
    if !data.is_empty() {
        fields.push(ParsedField {
            name: "raw_extension_data".into(),
            value: IdlValue::String(hex::encode(data)),
        });
    }

    Ok(Some(parsed_instruction(
        name,
        fields,
        generate_generic_account_names(instruction.accounts.len()),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::transaction::{AccountReferenceSummary, AccountSourceSummary};

    fn create_test_instruction(
        data: Vec<u8>,
        accounts: Vec<AccountReferenceSummary>,
    ) -> InstructionSummary {
        InstructionSummary {
            index: 0,
            program: AccountReferenceSummary {
                index: 6,
                pubkey: Some(TOKEN2022_PROGRAM_ID.to_string()),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            accounts,
            data: data.into_boxed_slice(),
        }
    }

    fn create_test_account(
        index: usize,
        pubkey: &str,
        signer: bool,
        writable: bool,
    ) -> AccountReferenceSummary {
        AccountReferenceSummary {
            index,
            pubkey: Some(pubkey.to_string()),
            signer,
            writable,
            source: AccountSourceSummary::Static,
        }
    }

    #[test]
    fn test_transfer_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "SourcePubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "DestPubkey1111111111111111111111111111111111", false, true),
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", true, false),
        ];

        let mut data = vec![3];
        data.extend_from_slice(&1_000_000_u64.to_le_bytes());

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "Transfer");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "source");
        assert_eq!(parsed.account_names[1], "destination");
        assert_eq!(parsed.account_names[2], "owner");

        assert!(
            parsed.fields.iter().any(|field| field.name == "amount" && field.value == "1000000")
        );
    }

    #[test]
    fn test_transfer_instruction_invalid_data_length() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "SourcePubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "DestPubkey1111111111111111111111111111111111", false, true),
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", true, false),
        ];

        let mut data = vec![3];
        data.extend_from_slice(&[1, 2, 3, 4]);

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_transfer_instruction_rejects_insufficient_accounts() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "SourcePubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "DestPubkey1111111111111111111111111111111111", false, true),
        ];

        let mut data = vec![3];
        data.extend_from_slice(&1_000_u64.to_le_bytes());

        let instruction = create_test_instruction(data, accounts);
        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_initialize_mint_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "MintPubkey1111111111111111111111111111111111", false, true),
            create_test_account(1, "RentSysvar111111111111111111111111111111111", false, false),
        ];

        let mut data = vec![0];
        data.push(9);
        data.extend_from_slice(&[1u8; 32]);
        data.push(0);

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "InitializeMint");
        assert_eq!(parsed.account_names.len(), 2);
        assert_eq!(parsed.account_names[0], "mint");
        assert_eq!(parsed.account_names[1], "rent_sysvar");

        assert!(parsed.fields.iter().any(|field| field.name == "decimals" && field.value == "9"));
    }

    #[test]
    fn test_initialize_account_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", false, true),
            create_test_account(1, "MintPubkey111111111111111111111111111111111", false, false),
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", false, false),
            create_test_account(3, "RentSysvar111111111111111111111111111111111", false, false),
        ];

        let data = vec![1];

        let instruction = create_test_instruction(data, accounts);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "InitializeAccount");
        assert_eq!(
            parsed.account_names,
            vec!["account", "mint", "owner", "rent_sysvar"]
        );
    }

    #[test]
    fn test_initialize_multisig_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "MultisigPubkey1111111111111111111111111111111", false, true),
            create_test_account(1, "Signer1Pubkey11111111111111111111111111111111", true, false),
            create_test_account(2, "Signer2Pubkey11111111111111111111111111111111", true, false),
            create_test_account(3, "RentSysvar111111111111111111111111111111111", false, false),
        ];

        let data = vec![2, 2];

        let instruction = create_test_instruction(data, accounts);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "InitializeMultisig");
        assert_eq!(
            parsed.account_names,
            vec!["multisig", "signer_1", "signer_2", "rent_sysvar"]
        );
        assert!(parsed.fields.iter().any(|field| field.name == "m" && field.value == "2"));
    }

    #[test]
    fn test_approve_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "SourcePubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "DelegatePubkey1111111111111111111111111111111", false, true),
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", true, false),
        ];

        let mut data = vec![4];
        data.extend_from_slice(&500_000_u64.to_le_bytes());

        let instruction = create_test_instruction(data, accounts);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "Approve");
        assert_eq!(parsed.account_names, vec!["source", "delegate", "owner"]);
        assert!(
            parsed.fields.iter().any(|field| field.name == "amount" && field.value == "500000")
        );
    }

    #[test]
    fn test_revoke_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "SourcePubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "OwnerPubkey111111111111111111111111111111111", true, false),
        ];

        let data = vec![5];

        let instruction = create_test_instruction(data, accounts);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "Revoke");
        assert_eq!(parsed.account_names, vec!["source", "owner"]);
    }

    #[test]
    fn test_set_authority_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", false, true),
            create_test_account(1, "AuthorityPubkey11111111111111111111111111111", true, false),
        ];

        let mut data = vec![6, 0, 1];
        data.extend_from_slice(&[7u8; 32]);

        let instruction = create_test_instruction(data, accounts);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "SetAuthority");
        assert_eq!(parsed.account_names, vec!["account", "authority"]);
        assert!(
            parsed
                .fields
                .iter()
                .any(|field| field.name == "authority_type" && field.value == "MintTokens")
        );
        assert!(
            parsed.fields.iter().any(|field| field.name == "cleared" && field.value == "false")
        );
    }

    #[test]
    fn test_set_authority_instruction_rejects_insufficient_accounts() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![create_test_account(
            0,
            "AccountPubkey11111111111111111111111111111111",
            false,
            true,
        )];

        let data = vec![6, 0, 0];
        let instruction = create_test_instruction(data, accounts);
        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_mint_to_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "MintPubkey111111111111111111111111111111111", false, true),
            create_test_account(1, "AccountPubkey11111111111111111111111111111111", false, true),
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", true, false),
        ];
        let mut data = vec![7];
        data.extend_from_slice(&1_000_000_u64.to_le_bytes());
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "MintTo");
        assert_eq!(parsed.account_names, vec!["mint", "account", "owner"]);
    }

    #[test]
    fn test_burn_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "MintPubkey111111111111111111111111111111111", false, false),
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", true, false),
        ];
        let mut data = vec![8];
        data.extend_from_slice(&250_000_u64.to_le_bytes());
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "Burn");
        assert_eq!(parsed.account_names, vec!["account", "mint", "owner"]);
    }

    #[test]
    fn test_close_account_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "DestinationPubkey11111111111111111111111111111", false, true),
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", true, false),
        ];
        let instruction = create_test_instruction(vec![9], accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "CloseAccount");
        assert_eq!(parsed.account_names, vec!["account", "destination", "owner"]);
    }

    #[test]
    fn test_freeze_account_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "MintPubkey111111111111111111111111111111111", false, false),
            create_test_account(2, "FreezeAuthorityPubkey111111111111111111111111", true, false),
        ];
        let instruction = create_test_instruction(vec![10], accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "FreezeAccount");
        assert_eq!(parsed.account_names, vec!["account", "mint", "freeze_authority"]);
    }

    #[test]
    fn test_thaw_account_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "MintPubkey111111111111111111111111111111111", false, false),
            create_test_account(2, "FreezeAuthorityPubkey111111111111111111111111", true, false),
        ];
        let instruction = create_test_instruction(vec![11], accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "ThawAccount");
        assert_eq!(parsed.account_names, vec!["account", "mint", "freeze_authority"]);
    }

    #[test]
    fn test_transfer_checked_instruction_with_multiple_signers() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "SourcePubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "MintPubkey111111111111111111111111111111111", false, false),
            create_test_account(2, "DestPubkey1111111111111111111111111111111111", false, true),
            create_test_account(3, "OwnerPubkey111111111111111111111111111111111", false, false),
            create_test_account(4, "Signer1Pubkey1111111111111111111111111111111", true, false),
            create_test_account(5, "Signer2Pubkey1111111111111111111111111111111", true, false),
        ];

        let mut data = vec![12];
        data.extend_from_slice(&750_000_u64.to_le_bytes());
        data.push(9);

        let instruction = create_test_instruction(data, accounts);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "TransferChecked");
        assert_eq!(
            parsed.account_names,
            vec![
                "source",
                "mint",
                "destination",
                "owner",
                "additional_signer_1",
                "additional_signer_2"
            ]
        );
    }

    #[test]
    fn test_approve_checked_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "SourcePubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "MintPubkey111111111111111111111111111111111", false, false),
            create_test_account(2, "DelegatePubkey1111111111111111111111111111111", false, true),
            create_test_account(3, "OwnerPubkey111111111111111111111111111111111", true, false),
        ];
        let mut data = vec![13];
        data.extend_from_slice(&300_000_u64.to_le_bytes());
        data.push(6);
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "ApproveChecked");
        assert_eq!(parsed.account_names, vec!["source", "mint", "delegate", "owner"]);
    }

    #[test]
    fn test_initialize_account2_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", false, true),
            create_test_account(1, "MintPubkey111111111111111111111111111111111", false, false),
            create_test_account(2, "RentSysvar111111111111111111111111111111111", false, false),
        ];
        let mut data = vec![16];
        data.extend_from_slice(&[0u8; 32]);
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "InitializeAccount2");
    }

    #[test]
    fn test_sync_native_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![create_test_account(
            0,
            "NativeAccountPubkey1111111111111111111111111",
            false,
            true,
        )];
        let instruction = create_test_instruction(vec![17], accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "SyncNative");
    }

    #[test]
    fn test_ui_amount_to_amount_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![create_test_account(
            0,
            "MintPubkey111111111111111111111111111111111",
            false,
            false,
        )];
        let mut data = vec![24];
        data.extend_from_slice(b"100.50");
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "UiAmountToAmount");
        assert!(
            parsed.fields.iter().any(|field| field.name == "ui_amount" && field.value == "100.50")
        );
    }

    #[test]
    fn test_initialize_mint_close_authority_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![create_test_account(
            0,
            "MintPubkey111111111111111111111111111111111",
            false,
            true,
        )];
        let mut data = vec![25, 1];
        data.extend_from_slice(&[2u8; 32]);
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "InitializeMintCloseAuthority");
    }

    #[test]
    fn test_reallocate_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", false, true),
            create_test_account(1, "PayerPubkey1111111111111111111111111111111111", true, true),
            create_test_account(2, "SystemProgram1111111111111111111111111111111", false, false),
            create_test_account(3, "OwnerPubkey111111111111111111111111111111111", true, false),
        ];
        let mut data = vec![29];
        data.extend_from_slice(&1u16.to_le_bytes());
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "Reallocate");
    }

    #[test]
    fn test_create_native_mint_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "FundingPubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "NativeMintPubkey111111111111111111111111111", false, true),
            create_test_account(2, "SystemProgram1111111111111111111111111111111", false, false),
        ];
        let instruction = create_test_instruction(vec![31], accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "CreateNativeMint");
    }

    #[test]
    fn test_withdraw_excess_lamports_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "SourcePubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "DestPubkey1111111111111111111111111111111111", false, true),
            create_test_account(2, "AuthorityPubkey11111111111111111111111111111", true, false),
        ];
        let instruction = create_test_instruction(vec![38], accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "WithdrawExcessLamports");
        assert_eq!(parsed.account_names, vec!["source", "destination", "authority"]);
    }

    #[test]
    fn test_extension_prefix_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![create_test_account(
            0,
            "AccountPubkey11111111111111111111111111111111",
            false,
            true,
        )];
        let data = vec![27, 0xAB, 0xCD];
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "ConfidentialTransferExtension");
        assert!(
            parsed
                .fields
                .iter()
                .any(|field| field.name == "raw_extension_data" && field.value == "abcd")
        );
    }

    #[test]
    fn test_scaled_ui_amount_update_multiplier_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "MintPubkey11111111111111111111111111111111111", false, true),
            create_test_account(
                1,
                "AuthorityPubkey11111111111111111111111111111",
                true,
                false,
            ),
        ];
        // discriminator 43, sub-tag 1 (UpdateMultiplier), f64 multiplier, i64 timestamp.
        let mut data = vec![43, 1];
        data.extend_from_slice(&2.5_f64.to_le_bytes());
        data.extend_from_slice(&1_000_i64.to_le_bytes());
        let instruction = create_test_instruction(data, accounts);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "UpdateMultiplier");
        assert_eq!(parsed.account_names, vec!["mint", "authority"]);
        assert!(parsed.fields.iter().any(|f| f.name == "multiplier" && f.value == "2.5"));
        assert!(
            parsed.fields.iter().any(|f| f.name == "effective_timestamp" && f.value == "1000")
        );
        // Should NOT fall back to the raw dump.
        assert!(!parsed.fields.iter().any(|f| f.name == "raw_extension_data"));
    }

    #[test]
    fn test_scaled_ui_amount_initialize_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![create_test_account(
            0,
            "MintPubkey11111111111111111111111111111111111",
            false,
            true,
        )];
        // discriminator 43, sub-tag 0 (Initialize), all-zero authority (None), f64 multiplier.
        let mut data = vec![43, 0];
        data.extend_from_slice(&[0u8; 32]);
        data.extend_from_slice(&1.0_f64.to_le_bytes());
        let instruction = create_test_instruction(data, accounts);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "InitializeScaledUiAmountConfig");
        assert_eq!(parsed.account_names, vec!["mint"]);
        assert!(parsed.fields.iter().any(|f| f.name == "authority" && f.value == "none"));
        assert!(parsed.fields.iter().any(|f| f.name == "multiplier" && f.value == "1"));
    }
}
