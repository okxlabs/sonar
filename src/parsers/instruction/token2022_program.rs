use anyhow::Result;
use solana_pubkey::Pubkey;
use sonar_idl::IdlValue;

use super::{InstructionParser, ParsedField, ParsedInstruction};
use crate::core::transaction::InstructionSummary;
use crate::parsers::binary_reader::{self, BinaryReader};

/// Token2022 program ID: TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb
const TOKEN2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

/// Instruction discriminators for Token2022 program
/// Token2022 uses the same discriminators as Token for basic instructions
const INITIALIZE_MINT_DISCRIMINATOR: u8 = 0;
const INITIALIZE_ACCOUNT_DISCRIMINATOR: u8 = 1;
const INITIALIZE_MULTISIG_DISCRIMINATOR: u8 = 2;
const SET_AUTHORITY_DISCRIMINATOR: u8 = 6;
const INITIALIZE_ACCOUNT2_DISCRIMINATOR: u8 = 16;
const INITIALIZE_ACCOUNT3_DISCRIMINATOR: u8 = 18;
const INITIALIZE_MULTISIG2_DISCRIMINATOR: u8 = 19;
const INITIALIZE_MINT2_DISCRIMINATOR: u8 = 20;
const AMOUNT_TO_UI_AMOUNT_DISCRIMINATOR: u8 = 23;
const UI_AMOUNT_TO_AMOUNT_DISCRIMINATOR: u8 = 24;
const INITIALIZE_MINT_CLOSE_AUTHORITY_DISCRIMINATOR: u8 = 25;
const TRANSFER_FEE_EXTENSION_DISCRIMINATOR: u8 = 26;
const REALLOCATE_DISCRIMINATOR: u8 = 29;
const CREATE_NATIVE_MINT_DISCRIMINATOR: u8 = 31;
const INITIALIZE_PERMANENT_DELEGATE_DISCRIMINATOR: u8 = 35;
const WITHDRAW_EXCESS_LAMPORTS_DISCRIMINATOR: u8 = 38;

/// Data layout patterns shared by multiple Token-2022 instructions.
enum DataLayout {
    /// 8-byte u64 amount
    Amount,
    /// 8-byte u64 amount + 1-byte u8 decimals
    AmountDecimals,
    /// No instruction data
    NoData,
    /// Single account, no data
    SingleAccount,
    /// Extension prefix: read sub-discriminator byte, pass to extension handler
    ExtensionPrefix,
}

/// Declarative definition for a Token-2022 instruction that follows a common pattern.
struct InstructionDef {
    discriminator: u8,
    name: &'static str,
    layout: DataLayout,
    min_accounts: usize,
    account_names: &'static [&'static str],
}

static INSTRUCTIONS: &[InstructionDef] = &[
    // Amount (u64 only)
    InstructionDef {
        discriminator: 3,
        name: "Transfer",
        layout: DataLayout::Amount,
        min_accounts: 3,
        account_names: &["source", "destination", "owner"],
    },
    InstructionDef {
        discriminator: 4,
        name: "Approve",
        layout: DataLayout::Amount,
        min_accounts: 3,
        account_names: &["source", "delegate", "owner"],
    },
    InstructionDef {
        discriminator: 7,
        name: "MintTo",
        layout: DataLayout::Amount,
        min_accounts: 3,
        account_names: &["mint", "account", "owner"],
    },
    InstructionDef {
        discriminator: 8,
        name: "Burn",
        layout: DataLayout::Amount,
        min_accounts: 3,
        account_names: &["account", "mint", "owner"],
    },
    // AmountDecimals (u64 + u8)
    InstructionDef {
        discriminator: 12,
        name: "TransferChecked",
        layout: DataLayout::AmountDecimals,
        min_accounts: 4,
        account_names: &["source", "mint", "destination", "owner"],
    },
    InstructionDef {
        discriminator: 13,
        name: "ApproveChecked",
        layout: DataLayout::AmountDecimals,
        min_accounts: 4,
        account_names: &["source", "mint", "delegate", "owner"],
    },
    InstructionDef {
        discriminator: 14,
        name: "MintToChecked",
        layout: DataLayout::AmountDecimals,
        min_accounts: 3,
        account_names: &["mint", "account", "owner"],
    },
    InstructionDef {
        discriminator: 15,
        name: "BurnChecked",
        layout: DataLayout::AmountDecimals,
        min_accounts: 3,
        account_names: &["account", "mint", "owner"],
    },
    // NoData
    InstructionDef {
        discriminator: 5,
        name: "Revoke",
        layout: DataLayout::NoData,
        min_accounts: 2,
        account_names: &["source", "owner"],
    },
    InstructionDef {
        discriminator: 9,
        name: "CloseAccount",
        layout: DataLayout::NoData,
        min_accounts: 3,
        account_names: &["account", "destination", "owner"],
    },
    InstructionDef {
        discriminator: 10,
        name: "FreezeAccount",
        layout: DataLayout::NoData,
        min_accounts: 3,
        account_names: &["account", "mint", "freeze_authority"],
    },
    InstructionDef {
        discriminator: 11,
        name: "ThawAccount",
        layout: DataLayout::NoData,
        min_accounts: 3,
        account_names: &["account", "mint", "freeze_authority"],
    },
    // SingleAccount
    InstructionDef {
        discriminator: 17,
        name: "SyncNative",
        layout: DataLayout::SingleAccount,
        min_accounts: 1,
        account_names: &["account"],
    },
    InstructionDef {
        discriminator: 21,
        name: "GetAccountDataSize",
        layout: DataLayout::SingleAccount,
        min_accounts: 1,
        account_names: &["mint"],
    },
    InstructionDef {
        discriminator: 22,
        name: "InitializeImmutableOwner",
        layout: DataLayout::SingleAccount,
        min_accounts: 1,
        account_names: &["account"],
    },
    InstructionDef {
        discriminator: 32,
        name: "InitializeNonTransferableMint",
        layout: DataLayout::SingleAccount,
        min_accounts: 1,
        account_names: &["mint"],
    },
    // ExtensionPrefix
    InstructionDef {
        discriminator: 27,
        name: "ConfidentialTransferExtension",
        layout: DataLayout::ExtensionPrefix,
        min_accounts: 0,
        account_names: &[],
    },
    InstructionDef {
        discriminator: 28,
        name: "DefaultAccountStateExtension",
        layout: DataLayout::ExtensionPrefix,
        min_accounts: 0,
        account_names: &[],
    },
    InstructionDef {
        discriminator: 30,
        name: "MemoTransferExtension",
        layout: DataLayout::ExtensionPrefix,
        min_accounts: 0,
        account_names: &[],
    },
    InstructionDef {
        discriminator: 33,
        name: "InterestBearingMintExtension",
        layout: DataLayout::ExtensionPrefix,
        min_accounts: 0,
        account_names: &[],
    },
    InstructionDef {
        discriminator: 34,
        name: "CpiGuardExtension",
        layout: DataLayout::ExtensionPrefix,
        min_accounts: 0,
        account_names: &[],
    },
    InstructionDef {
        discriminator: 36,
        name: "TransferHookExtension",
        layout: DataLayout::ExtensionPrefix,
        min_accounts: 0,
        account_names: &[],
    },
    InstructionDef {
        discriminator: 37,
        name: "ConfidentialTransferFeeExtension",
        layout: DataLayout::ExtensionPrefix,
        min_accounts: 0,
        account_names: &[],
    },
    InstructionDef {
        discriminator: 39,
        name: "MetadataPointerExtension",
        layout: DataLayout::ExtensionPrefix,
        min_accounts: 0,
        account_names: &[],
    },
    InstructionDef {
        discriminator: 40,
        name: "GroupPointerExtension",
        layout: DataLayout::ExtensionPrefix,
        min_accounts: 0,
        account_names: &[],
    },
    InstructionDef {
        discriminator: 41,
        name: "GroupMemberPointerExtension",
        layout: DataLayout::ExtensionPrefix,
        min_accounts: 0,
        account_names: &[],
    },
    InstructionDef {
        discriminator: 42,
        name: "ConfidentialMintBurnExtension",
        layout: DataLayout::ExtensionPrefix,
        min_accounts: 0,
        account_names: &[],
    },
    InstructionDef {
        discriminator: 43,
        name: "ScaledUiAmountExtension",
        layout: DataLayout::ExtensionPrefix,
        min_accounts: 0,
        account_names: &[],
    },
    InstructionDef {
        discriminator: 44,
        name: "PausableExtension",
        layout: DataLayout::ExtensionPrefix,
        min_accounts: 0,
        account_names: &[],
    },
];

