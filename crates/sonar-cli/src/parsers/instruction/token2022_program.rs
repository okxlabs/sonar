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
const CONFIDENTIAL_TRANSFER_EXTENSION_DISCRIMINATOR: u8 = 27;
const DEFAULT_ACCOUNT_STATE_EXTENSION_DISCRIMINATOR: u8 = 28;
const REALLOCATE_DISCRIMINATOR: u8 = 29;
const MEMO_TRANSFER_EXTENSION_DISCRIMINATOR: u8 = 30;
const CREATE_NATIVE_MINT_DISCRIMINATOR: u8 = 31;
const INITIALIZE_NON_TRANSFERABLE_MINT_DISCRIMINATOR: u8 = 32;
const INTEREST_BEARING_MINT_EXTENSION_DISCRIMINATOR: u8 = 33;
const CPI_GUARD_EXTENSION_DISCRIMINATOR: u8 = 34;
const INITIALIZE_PERMANENT_DELEGATE_DISCRIMINATOR: u8 = 35;
const TRANSFER_HOOK_EXTENSION_DISCRIMINATOR: u8 = 36;
const CONFIDENTIAL_TRANSFER_FEE_EXTENSION_DISCRIMINATOR: u8 = 37;
const METADATA_POINTER_EXTENSION_DISCRIMINATOR: u8 = 39;
const GROUP_POINTER_EXTENSION_DISCRIMINATOR: u8 = 40;
const GROUP_MEMBER_POINTER_EXTENSION_DISCRIMINATOR: u8 = 41;
const CONFIDENTIAL_MINT_BURN_EXTENSION_DISCRIMINATOR: u8 = 42;
const SCALED_UI_AMOUNT_EXTENSION_DISCRIMINATOR: u8 = 43;
const PAUSABLE_EXTENSION_DISCRIMINATOR: u8 = 44;

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

        match instruction_id {
            INITIALIZE_MINT_CLOSE_AUTHORITY_DISCRIMINATOR => {
                parse_initialize_mint_close_authority_instruction(data, instruction)
            }
            TRANSFER_FEE_EXTENSION_DISCRIMINATOR => {
                parse_transfer_fee_extension_instruction(data, instruction)
            }
            CONFIDENTIAL_TRANSFER_EXTENSION_DISCRIMINATOR => {
                parse_confidential_transfer_extension_instruction(data, instruction)
            }
            DEFAULT_ACCOUNT_STATE_EXTENSION_DISCRIMINATOR => {
                parse_default_account_state_extension_instruction(data, instruction)
            }
            REALLOCATE_DISCRIMINATOR => parse_reallocate_instruction(data, instruction),
            MEMO_TRANSFER_EXTENSION_DISCRIMINATOR => parse_toggle_extension_instruction(
                "MemoTransfer",
                "account",
                "owner",
                data,
                instruction,
            ),
            CREATE_NATIVE_MINT_DISCRIMINATOR => parse_create_native_mint_instruction(instruction),
            INITIALIZE_NON_TRANSFERABLE_MINT_DISCRIMINATOR => {
                parse_initialize_non_transferable_mint_instruction(instruction)
            }
            INTEREST_BEARING_MINT_EXTENSION_DISCRIMINATOR => {
                parse_interest_bearing_mint_extension_instruction(data, instruction)
            }
            CPI_GUARD_EXTENSION_DISCRIMINATOR => parse_toggle_extension_instruction(
                "CpiGuard",
                "account",
                "owner",
                data,
                instruction,
            ),
            INITIALIZE_PERMANENT_DELEGATE_DISCRIMINATOR => {
                parse_initialize_permanent_delegate_instruction(data, instruction)
            }
            TRANSFER_HOOK_EXTENSION_DISCRIMINATOR => parse_pointer_extension_instruction(
                "TransferHook",
                "program_id",
                data,
                instruction,
            ),
            CONFIDENTIAL_TRANSFER_FEE_EXTENSION_DISCRIMINATOR => {
                parse_confidential_extension_instruction(
                    "ConfidentialTransferFee",
                    CONFIDENTIAL_TRANSFER_FEE_INSTRUCTIONS,
                    data,
                    instruction,
                )
            }
            METADATA_POINTER_EXTENSION_DISCRIMINATOR => parse_pointer_extension_instruction(
                "MetadataPointer",
                "metadata_address",
                data,
                instruction,
            ),
            GROUP_POINTER_EXTENSION_DISCRIMINATOR => parse_pointer_extension_instruction(
                "GroupPointer",
                "group_address",
                data,
                instruction,
            ),
            GROUP_MEMBER_POINTER_EXTENSION_DISCRIMINATOR => parse_pointer_extension_instruction(
                "GroupMemberPointer",
                "member_address",
                data,
                instruction,
            ),
            CONFIDENTIAL_MINT_BURN_EXTENSION_DISCRIMINATOR => {
                parse_confidential_extension_instruction(
                    "ConfidentialMintBurn",
                    CONFIDENTIAL_MINT_BURN_INSTRUCTIONS,
                    data,
                    instruction,
                )
            }
            SCALED_UI_AMOUNT_EXTENSION_DISCRIMINATOR => {
                parse_scaled_ui_amount_extension_instruction(data, instruction)
            }
            PAUSABLE_EXTENSION_DISCRIMINATOR => {
                parse_pausable_extension_instruction(data, instruction)
            }
            WITHDRAW_EXCESS_LAMPORTS_DISCRIMINATOR => {
                spl_token_common::parse_withdraw_excess_lamports_instruction(instruction)
            }
            // The metadata- and group-interface instructions are dispatched by
            // an 8-byte discriminator (not the single leading byte), so try
            // them against the full instruction data as a fallback.
            _ => parse_interface_instruction(instruction),
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
            mint_authority_account_names("authority", instruction.accounts.len()),
        ))
    })
}