// Parser for the Token2022 Program instructions.
// Supports all Token2022 Program instructions including Transfer and TransferChecked.
define_parser!(Token2022ProgramParser, TOKEN2022_PROGRAM_ID);

impl Token2022ProgramParser {
    pub fn with_program_id(program_id: Pubkey) -> Self {
        Self { program_id }
    }
}

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

        // Try table-driven dispatch first
        if let Some(def) = INSTRUCTIONS.iter().find(|d| d.discriminator == instruction_id) {
            return dispatch_table_instruction(def, data, instruction);
        }

        // Custom instructions that need individual parse functions
        match instruction_id {
            INITIALIZE_MINT_DISCRIMINATOR => parse_initialize_mint_instruction(data, instruction),
            INITIALIZE_ACCOUNT_DISCRIMINATOR => {
                parse_initialize_account_instruction(data, instruction)
            }
            INITIALIZE_MULTISIG_DISCRIMINATOR => {
                parse_initialize_multisig_instruction(data, instruction)
            }
            SET_AUTHORITY_DISCRIMINATOR => parse_set_authority_instruction(data, instruction),
            INITIALIZE_ACCOUNT2_DISCRIMINATOR => {
                parse_initialize_account2_instruction(data, instruction)
            }
            INITIALIZE_ACCOUNT3_DISCRIMINATOR => {
                parse_initialize_account3_instruction(data, instruction)
            }
            INITIALIZE_MULTISIG2_DISCRIMINATOR => {
                parse_initialize_multisig2_instruction(data, instruction)
            }
            INITIALIZE_MINT2_DISCRIMINATOR => parse_initialize_mint2_instruction(data, instruction),
            AMOUNT_TO_UI_AMOUNT_DISCRIMINATOR => {
                parse_amount_to_ui_amount_instruction(data, instruction)
            }
            UI_AMOUNT_TO_AMOUNT_DISCRIMINATOR => {
                parse_ui_amount_to_amount_instruction(data, instruction)
            }
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
            WITHDRAW_EXCESS_LAMPORTS_DISCRIMINATOR => {
                parse_withdraw_excess_lamports_instruction(instruction)
            }
            _ => Ok(None),
        }
    }
}

fn dispatch_table_instruction(
    def: &InstructionDef,
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    match def.layout {
        DataLayout::Amount => {
            if data.len() != 8 || instruction.accounts.len() < def.min_accounts {
                return Ok(None);
            }
            binary_reader::try_parse(data, |reader| {
                let amount = reader.read_u64()?;
                let mut account_names: Vec<String> =
                    def.account_names.iter().map(|s| s.to_string()).collect();
                append_additional_signer_accounts(
                    &mut account_names,
                    def.min_accounts,
                    instruction.accounts.len(),
                );
                Ok(ParsedInstruction {
                    name: def.name.to_string(),
                    fields: vec![ParsedField {
                        name: "amount".into(),
                        value: IdlValue::U64(amount),
                    }]
                    .into(),
                    account_names,
                })
            })
        }
        DataLayout::AmountDecimals => {
            if data.len() != 9 || instruction.accounts.len() < def.min_accounts {
                return Ok(None);
            }
            binary_reader::try_parse(data, |reader| {
                let amount = reader.read_u64()?;
                let decimals = reader.read_u8()?;
                let mut account_names: Vec<String> =
                    def.account_names.iter().map(|s| s.to_string()).collect();
                append_additional_signer_accounts(
                    &mut account_names,
                    def.min_accounts,
                    instruction.accounts.len(),
                );
                Ok(ParsedInstruction {
                    name: def.name.to_string(),
                    fields: vec![
                        ParsedField { name: "amount".into(), value: IdlValue::U64(amount) },
                        ParsedField { name: "decimals".into(), value: IdlValue::U8(decimals) },
                    ]
                    .into(),
                    account_names,
                })
            })
        }
        DataLayout::NoData => {
            if instruction.accounts.len() < def.min_accounts {
                return Ok(None);
            }
            let mut account_names: Vec<String> =
                def.account_names.iter().map(|s| s.to_string()).collect();
            append_additional_signer_accounts(
                &mut account_names,
                def.min_accounts,
                instruction.accounts.len(),
            );
            Ok(Some(ParsedInstruction {
                name: def.name.to_string(),
                fields: vec![].into(),
                account_names,
            }))
        }
        DataLayout::SingleAccount => {
            if instruction.accounts.len() != 1 {
                return Ok(None);
            }
            Ok(Some(ParsedInstruction {
                name: def.name.to_string(),
                fields: vec![].into(),
                account_names: def.account_names.iter().map(|s| s.to_string()).collect(),
            }))
        }
        DataLayout::ExtensionPrefix => {
            parse_extension_prefix_instruction(def.name, data, instruction)
        }
    }
}