/// Sub-instruction names for `ConfidentialTransfer` (discriminator 27), indexed
/// by sub-tag. Full ZK payloads are not decoded; the raw payload is preserved.
static CONFIDENTIAL_TRANSFER_INSTRUCTIONS: &[&str] = &[
    "InitializeMint",
    "UpdateMint",
    "ConfigureAccount",
    "ApproveAccount",
    "EmptyAccount",
    "Deposit",
    "Withdraw",
    "Transfer",
    "ApplyPendingBalance",
    "EnableConfidentialCredits",
    "DisableConfidentialCredits",
    "EnableNonConfidentialCredits",
    "DisableNonConfidentialCredits",
    "TransferWithFee",
    "ConfigureAccountWithRegistry",
];

/// Sub-instruction names for `ConfidentialTransferFee` (discriminator 37).
static CONFIDENTIAL_TRANSFER_FEE_INSTRUCTIONS: &[&str] = &[
    "InitializeConfidentialTransferFeeConfig",
    "WithdrawWithheldTokensFromMint",
    "WithdrawWithheldTokensFromAccounts",
    "HarvestWithheldTokensToMint",
    "EnableHarvestToMint",
    "DisableHarvestToMint",
];

/// Sub-instruction names for `ConfidentialMintBurn` (discriminator 42).
static CONFIDENTIAL_MINT_BURN_INSTRUCTIONS: &[&str] = &[
    "InitializeMint",
    "RotateSupplyElGamalPubkey",
    "UpdateDecryptableSupply",
    "Mint",
    "Burn",
    "ApplyPendingBurn",
];

/// Confidential-* extensions carry large zero-knowledge payloads (ElGamal
/// ciphertexts and proofs) that aren't practical to fully decode here. We name
/// the specific sub-instruction — prefixed with its family so it can't be
/// confused with same-named native instructions (e.g. `InitializeMint`) — and
/// preserve the raw payload bytes.
fn parse_confidential_extension_instruction(
    family: &str,
    sub_instructions: &[&str],
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.is_empty() {
        return Ok(None);
    }
    let (sub_tag, payload) = data.split_first().unwrap();
    let name = match sub_instructions.get(*sub_tag as usize) {
        Some(name) => format!("{family}{name}"),
        None => format!("{family}Extension({sub_tag})"),
    };

    let mut fields = Vec::new();
    if !payload.is_empty() {
        fields.push(ParsedField {
            name: "raw_extension_data".into(),
            value: IdlValue::String(hex::encode(payload)),
        });
    }

    Ok(Some(parsed_instruction(
        &name,
        fields,
        generate_generic_account_names(instruction.accounts.len()),
    )))
}

/// `ConfidentialTransfer` extension (discriminator 27). Its `InitializeMint` and
/// `UpdateMint` sub-instructions carry plain configuration data (no ZK proofs),
/// so we decode them fully. The remaining proof-bearing sub-instructions fall
/// back to the family-prefixed name plus raw payload.
fn parse_confidential_transfer_extension_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.is_empty() {
        return Ok(None);
    }
    let (sub_tag, payload) = data.split_first().unwrap();
    match sub_tag {
        // InitializeMint { authority, auto_approve_new_accounts, auditor_elgamal_pubkey }
        0 => binary_reader::try_parse(payload, |reader| {
            let authority = reader.read_optional_non_zero_pubkey()?;
            let auto_approve = reader.read_u8()? != 0;
            let auditor = reader.read_optional_non_zero_bytes32()?;
            Ok(parsed_instruction(
                "InitializeConfidentialTransferMint",
                vec![
                    ParsedField {
                        name: "authority".into(),
                        value: IdlValue::String(option_pubkey_string(authority)),
                    },
                    ParsedField {
                        name: "auto_approve_new_accounts".into(),
                        value: IdlValue::Bool(auto_approve),
                    },
                    ParsedField {
                        name: "auditor_elgamal_pubkey".into(),
                        value: IdlValue::String(option_elgamal_string(auditor)),
                    },
                ],
                owned_account_names(&["mint"]),
            ))
        }),
        // UpdateMint { auto_approve_new_accounts, auditor_elgamal_pubkey }
        1 => binary_reader::try_parse(payload, |reader| {
            let auto_approve = reader.read_u8()? != 0;
            let auditor = reader.read_optional_non_zero_bytes32()?;
            Ok(parsed_instruction(
                "UpdateConfidentialTransferMint",
                vec![
                    ParsedField {
                        name: "auto_approve_new_accounts".into(),
                        value: IdlValue::Bool(auto_approve),
                    },
                    ParsedField {
                        name: "auditor_elgamal_pubkey".into(),
                        value: IdlValue::String(option_elgamal_string(auditor)),
                    },
                ],
                mint_authority_account_names("authority", instruction.accounts.len()),
            ))
        }),
        _ => parse_confidential_extension_instruction(
            "ConfidentialTransfer",
            CONFIDENTIAL_TRANSFER_INSTRUCTIONS,
            data,
            instruction,
        ),
    }
}

/// Format a 32-byte `OptionalNonZeroElGamalPubkey` as base64 (the canonical
/// ElGamal pubkey encoding), or `none` when absent.
fn option_elgamal_string(elgamal: Option<[u8; 32]>) -> String {
    use base64::Engine;
    elgamal.map_or_else(
        || "none".to_string(),
        |bytes| base64::engine::general_purpose::STANDARD.encode(bytes),
    )
}

/// Fallback label for an unrecognized extension sub-tag: name it
/// `<family>Extension(<sub_tag>)` and preserve the raw payload bytes.
fn unknown_extension_instruction(
    family: &str,
    sub_tag: u8,
    payload: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    let mut fields = Vec::new();
    if !payload.is_empty() {
        fields.push(ParsedField {
            name: "raw_extension_data".into(),
            value: IdlValue::String(hex::encode(payload)),
        });
    }
    Ok(Some(parsed_instruction(
        &format!("{family}Extension({sub_tag})"),
        fields,
        generate_generic_account_names(instruction.accounts.len()),
    )))
}