fn append_additional_signer_accounts(
    account_names: &mut Vec<String>,
    base_accounts: usize,
    total_accounts: usize,
) {
    super::append_extra_account_names(
        account_names,
        total_accounts,
        base_accounts,
        "additional_signer_",
    );
}

/// Parses an InitializeMint instruction: 0
fn parse_initialize_mint_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() < 34 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let decimals = reader.read_u8()?;
        let _mint_authority = reader.read_pubkey()?;
        let has_freeze_authority = reader.read_bool()?;
        if has_freeze_authority {
            let _freeze_authority = reader.read_pubkey()?;
        }
        Ok(ParsedInstruction {
            name: "InitializeMint".to_string(),
            fields: vec![
                ParsedField { name: "decimals".into(), value: IdlValue::U8(decimals) },
                ParsedField {
                    name: "has_freeze_authority".into(),
                    value: IdlValue::Bool(has_freeze_authority),
                },
            ]
            .into(),
            account_names: vec!["mint".to_string(), "rent_sysvar".to_string()],
        })
    })
}

/// Parses an InitializeAccount instruction: 1
fn parse_initialize_account_instruction(
    _data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 4 {
        return Ok(None); // Invalid number of accounts for InitializeAccount
    }

    Ok(Some(ParsedInstruction {
        name: "InitializeAccount".to_string(),
        fields: vec![].into(),
        account_names: vec![
            "account".to_string(),
            "mint".to_string(),
            "owner".to_string(),
            "rent_sysvar".to_string(),
        ],
    }))
}

/// Parses an InitializeMultisig instruction: 2
fn parse_initialize_multisig_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 1 || instruction.accounts.len() < 3 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let m = reader.read_u8()?;
        let mut account_names = vec!["multisig".to_string(), "rent_sysvar".to_string()];
        let num_signers = instruction.accounts.len().saturating_sub(2);
        for i in 0..num_signers {
            account_names.insert(1 + i, format!("signer_{}", i + 1));
        }
        Ok(ParsedInstruction {
            name: "InitializeMultisig".to_string(),
            fields: vec![ParsedField { name: "m".into(), value: IdlValue::U8(m) }].into(),
            account_names,
        })
    })
}

/// Parses a SetAuthority instruction: 6
fn parse_set_authority_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() < 2 || instruction.accounts.len() < 2 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let authority_type = match reader.read_u8()? {
            0 => "MintTokens",
            1 => "FreezeAccount",
            2 => "AccountOwner",
            3 => "CloseAccount",
            _ => "Unknown",
        };
        let option_tag = reader.read_u8()?;
        let cleared = match option_tag {
            0 => true,
            1 => {
                let _new_authority = reader.read_pubkey()?;
                false
            }
            other => anyhow::bail!("invalid option tag {other}"),
        };
        let mut account_names = vec!["account".to_string(), "authority".to_string()];
        append_additional_signer_accounts(&mut account_names, 2, instruction.accounts.len());
        Ok(ParsedInstruction {
            name: "SetAuthority".to_string(),
            fields: vec![
                ParsedField {
                    name: "authority_type".into(),
                    value: IdlValue::String(authority_type.to_string().into()),
                },
                ParsedField { name: "cleared".into(), value: IdlValue::Bool(cleared) },
            ]
            .into(),
            account_names,
        })
    })
}

/// Parses an InitializeAccount2 instruction: 16
fn parse_initialize_account2_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 32 || instruction.accounts.len() != 3 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let owner_pubkey = reader.read_pubkey_as_string()?;
        Ok(ParsedInstruction {
            name: "InitializeAccount2".to_string(),
            fields: vec![ParsedField {
                name: "owner".into(),
                value: IdlValue::String(owner_pubkey.into()),
            }]
            .into(),
            account_names: vec![
                "account".to_string(),
                "mint".to_string(),
                "rent_sysvar".to_string(),
            ],
        })
    })
}

/// Parses an InitializeAccount3 instruction: 18
fn parse_initialize_account3_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 32 || instruction.accounts.len() < 2 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let owner_pubkey = reader.read_pubkey_as_string()?;
        let mut account_names = vec!["account".to_string(), "mint".to_string()];
        for i in 2..instruction.accounts.len() {
            account_names.push(format!("account_{}", i + 1));
        }
        Ok(ParsedInstruction {
            name: "InitializeAccount3".to_string(),
            fields: vec![ParsedField {
                name: "owner".into(),
                value: IdlValue::String(owner_pubkey.into()),
            }]
            .into(),
            account_names,
        })
    })
}

/// Parses an InitializeMultisig2 instruction: 19
fn parse_initialize_multisig2_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 1 || instruction.accounts.len() < 2 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let m = reader.read_u8()?;
        let mut account_names = vec!["multisig".to_string()];
        let num_signers = instruction.accounts.len().saturating_sub(1);
        for i in 0..num_signers {
            account_names.push(format!("signer_{}", i + 1));
        }
        Ok(ParsedInstruction {
            name: "InitializeMultisig2".to_string(),
            fields: vec![ParsedField { name: "m".into(), value: IdlValue::U8(m) }].into(),
            account_names,
        })
    })
}