/// Account names for an extension "update" instruction: `[mint]` alone, or
/// `[mint, <authority>]` (plus any multisig signers) when an authority account
/// is present. `authority` names the second account (e.g. `authority`,
/// `rate_authority`).
fn mint_authority_account_names(authority: &str, total_accounts: usize) -> Vec<String> {
    let base: &[&str] = if total_accounts >= 2 { &["mint", authority] } else { &["mint"] };
    account_names_with_signers(base, total_accounts)
}

/// Pointer extensions (`MetadataPointer`, `GroupPointer`, `GroupMemberPointer`,
/// `TransferHook`) all share the same shape:
///   * `Initialize` (sub-tag 0): `authority` + `<target>`, both
///     `OptionalNonZeroPubkey`. Accounts: `[mint]`.
///   * `Update` (sub-tag 1): `<target>` (`OptionalNonZeroPubkey`). Accounts:
///     `[mint, authority, ..signers]`.
fn parse_pointer_extension_instruction(
    family: &str,
    target_field: &str,
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.is_empty() {
        return Ok(None);
    }
    let (sub_tag, payload) = data.split_first().unwrap();
    match sub_tag {
        0 => binary_reader::try_parse(payload, |reader| {
            let authority = reader.read_optional_non_zero_pubkey()?;
            let target = reader.read_optional_non_zero_pubkey()?;
            Ok(parsed_instruction(
                &format!("Initialize{family}"),
                vec![
                    ParsedField {
                        name: "authority".into(),
                        value: IdlValue::String(option_pubkey_string(authority)),
                    },
                    ParsedField {
                        name: target_field.into(),
                        value: IdlValue::String(option_pubkey_string(target)),
                    },
                ],
                owned_account_names(&["mint"]),
            ))
        }),
        1 => binary_reader::try_parse(payload, |reader| {
            let target = reader.read_optional_non_zero_pubkey()?;
            Ok(parsed_instruction(
                &format!("Update{family}"),
                vec![ParsedField {
                    name: target_field.into(),
                    value: IdlValue::String(option_pubkey_string(target)),
                }],
                mint_authority_account_names("authority", instruction.accounts.len()),
            ))
        }),
        unknown => unknown_extension_instruction(family, *unknown, payload, instruction),
    }
}

/// `DefaultAccountState` extension (discriminator 28). Sub-tag 0 = `Initialize`,
/// 1 = `Update`; both carry a single `AccountState` byte.
fn parse_default_account_state_extension_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.is_empty() {
        return Ok(None);
    }
    let (sub_tag, payload) = data.split_first().unwrap();
    let (name, base_account_names): (&str, &[&str]) = match sub_tag {
        0 => ("InitializeDefaultAccountState", &["mint"]),
        1 => ("UpdateDefaultAccountState", &["mint", "freeze_authority"]),
        unknown => {
            return unknown_extension_instruction(
                "DefaultAccountState",
                *unknown,
                payload,
                instruction,
            );
        }
    };
    binary_reader::try_parse(payload, |reader| {
        let state = reader.read_u8()?;
        Ok(parsed_instruction(
            name,
            vec![ParsedField {
                name: "account_state".into(),
                value: IdlValue::String(account_state_name(state)),
            }],
            account_names_with_signers(base_account_names, instruction.accounts.len()),
        ))
    })
}

fn account_state_name(state: u8) -> String {
    match state {
        0 => "Uninitialized".to_string(),
        1 => "Initialized".to_string(),
        2 => "Frozen".to_string(),
        other => other.to_string(),
    }
}