/// Parses an InitializeMint2 instruction: 20
fn parse_initialize_mint2_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() < 34 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let decimals = reader.read_u8()?;
        let _mint_authority = reader.read_pubkey()?;
        let has_freeze_authority = reader.read_bool()?;
        if has_freeze_authority {
            let _freeze_authority = reader.read_pubkey()?;
        }
        Ok(ParsedInstruction {
            name: "InitializeMint2".to_string(),
            fields: vec![
                ParsedField { name: "decimals".into(), value: IdlValue::U8(decimals) },
                ParsedField {
                    name: "has_freeze_authority".into(),
                    value: IdlValue::Bool(has_freeze_authority),
                },
            ]
            .into(),
            account_names: vec!["mint".to_string()],
        })
    })
}

/// Parses an AmountToUiAmount instruction: 23
fn parse_amount_to_ui_amount_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 8 || instruction.accounts.len() != 1 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let amount = reader.read_u64()?;
        Ok(ParsedInstruction {
            name: "AmountToUiAmount".to_string(),
            fields: vec![ParsedField { name: "amount".into(), value: IdlValue::U64(amount) }]
                .into(),
            account_names: vec!["mint".to_string()],
        })
    })
}

/// Parses a UiAmountToAmount instruction: 24
fn parse_ui_amount_to_amount_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 1 {
        return Ok(None); // Invalid number of accounts for UiAmountToAmount
    }

    // Decode the ASCII string
    let ui_amount = match std::str::from_utf8(data) {
        Ok(s) => s.to_string(),
        Err(_) => "invalid_utf8".to_string(),
    };

    Ok(Some(ParsedInstruction {
        name: "UiAmountToAmount".to_string(),
        fields: vec![ParsedField {
            name: "ui_amount".into(),
            value: IdlValue::String(ui_amount.into()),
        }]
        .into(),
        account_names: vec!["mint".to_string()],
    }))
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
        let close_authority_value =
            close_authority.map_or_else(|| "none".to_string(), |pk| pk.to_string());
        Ok(ParsedInstruction {
            name: "InitializeMintCloseAuthority".to_string(),
            fields: vec![ParsedField {
                name: "close_authority".into(),
                value: IdlValue::String(close_authority_value.into()),
            }]
            .into(),
            account_names: vec!["mint".to_string()],
        })
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
        unknown => Ok(Some(ParsedInstruction {
            name: format!("TransferFeeExtension({unknown})"),
            fields: vec![ParsedField {
                name: "raw_extension_data".into(),
                value: IdlValue::String(hex::encode(payload).into()),
            }]
            .into(),
            account_names: generate_generic_account_names(instruction.accounts.len()),
        })),
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
        Ok(ParsedInstruction {
            name: "InitializeTransferFeeConfig".to_string(),
            fields: vec![
                ParsedField {
                    name: "transfer_fee_config_authority".into(),
                    value: IdlValue::String(
                        config_authority
                            .map_or_else(|| "none".to_string(), |pk| pk.to_string())
                            .into(),
                    ),
                },
                ParsedField {
                    name: "withdraw_withheld_authority".into(),
                    value: IdlValue::String(
                        withdraw_authority
                            .map_or_else(|| "none".to_string(), |pk| pk.to_string())
                            .into(),
                    ),
                },
                ParsedField { name: "transfer_fee_basis_points".into(), value: IdlValue::U16(bps) },
                ParsedField { name: "maximum_fee".into(), value: IdlValue::U64(maximum_fee) },
            ]
            .into(),
            account_names: vec!["mint".to_string()],
        })
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

        let mut account_names =
            vec!["source".to_string(), "mint".to_string(), "destination".to_string()];
        if instruction.accounts.len() > 3 {
            account_names.push("authority".to_string());
        }
        append_additional_signer_accounts(&mut account_names, 4, instruction.accounts.len());

        Ok(ParsedInstruction {
            name: "TransferCheckedWithFee".to_string(),
            fields: vec![
                ParsedField { name: "amount".into(), value: IdlValue::U64(amount) },
                ParsedField { name: "decimals".into(), value: IdlValue::U8(decimals) },
                ParsedField { name: "fee".into(), value: IdlValue::U64(fee) },
            ]
            .into(),
            account_names,
        })
    })
}

fn parse_transfer_fee_withdraw_from_mint_instruction(
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 2 {
        return Ok(None);
    }

    let mut account_names = vec!["mint".to_string(), "destination".to_string()];
    if instruction.accounts.len() >= 3 {
        account_names.push("authority".to_string());
    }
    append_additional_signer_accounts(&mut account_names, 3, instruction.accounts.len());

    Ok(Some(ParsedInstruction {
        name: "WithdrawWithheldTokensFromMint".to_string(),
        fields: vec![].into(),
        account_names,
    }))
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

    let mut account_names =
        vec!["mint".to_string(), "destination".to_string(), "authority".to_string()];
    let signers_count = instruction.accounts.len().saturating_sub(3 + num_token_accounts);
    for i in 0..signers_count {
        account_names.push(format!("additional_signer_{}", i + 1));
    }
    for i in 0..num_token_accounts {
        account_names.push(format!("source_account_{}", i + 1));
    }

    Ok(Some(ParsedInstruction {
        name: "WithdrawWithheldTokensFromAccounts".to_string(),
        fields: vec![ParsedField {
            name: "num_token_accounts".into(),
            value: IdlValue::U32(num_token_accounts as u32),
        }]
        .into(),
        account_names,
    }))
}

fn parse_transfer_fee_harvest_to_mint_instruction(
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.is_empty() {
        return Ok(None);
    }

    let mut account_names = vec!["mint".to_string()];
    for i in 1..instruction.accounts.len() {
        account_names.push(format!("source_account_{}", i));
    }

    Ok(Some(ParsedInstruction {
        name: "HarvestWithheldTokensToMint".to_string(),
        fields: vec![].into(),
        account_names,
    }))
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

        let mut account_names = vec!["mint".to_string()];
        if instruction.accounts.len() >= 2 {
            account_names.push("authority".to_string());
        }
        append_additional_signer_accounts(&mut account_names, 2, instruction.accounts.len());

        Ok(ParsedInstruction {
            name: "SetTransferFee".to_string(),
            fields: vec![
                ParsedField {
                    name: "transfer_fee_basis_points".into(),
                    value: IdlValue::String(transfer_fee_basis_points.to_string().into()),
                },
                ParsedField { name: "maximum_fee".into(), value: IdlValue::U64(maximum_fee) },
            ]
            .into(),
            account_names,
        })
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
    let mut account_names: Vec<String> =
        named.iter().take(instruction.accounts.len()).map(|s| s.to_string()).collect();
    append_additional_signer_accounts(&mut account_names, 4, instruction.accounts.len());

    Ok(Some(ParsedInstruction {
        name: "Reallocate".to_string(),
        fields: vec![ParsedField {
            name: "extension_types".into(),
            value: IdlValue::String(
                if extension_types.is_empty() {
                    "none".to_string()
                } else {
                    extension_types.join(",")
                }
                .into(),
            ),
        }]
        .into(),
        account_names,
    }))
}

fn parse_create_native_mint_instruction(
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 3 {
        return Ok(None);
    }

    Ok(Some(ParsedInstruction {
        name: "CreateNativeMint".to_string(),
        fields: vec![].into(),
        account_names: vec![
            "funding_account".to_string(),
            "native_mint".to_string(),
            "system_program".to_string(),
        ],
    }))
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
        Ok(ParsedInstruction {
            name: "InitializePermanentDelegate".to_string(),
            fields: vec![ParsedField {
                name: "delegate".into(),
                value: IdlValue::String(delegate.into()),
            }]
            .into(),
            account_names: vec!["mint".to_string()],
        })
    })
}

fn parse_withdraw_excess_lamports_instruction(
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 3 {
        return Ok(None);
    }

    let mut account_names =
        vec!["source".to_string(), "destination".to_string(), "authority".to_string()];
    append_additional_signer_accounts(&mut account_names, 3, instruction.accounts.len());

    Ok(Some(ParsedInstruction {
        name: "WithdrawExcessLamports".to_string(),
        fields: vec![].into(),
        account_names,
    }))
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
            value: IdlValue::String(hex::encode(data).into()),
        });
    }

    Ok(Some(ParsedInstruction {
        name: name.to_string(),
        fields: fields.into(),
        account_names: generate_generic_account_names(instruction.accounts.len()),
    }))
}