/// `InterestBearingMint` extension (discriminator 33). Sub-tag 0 = `Initialize`
/// (`rate_authority` + `rate`), 1 = `UpdateRate` (`rate`). The rate is signed
/// basis points (`i16`).
fn parse_interest_bearing_mint_extension_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.is_empty() {
        return Ok(None);
    }
    let (sub_tag, payload) = data.split_first().unwrap();
    match sub_tag {
        0 => binary_reader::try_parse(payload, |reader| {
            let rate_authority = reader.read_optional_non_zero_pubkey()?;
            let rate = reader.read_i16()?;
            Ok(parsed_instruction(
                "InitializeInterestBearingConfig",
                vec![
                    ParsedField {
                        name: "rate_authority".into(),
                        value: IdlValue::String(option_pubkey_string(rate_authority)),
                    },
                    ParsedField { name: "rate".into(), value: IdlValue::I16(rate) },
                ],
                owned_account_names(&["mint"]),
            ))
        }),
        1 => binary_reader::try_parse(payload, |reader| {
            let rate = reader.read_i16()?;
            Ok(parsed_instruction(
                "UpdateRateInterestBearingMint",
                vec![ParsedField { name: "rate".into(), value: IdlValue::I16(rate) }],
                mint_authority_account_names("rate_authority", instruction.accounts.len()),
            ))
        }),
        unknown => {
            unknown_extension_instruction("InterestBearingMint", *unknown, payload, instruction)
        }
    }
}

/// `Pausable` extension (discriminator 44). Sub-tag 0 = `Initialize`
/// (`authority`), 1 = `Pause`, 2 = `Resume`.
fn parse_pausable_extension_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.is_empty() {
        return Ok(None);
    }
    let (sub_tag, payload) = data.split_first().unwrap();
    match sub_tag {
        0 => binary_reader::try_parse(payload, |reader| {
            let authority = reader.read_pubkey_as_string()?;
            Ok(parsed_instruction(
                "InitializePausableConfig",
                vec![ParsedField { name: "authority".into(), value: IdlValue::String(authority) }],
                owned_account_names(&["mint"]),
            ))
        }),
        1 | 2 => {
            let name = if *sub_tag == 1 { "Pause" } else { "Resume" };
            Ok(Some(parsed_instruction(
                name,
                vec![],
                mint_authority_account_names("authority", instruction.accounts.len()),
            )))
        }
        unknown => unknown_extension_instruction("Pausable", *unknown, payload, instruction),
    }
}

/// Toggle-style account extensions (`MemoTransfer`, `CpiGuard`): sub-tag 0 =
/// `Enable`, 1 = `Disable`. Both operate on a token account and carry no data.
fn parse_toggle_extension_instruction(
    family: &str,
    primary_account: &str,
    authority_account: &str,
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.is_empty() || instruction.accounts.is_empty() {
        return Ok(None);
    }
    let action = match data[0] {
        0 => "Enable",
        1 => "Disable",
        unknown => {
            return Ok(Some(parsed_instruction(
                &format!("{family}Extension({unknown})"),
                vec![],
                generate_generic_account_names(instruction.accounts.len()),
            )));
        }
    };
    let base_account_names: &[&str] = if instruction.accounts.len() >= 2 {
        &[primary_account, authority_account]
    } else {
        std::slice::from_ref(&primary_account)
    };
    Ok(Some(parsed_instruction(
        &format!("{action}{family}"),
        vec![],
        account_names_with_signers(base_account_names, instruction.accounts.len()),
    )))
}

/// Decode the metadata-interface and group-interface instructions embedded in
/// the Token-2022 program. These are dispatched by an 8-byte discriminator over
/// the full instruction data rather than the single leading byte used by the
/// native instructions.
fn parse_interface_instruction(
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if let Some(parsed) = parse_token_metadata_interface_instruction(instruction) {
        return Ok(Some(parsed));
    }
    if let Some(parsed) = parse_token_group_interface_instruction(instruction) {
        return Ok(Some(parsed));
    }
    Ok(None)
}

fn parse_token_metadata_interface_instruction(
    instruction: &InstructionSummary,
) -> Option<ParsedInstruction> {
    use spl_token_metadata_interface::instruction::TokenMetadataInstruction;

    let decoded = TokenMetadataInstruction::unpack(&instruction.data).ok()?;
    let (name, fields, base_account_names): (&str, Vec<ParsedField>, &[&str]) = match decoded {
        TokenMetadataInstruction::Initialize(data) => (
            "InitializeTokenMetadata",
            vec![
                ParsedField { name: "name".into(), value: IdlValue::String(data.name) },
                ParsedField { name: "symbol".into(), value: IdlValue::String(data.symbol) },
                ParsedField { name: "uri".into(), value: IdlValue::String(data.uri) },
            ],
            &["metadata", "update_authority", "mint", "mint_authority"],
        ),
        TokenMetadataInstruction::UpdateField(data) => (
            "UpdateTokenMetadataField",
            vec![
                ParsedField {
                    name: "field".into(),
                    value: IdlValue::String(metadata_field_name(&data.field)),
                },
                ParsedField { name: "value".into(), value: IdlValue::String(data.value) },
            ],
            &["metadata", "update_authority"],
        ),
        TokenMetadataInstruction::RemoveKey(data) => (
            "RemoveTokenMetadataKey",
            vec![
                ParsedField { name: "idempotent".into(), value: IdlValue::Bool(data.idempotent) },
                ParsedField { name: "key".into(), value: IdlValue::String(data.key) },
            ],
            &["metadata", "update_authority"],
        ),
        TokenMetadataInstruction::UpdateAuthority(data) => (
            "UpdateTokenMetadataAuthority",
            vec![ParsedField {
                name: "new_authority".into(),
                value: IdlValue::String(option_pubkey_string(Option::<Pubkey>::from(
                    data.new_authority,
                ))),
            }],
            &["metadata", "update_authority"],
        ),
        TokenMetadataInstruction::Emit(data) => (
            "EmitTokenMetadata",
            vec![
                ParsedField {
                    name: "start".into(),
                    value: data.start.map_or(IdlValue::Null, IdlValue::U64),
                },
                ParsedField {
                    name: "end".into(),
                    value: data.end.map_or(IdlValue::Null, IdlValue::U64),
                },
            ],
            &["metadata"],
        ),
    };

    Some(parsed_instruction(
        name,
        fields,
        account_names_with_signers(base_account_names, instruction.accounts.len()),
    ))
}

fn metadata_field_name(field: &spl_token_metadata_interface::state::Field) -> String {
    use spl_token_metadata_interface::state::Field;
    match field {
        Field::Name => "Name".to_string(),
        Field::Symbol => "Symbol".to_string(),
        Field::Uri => "Uri".to_string(),
        Field::Key(key) => format!("Key({key})"),
    }
}