fn generate_generic_account_names(len: usize) -> Vec<String> {
    (0..len).map(|i| format!("account_{}", i + 1)).collect()
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

        // Transfer instruction with 1-byte discriminator (3) + 8 bytes amount
        let mut data = vec![3]; // 1-byte discriminator for Transfer
        data.extend_from_slice(&1_000_000_u64.to_le_bytes()); // 8 bytes amount

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

        // Transfer instruction with wrong data length
        let mut data = vec![3]; // 1-byte discriminator for Transfer
        data.extend_from_slice(&[1, 2, 3, 4]); // Only 4 bytes instead of 8

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

        // InitializeMint instruction with 1-byte discriminator (0) + 1 byte decimals + authorities
        let mut data = vec![0]; // 1-byte discriminator
        data.push(9); // 1 byte decimals
        data.extend_from_slice(&[1u8; 32]); // mint authority pubkey
        data.push(0); // freeze authority: None

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

        // InitializeAccount instruction with 1-byte discriminator (1) + empty data
        let data = vec![1]; // 1-byte discriminator for InitializeAccount

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "InitializeAccount");
        assert_eq!(parsed.account_names.len(), 4);
        assert_eq!(parsed.account_names[0], "account");
        assert_eq!(parsed.account_names[1], "mint");
        assert_eq!(parsed.account_names[2], "owner");
        assert_eq!(parsed.account_names[3], "rent_sysvar");
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

        // InitializeMultisig instruction with 1-byte discriminator (2) + 1 byte m
        let mut data = vec![2]; // 1-byte discriminator for InitializeMultisig
        data.push(2); // m = 2 (number of required signatures)

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "InitializeMultisig");
        assert_eq!(parsed.account_names.len(), 4);
        assert_eq!(parsed.account_names[0], "multisig");
        assert_eq!(parsed.account_names[1], "signer_1");
        assert_eq!(parsed.account_names[2], "signer_2");
        assert_eq!(parsed.account_names[3], "rent_sysvar");

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

        // Approve instruction with 1-byte discriminator (4) + 8 bytes amount
        let mut data = vec![4]; // 1-byte discriminator for Approve
        data.extend_from_slice(&500_000_u64.to_le_bytes()); // 8 bytes amount

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "Approve");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "source");
        assert_eq!(parsed.account_names[1], "delegate");
        assert_eq!(parsed.account_names[2], "owner");

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

        // Revoke instruction with 1-byte discriminator (5) + empty data
        let data = vec![5]; // 1-byte discriminator for Revoke

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "Revoke");
        assert_eq!(parsed.account_names.len(), 2);
        assert_eq!(parsed.account_names[0], "source");
        assert_eq!(parsed.account_names[1], "owner");
    }

    #[test]
    fn test_set_authority_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", false, true),
            create_test_account(1, "AuthorityPubkey11111111111111111111111111111", true, false),
        ];

        // SetAuthority instruction with 1-byte discriminator (6) + authority_type
        let mut data = vec![6]; // 1-byte discriminator for SetAuthority
        data.push(0); // authority_type = MintTokens
        data.push(1); // new_authority_flag = present
        data.extend_from_slice(&[7u8; 32]); // new authority pubkey

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "SetAuthority");
        assert_eq!(parsed.account_names.len(), 2);
        assert_eq!(parsed.account_names[0], "account");
        assert_eq!(parsed.account_names[1], "authority");

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

        let data = vec![6, 0, 0]; // authority_type + COption::None
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

        // MintTo instruction with 1-byte discriminator (7) + 8 bytes amount
        let mut data = vec![7]; // 1-byte discriminator for MintTo
        data.extend_from_slice(&1_000_000_u64.to_le_bytes()); // 8 bytes amount

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "MintTo");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "mint");
        assert_eq!(parsed.account_names[1], "account");
        assert_eq!(parsed.account_names[2], "owner");

        assert!(
            parsed.fields.iter().any(|field| field.name == "amount" && field.value == "1000000")
        );
    }

    #[test]
    fn test_burn_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "MintPubkey111111111111111111111111111111111", false, false),
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", true, false),
        ];

        // Burn instruction with 1-byte discriminator (8) + 8 bytes amount
        let mut data = vec![8]; // 1-byte discriminator for Burn
        data.extend_from_slice(&250_000_u64.to_le_bytes()); // 8 bytes amount

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "Burn");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "account");
        assert_eq!(parsed.account_names[1], "mint");
        assert_eq!(parsed.account_names[2], "owner");

        assert!(
            parsed.fields.iter().any(|field| field.name == "amount" && field.value == "250000")
        );
    }

    #[test]
    fn test_close_account_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "DestinationPubkey11111111111111111111111111111", false, true),
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", true, false),
        ];

        // CloseAccount instruction with 1-byte discriminator (9) + empty data
        let data = vec![9]; // 1-byte discriminator for CloseAccount

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "CloseAccount");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "account");
        assert_eq!(parsed.account_names[1], "destination");
        assert_eq!(parsed.account_names[2], "owner");
    }

    #[test]
    fn test_freeze_account_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "MintPubkey111111111111111111111111111111111", false, false),
            create_test_account(2, "FreezeAuthorityPubkey111111111111111111111111", true, false),
        ];

        // FreezeAccount instruction with 1-byte discriminator (10) + empty data
        let data = vec![10]; // 1-byte discriminator for FreezeAccount

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "FreezeAccount");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "account");
        assert_eq!(parsed.account_names[1], "mint");
        assert_eq!(parsed.account_names[2], "freeze_authority");
    }

    #[test]
    fn test_thaw_account_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "MintPubkey111111111111111111111111111111111", false, false),
            create_test_account(2, "FreezeAuthorityPubkey111111111111111111111111", true, false),
        ];

        // ThawAccount instruction with 1-byte discriminator (11) + empty data
        let data = vec![11]; // 1-byte discriminator for ThawAccount

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "ThawAccount");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "account");
        assert_eq!(parsed.account_names[1], "mint");
        assert_eq!(parsed.account_names[2], "freeze_authority");
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

        // ApproveChecked instruction with 1-byte discriminator (13) + 8 bytes amount + 1 byte decimals
        let mut data = vec![13]; // 1-byte discriminator for ApproveChecked
        data.extend_from_slice(&300_000_u64.to_le_bytes()); // 8 bytes amount
        data.push(6); // 1 byte decimals

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "ApproveChecked");
        assert_eq!(parsed.account_names.len(), 4);
        assert_eq!(parsed.account_names[0], "source");
        assert_eq!(parsed.account_names[1], "mint");
        assert_eq!(parsed.account_names[2], "delegate");
        assert_eq!(parsed.account_names[3], "owner");

        assert!(
            parsed.fields.iter().any(|field| field.name == "amount" && field.value == "300000")
        );
        assert!(parsed.fields.iter().any(|field| field.name == "decimals" && field.value == "6"));
    }

    #[test]
    fn test_mint_to_checked_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "MintPubkey111111111111111111111111111111111", false, true),
            create_test_account(1, "AccountPubkey11111111111111111111111111111111", false, true),
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", true, false),
        ];

        // MintToChecked instruction with 1-byte discriminator (14) + 8 bytes amount + 1 byte decimals
        let mut data = vec![14]; // 1-byte discriminator for MintToChecked
        data.extend_from_slice(&2_000_000_u64.to_le_bytes()); // 8 bytes amount
        data.push(9); // 1 byte decimals

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "MintToChecked");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "mint");
        assert_eq!(parsed.account_names[1], "account");
        assert_eq!(parsed.account_names[2], "owner");

        assert!(
            parsed.fields.iter().any(|field| field.name == "amount" && field.value == "2000000")
        );
        assert!(parsed.fields.iter().any(|field| field.name == "decimals" && field.value == "9"));
    }

    #[test]
    fn test_burn_checked_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "MintPubkey111111111111111111111111111111111", false, false),
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", true, false),
        ];

        // BurnChecked instruction with 1-byte discriminator (15) + 8 bytes amount + 1 byte decimals
        let mut data = vec![15]; // 1-byte discriminator for BurnChecked
        data.extend_from_slice(&100_000_u64.to_le_bytes()); // 8 bytes amount
        data.push(6); // 1 byte decimals

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "BurnChecked");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "account");
        assert_eq!(parsed.account_names[1], "mint");
        assert_eq!(parsed.account_names[2], "owner");

        assert!(
            parsed.fields.iter().any(|field| field.name == "amount" && field.value == "100000")
        );
        assert!(parsed.fields.iter().any(|field| field.name == "decimals" && field.value == "6"));
    }

    #[test]
    fn test_initialize_account2_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", false, true),
            create_test_account(1, "MintPubkey111111111111111111111111111111111", false, false),
            create_test_account(2, "RentSysvar111111111111111111111111111111111", false, false),
        ];

        // InitializeAccount2 instruction with 1-byte discriminator (16) + 32 bytes owner pubkey
        let mut data = vec![16]; // 1-byte discriminator for InitializeAccount2
        data.extend_from_slice(&[0u8; 32]); // 32 bytes owner pubkey

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "InitializeAccount2");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "account");
        assert_eq!(parsed.account_names[1], "mint");
        assert_eq!(parsed.account_names[2], "rent_sysvar");

        // Check that owner field is present
        assert!(parsed.fields.iter().any(|field| field.name == "owner"));
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

        // SyncNative instruction with 1-byte discriminator (17) + empty data
        let data = vec![17]; // 1-byte discriminator for SyncNative

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "SyncNative");
        assert_eq!(parsed.account_names.len(), 1);
        assert_eq!(parsed.account_names[0], "account");
    }

    #[test]
    fn test_initialize_account3_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", false, true),
            create_test_account(1, "MintPubkey111111111111111111111111111111111", false, false),
        ];

        // InitializeAccount3 instruction with 1-byte discriminator (18) + 32 bytes owner pubkey
        let mut data = vec![18]; // 1-byte discriminator for InitializeAccount3
        data.extend_from_slice(&[0u8; 32]); // 32 bytes owner pubkey

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "InitializeAccount3");
        assert_eq!(parsed.account_names.len(), 2);
        assert_eq!(parsed.account_names[0], "account");
        assert_eq!(parsed.account_names[1], "mint");

        // Check that owner field is present
        assert!(parsed.fields.iter().any(|field| field.name == "owner"));
    }

    #[test]
    fn test_initialize_multisig2_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "MultisigPubkey1111111111111111111111111111111", false, true),
            create_test_account(1, "Signer1Pubkey11111111111111111111111111111111", true, false),
            create_test_account(2, "Signer2Pubkey11111111111111111111111111111111", true, false),
            create_test_account(3, "Signer3Pubkey11111111111111111111111111111111", true, false),
        ];

        // InitializeMultisig2 instruction with 1-byte discriminator (19) + 1 byte m
        let mut data = vec![19]; // 1-byte discriminator for InitializeMultisig2
        data.push(2); // m = 2 (number of required signatures)

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "InitializeMultisig2");
        assert_eq!(parsed.account_names.len(), 4);
        assert_eq!(parsed.account_names[0], "multisig");
        assert_eq!(parsed.account_names[1], "signer_1");
        assert_eq!(parsed.account_names[2], "signer_2");
        assert_eq!(parsed.account_names[3], "signer_3");

        assert!(parsed.fields.iter().any(|field| field.name == "m" && field.value == "2"));
    }

    #[test]
    fn test_initialize_mint2_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![create_test_account(
            0,
            "MintPubkey111111111111111111111111111111111",
            false,
            true,
        )];

        // InitializeMint2 instruction with 1-byte discriminator (20) + 1 byte decimals + authorities
        let mut data = vec![20]; // 1-byte discriminator
        data.push(6); // 1 byte decimals
        data.extend_from_slice(&[3u8; 32]); // mint authority pubkey
        data.push(0); // freeze authority: None

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "InitializeMint2");
        assert_eq!(parsed.account_names.len(), 1);
        assert_eq!(parsed.account_names[0], "mint");

        assert!(parsed.fields.iter().any(|field| field.name == "decimals" && field.value == "6"));
    }

    #[test]
    fn test_get_account_data_size_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![create_test_account(
            0,
            "MintPubkey111111111111111111111111111111111",
            false,
            false,
        )];

        // GetAccountDataSize instruction with 1-byte discriminator (21) + empty data
        let data = vec![21]; // 1-byte discriminator for GetAccountDataSize

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "GetAccountDataSize");
        assert_eq!(parsed.account_names.len(), 1);
        assert_eq!(parsed.account_names[0], "mint");
    }

    #[test]
    fn test_initialize_immutable_owner_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![create_test_account(
            0,
            "AccountPubkey11111111111111111111111111111111",
            false,
            true,
        )];

        // InitializeImmutableOwner instruction with 1-byte discriminator (22) + empty data
        let data = vec![22]; // 1-byte discriminator for InitializeImmutableOwner

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "InitializeImmutableOwner");
        assert_eq!(parsed.account_names.len(), 1);
        assert_eq!(parsed.account_names[0], "account");
    }

    #[test]
    fn test_amount_to_ui_amount_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![create_test_account(
            0,
            "MintPubkey111111111111111111111111111111111",
            false,
            false,
        )];

        // AmountToUiAmount instruction with 1-byte discriminator (23) + 8 bytes amount
        let mut data = vec![23]; // 1-byte discriminator for AmountToUiAmount
        data.extend_from_slice(&5_000_000_u64.to_le_bytes()); // 8 bytes amount

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "AmountToUiAmount");
        assert_eq!(parsed.account_names.len(), 1);
        assert_eq!(parsed.account_names[0], "mint");

        assert!(
            parsed.fields.iter().any(|field| field.name == "amount" && field.value == "5000000")
        );
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

        // UiAmountToAmount instruction with 1-byte discriminator (24) + UI amount string
        let mut data = vec![24]; // 1-byte discriminator for UiAmountToAmount
        data.extend_from_slice(b"100.50"); // UI amount as ASCII string

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "UiAmountToAmount");
        assert_eq!(parsed.account_names.len(), 1);
        assert_eq!(parsed.account_names[0], "mint");

        assert!(
            parsed.fields.iter().any(|field| field.name == "ui_amount" && field.value == "100.50")
        );
    }

    #[test]
    fn test_transfer_checked_instruction_with_multiple_signers() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "SourcePubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "MintPubkey111111111111111111111111111111111", false, false),
            create_test_account(2, "DestPubkey1111111111111111111111111111111111", false, true),
            create_test_account(3, "OwnerPubkey111111111111111111111111111111111", false, false), // owner is not a signer (likely a PDA)
            create_test_account(4, "Signer1Pubkey1111111111111111111111111111111", true, false), // additional signer
            create_test_account(5, "Signer2Pubkey1111111111111111111111111111111", true, false), // another additional signer
        ];

        let mut data = vec![12]; // 1-byte discriminator for TransferChecked
        data.extend_from_slice(&750_000_u64.to_le_bytes());
        data.push(9); // 1 byte decimals

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "TransferChecked");
        assert_eq!(parsed.account_names.len(), 6); // 4 base + 2 additional signers
        assert_eq!(parsed.account_names[0], "source");
        assert_eq!(parsed.account_names[1], "mint");
        assert_eq!(parsed.account_names[2], "destination");
        assert_eq!(parsed.account_names[3], "owner");
        assert_eq!(parsed.account_names[4], "additional_signer_1");
        assert_eq!(parsed.account_names[5], "additional_signer_2");

        assert!(
            parsed.fields.iter().any(|field| field.name == "amount" && field.value == "750000")
        );
        assert!(parsed.fields.iter().any(|field| field.name == "decimals" && field.value == "9"));
    }

    #[test]
    fn test_transfer_instruction_with_multiple_signers() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "SourcePubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "DestPubkey1111111111111111111111111111111111", false, true),
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", false, false),
            create_test_account(3, "Signer1Pubkey1111111111111111111111111111111", true, false),
            create_test_account(4, "Signer2Pubkey1111111111111111111111111111111", true, false),
        ];

        let mut data = vec![3];
        data.extend_from_slice(&9_999_u64.to_le_bytes());
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();

        assert_eq!(parsed.name, "Transfer");
        assert_eq!(
            parsed.account_names,
            vec!["source", "destination", "owner", "additional_signer_1", "additional_signer_2"]
        );
    }

    #[test]
    fn test_set_authority_instruction_with_multiple_signers() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", false, true),
            create_test_account(1, "OwnerPubkey111111111111111111111111111111111", false, false),
            create_test_account(2, "Signer1Pubkey1111111111111111111111111111111", true, false),
            create_test_account(3, "Signer2Pubkey1111111111111111111111111111111", true, false),
        ];

        let data = vec![6, 2, 0]; // authority_type=AccountOwner, clear authority
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();

        assert_eq!(parsed.name, "SetAuthority");
        assert_eq!(
            parsed.account_names,
            vec!["account", "authority", "additional_signer_1", "additional_signer_2"]
        );
    }

    #[test]
    fn test_close_account_instruction_with_multiple_signers() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", true, true),
            create_test_account(1, "DestinationPubkey11111111111111111111111111111", false, true),
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", false, false),
            create_test_account(3, "Signer1Pubkey1111111111111111111111111111111", true, false),
            create_test_account(4, "Signer2Pubkey1111111111111111111111111111111", true, false),
        ];

        let instruction = create_test_instruction(vec![9], accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();

        assert_eq!(parsed.name, "CloseAccount");
        assert_eq!(
            parsed.account_names,
            vec!["account", "destination", "owner", "additional_signer_1", "additional_signer_2"]
        );
    }

    #[test]
    fn test_mint_to_checked_instruction_with_multiple_signers() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "MintPubkey111111111111111111111111111111111", false, true),
            create_test_account(1, "AccountPubkey11111111111111111111111111111111", false, true),
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", false, false),
            create_test_account(3, "Signer1Pubkey1111111111111111111111111111111", true, false),
            create_test_account(4, "Signer2Pubkey1111111111111111111111111111111", true, false),
        ];

        let mut data = vec![14];
        data.extend_from_slice(&2_500_u64.to_le_bytes());
        data.push(6);
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();

        assert_eq!(parsed.name, "MintToChecked");
        assert_eq!(
            parsed.account_names,
            vec!["mint", "account", "owner", "additional_signer_1", "additional_signer_2"]
        );
    }

    #[test]
    fn test_program_id() {
        let parser = Token2022ProgramParser::new();
        let expected_id = Pubkey::from_str_const(TOKEN2022_PROGRAM_ID);
        assert_eq!(parser.program_id(), &expected_id);
    }

    #[test]
    fn test_custom_program_id() {
        let custom_program_id =
            Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
        let parser = Token2022ProgramParser::with_program_id(custom_program_id);

        assert_eq!(parser.program_id(), &custom_program_id);
    }

    #[test]
    fn test_initialize_mint_close_authority_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let mut data = vec![INITIALIZE_MINT_CLOSE_AUTHORITY_DISCRIMINATOR as u8];
        data.push(1); // Some
        data.extend_from_slice(&[2u8; 32]);

        let accounts = vec![create_test_account(
            0,
            "MintPubkey111111111111111111111111111111111",
            false,
            true,
        )];
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();

        assert_eq!(parsed.name, "InitializeMintCloseAuthority");
        assert_eq!(parsed.account_names, vec!["mint"]);
        assert!(parsed.fields.iter().any(|field| field.name == "close_authority"));
    }

    #[test]
    fn test_transfer_checked_with_fee_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let mut data = vec![TRANSFER_FEE_EXTENSION_DISCRIMINATOR as u8];
        data.push(1); // TransferCheckedWithFee
        data.extend_from_slice(&500_u64.to_le_bytes());
        data.push(6);
        data.extend_from_slice(&5_u64.to_le_bytes());

        let accounts = vec![
            create_test_account(0, "Source111111111111111111111111111111111", true, true),
            create_test_account(1, "Mint11111111111111111111111111111111111", false, false),
            create_test_account(2, "Dest11111111111111111111111111111111111", false, true),
            create_test_account(3, "Owner1111111111111111111111111111111111", true, false),
            create_test_account(4, "Signer111111111111111111111111111111111", true, false),
        ];

        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();

        assert_eq!(parsed.name, "TransferCheckedWithFee");
        assert_eq!(parsed.account_names[0], "source");
        assert_eq!(parsed.account_names[1], "mint");
        assert_eq!(parsed.account_names[2], "destination");
        assert_eq!(parsed.account_names[3], "authority");
        assert_eq!(parsed.account_names[4], "additional_signer_1");
        assert!(parsed.fields.iter().any(|field| field.name == "fee" && field.value == "5"));
    }

    #[test]
    fn test_reallocate_instruction_parsing() {
        let parser = Token2022ProgramParser::new();
        let mut data = vec![REALLOCATE_DISCRIMINATOR as u8];
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());

        let accounts = vec![
            create_test_account(0, "Account11111111111111111111111111111111", false, true),
            create_test_account(1, "Payer1111111111111111111111111111111111", true, true),
            create_test_account(2, "Sys111111111111111111111111111111111111", false, false),
            create_test_account(3, "Owner1111111111111111111111111111111111", true, false),
        ];

        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();

        assert_eq!(parsed.name, "Reallocate");
        assert_eq!(
            parsed.account_names,
            vec!["account", "payer", "system_program", "owner_or_delegate"]
        );
        assert!(
            parsed
                .fields
                .iter()
                .any(|field| field.name == "extension_types" && field.value == "1,2")
        );
    }

    #[test]
    fn test_withdraw_withheld_tokens_from_accounts_parsing() {
        let parser = Token2022ProgramParser::new();
        let mut data = vec![TRANSFER_FEE_EXTENSION_DISCRIMINATOR as u8];
        data.push(3); // WithdrawWithheldTokensFromAccounts
        data.push(2); // two token accounts

        let accounts = vec![
            create_test_account(0, "Mint11111111111111111111111111111111111", false, true),
            create_test_account(1, "Destination11111111111111111111111111111", false, true),
            create_test_account(2, "Authority111111111111111111111111111111", true, false),
            create_test_account(3, "Signer111111111111111111111111111111111", true, false),
            create_test_account(4, "Source111111111111111111111111111111111", false, true),
            create_test_account(5, "Source211111111111111111111111111111111", false, true),
        ];

        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();

        assert_eq!(parsed.name, "WithdrawWithheldTokensFromAccounts");
        assert_eq!(
            parsed.account_names,
            vec![
                "mint",
                "destination",
                "authority",
                "additional_signer_1",
                "source_account_1",
                "source_account_2"
            ]
        );
        assert!(
            parsed
                .fields
                .iter()
                .any(|field| field.name == "num_token_accounts" && field.value == "2")
        );
    }
}