fn parse_token_group_interface_instruction(
    instruction: &InstructionSummary,
) -> Option<ParsedInstruction> {
    use spl_token_group_interface::instruction::TokenGroupInstruction;

    let decoded = TokenGroupInstruction::unpack(&instruction.data).ok()?;
    let (name, fields, base_account_names): (&str, Vec<ParsedField>, &[&str]) = match decoded {
        TokenGroupInstruction::InitializeGroup(data) => (
            "InitializeTokenGroup",
            vec![
                ParsedField {
                    name: "update_authority".into(),
                    value: IdlValue::String(option_pubkey_string(Option::<Pubkey>::from(
                        data.update_authority,
                    ))),
                },
                ParsedField {
                    name: "max_size".into(),
                    value: IdlValue::U64(u64::from(data.max_size)),
                },
            ],
            &["group", "mint", "mint_authority"],
        ),
        TokenGroupInstruction::UpdateGroupMaxSize(data) => (
            "UpdateTokenGroupMaxSize",
            vec![ParsedField {
                name: "max_size".into(),
                value: IdlValue::U64(u64::from(data.max_size)),
            }],
            &["group", "update_authority"],
        ),
        TokenGroupInstruction::UpdateGroupAuthority(data) => (
            "UpdateTokenGroupAuthority",
            vec![ParsedField {
                name: "new_authority".into(),
                value: IdlValue::String(option_pubkey_string(Option::<Pubkey>::from(
                    data.new_authority,
                ))),
            }],
            &["group", "update_authority"],
        ),
        TokenGroupInstruction::InitializeMember(_) => (
            "InitializeTokenGroupMember",
            vec![],
            &["member", "member_mint", "member_mint_authority", "group", "group_update_authority"],
        ),
    };

    Some(parsed_instruction(
        name,
        fields,
        account_names_with_signers(base_account_names, instruction.accounts.len()),
    ))
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
    fn test_confidential_transfer_extension_names_sub_instruction() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![create_test_account(
            0,
            "AccountPubkey11111111111111111111111111111111",
            false,
            true,
        )];
        // discriminator 27, sub-tag 5 (Deposit), then raw payload. Proof-bearing
        // sub-instructions keep a family-prefixed name + raw payload.
        let data = vec![27, 5, 0xAB, 0xCD];
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "ConfidentialTransferDeposit");
        assert!(
            parsed
                .fields
                .iter()
                .any(|field| field.name == "raw_extension_data" && field.value == "abcd")
        );
    }

    #[test]
    fn test_confidential_transfer_initialize_mint_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts =
            vec![create_test_account(0, "MintPubkey11111111111111111111111111111111111", false, true)];
        let authority = Pubkey::new_unique();
        // discriminator 27, sub-tag 0 (InitializeMint): authority (32),
        // auto_approve_new_accounts (1 byte), auditor_elgamal_pubkey (32, zero = none).
        let mut data = vec![27, 0];
        data.extend_from_slice(authority.as_ref());
        data.push(1); // auto_approve_new_accounts = true
        data.extend_from_slice(&[0u8; 32]); // auditor = none
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        // Action-first name, distinct from the base InitializeMint instruction.
        assert_eq!(parsed.name, "InitializeConfidentialTransferMint");
        assert_eq!(parsed.account_names, vec!["mint"]);
        assert!(parsed.fields.iter().any(|f| f.name == "authority" && f.value == authority.to_string()));
        assert!(
            parsed.fields.iter().any(|f| f.name == "auto_approve_new_accounts" && f.value == "true")
        );
        assert!(
            parsed.fields.iter().any(|f| f.name == "auditor_elgamal_pubkey" && f.value == "none")
        );
        assert!(!parsed.fields.iter().any(|f| f.name == "raw_extension_data"));
    }

    #[test]
    fn test_metadata_pointer_initialize_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts =
            vec![create_test_account(0, "MintPubkey11111111111111111111111111111111111", false, true)];
        let authority = Pubkey::new_unique();
        let metadata = Pubkey::new_unique();
        // discriminator 39, sub-tag 0 (Initialize), authority, metadata_address.
        let mut data = vec![39, 0];
        data.extend_from_slice(authority.as_ref());
        data.extend_from_slice(metadata.as_ref());
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "InitializeMetadataPointer");
        assert_eq!(parsed.account_names, vec!["mint"]);
        assert!(parsed.fields.iter().any(|f| f.name == "authority" && f.value == authority.to_string()));
        assert!(
            parsed.fields.iter().any(|f| f.name == "metadata_address" && f.value == metadata.to_string())
        );
    }

    #[test]
    fn test_transfer_hook_update_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "MintPubkey11111111111111111111111111111111111", false, true),
            create_test_account(1, "AuthorityPubkey11111111111111111111111111111", true, false),
        ];
        // discriminator 36, sub-tag 1 (Update), all-zero program_id (None).
        let mut data = vec![36, 1];
        data.extend_from_slice(&[0u8; 32]);
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "UpdateTransferHook");
        assert_eq!(parsed.account_names, vec!["mint", "authority"]);
        assert!(parsed.fields.iter().any(|f| f.name == "program_id" && f.value == "none"));
    }

    #[test]
    fn test_default_account_state_initialize_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts =
            vec![create_test_account(0, "MintPubkey11111111111111111111111111111111111", false, true)];
        // discriminator 28, sub-tag 0 (Initialize), state 2 (Frozen).
        let data = vec![28, 0, 2];
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "InitializeDefaultAccountState");
        assert!(parsed.fields.iter().any(|f| f.name == "account_state" && f.value == "Frozen"));
    }

    #[test]
    fn test_interest_bearing_initialize_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts =
            vec![create_test_account(0, "MintPubkey11111111111111111111111111111111111", false, true)];
        // discriminator 33, sub-tag 0 (Initialize), all-zero authority, rate -500.
        let mut data = vec![33, 0];
        data.extend_from_slice(&[0u8; 32]);
        data.extend_from_slice(&(-500_i16).to_le_bytes());
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "InitializeInterestBearingConfig");
        assert!(parsed.fields.iter().any(|f| f.name == "rate_authority" && f.value == "none"));
        assert!(parsed.fields.iter().any(|f| f.name == "rate" && f.value == "-500"));
    }

    #[test]
    fn test_pausable_initialize_and_pause_parsing() {
        let parser = Token2022ProgramParser::new();
        let mint = "MintPubkey11111111111111111111111111111111111";
        let authority = Pubkey::new_unique();
        // Initialize (sub-tag 0) carries a 32-byte authority.
        let mut init_data = vec![44, 0];
        init_data.extend_from_slice(authority.as_ref());
        let init = create_test_instruction(init_data, vec![create_test_account(0, mint, false, true)]);
        let parsed = parser.parse_instruction(&init).unwrap().unwrap();
        assert_eq!(parsed.name, "InitializePausableConfig");
        assert!(parsed.fields.iter().any(|f| f.name == "authority" && f.value == authority.to_string()));

        // Pause (sub-tag 1) carries no data, accounts [mint, authority].
        let pause = create_test_instruction(
            vec![44, 1],
            vec![
                create_test_account(0, mint, false, true),
                create_test_account(1, "AuthorityPubkey11111111111111111111111111111", true, false),
            ],
        );
        let parsed = parser.parse_instruction(&pause).unwrap().unwrap();
        assert_eq!(parsed.name, "Pause");
        assert_eq!(parsed.account_names, vec!["mint", "authority"]);
    }

    #[test]
    fn test_memo_transfer_enable_parsing() {
        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", false, true),
            create_test_account(1, "OwnerPubkey111111111111111111111111111111111", true, false),
        ];
        // discriminator 30, sub-tag 0 (Enable).
        let instruction = create_test_instruction(vec![30, 0], accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "EnableMemoTransfer");
        assert_eq!(parsed.account_names, vec!["account", "owner"]);
    }

    #[test]
    fn test_token_metadata_interface_initialize_parsing() {
        use spl_token_metadata_interface::instruction::{Initialize, TokenMetadataInstruction};

        let parser = Token2022ProgramParser::new();
        let accounts = vec![
            create_test_account(0, "MetadataPubkey11111111111111111111111111111", false, true),
            create_test_account(1, "AuthorityPubkey11111111111111111111111111111", true, false),
            create_test_account(2, "MintPubkey11111111111111111111111111111111111", false, true),
            create_test_account(3, "MintAuthPubkey11111111111111111111111111111", true, false),
        ];
        let data = TokenMetadataInstruction::Initialize(Initialize {
            name: "My Token".to_string(),
            symbol: "MTK".to_string(),
            uri: "https://example.com".to_string(),
        })
        .pack();
        let instruction = create_test_instruction(data, accounts);
        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "InitializeTokenMetadata");
        assert_eq!(parsed.account_names, vec!["metadata", "update_authority", "mint", "mint_authority"]);
        assert!(parsed.fields.iter().any(|f| f.name == "name" && f.value == "My Token"));
        assert!(parsed.fields.iter().any(|f| f.name == "symbol" && f.value == "MTK"));
        assert!(parsed.fields.iter().any(|f| f.name == "uri" && f.value == "https://example.com"));
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
