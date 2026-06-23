//! Shared SPL Token instruction parsing used by both Legacy Token and Token-2022.
//!
//! Token-2022 is a superset of Legacy Token: every Legacy instruction is also
//! valid under Token-2022. This module owns that shared set — the base
//! instructions (discriminators 0–24, 38) plus the Pinocchio extensions
//! UnwrapLamports (45) and Batch (255) — so both flavors decode them
//! identically through [`parse_base_instruction`]. Token-2022 adds its own
//! extension instructions on top in `token2022_program`.

use anyhow::Result;
use sonar_idl::IdlValue;

use super::fixed_layout::{self, AccountRule, FieldDef, FieldType, InstructionDef};
use super::{ParsedField, ParsedInstruction};
use crate::core::transaction::InstructionSummary;
use crate::parsers::binary_reader;

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
pub(super) const WITHDRAW_EXCESS_LAMPORTS_DISCRIMINATOR: u8 = 38;
const UNWRAP_LAMPORTS_DISCRIMINATOR: u8 = 45;
const BATCH_DISCRIMINATOR: u8 = 255;

/// The fixed-layout subset of the base SPL Token instruction set, shared by
/// Legacy Token and Token-2022. Variable-length instructions (InitializeMint,
/// SetAuthority, UiAmountToAmount, multisig, the Pinocchio extensions, …) are
/// handled by the bespoke match in [`parse_base_instruction`].
///
/// Discriminators are the single leading byte. The `*Checked` instructions
/// carry a trailing `decimals` byte; the rest are either a bare `amount` or
/// have no data. All but `SyncNative`/`GetAccountDataSize`/`InitializeImmutableOwner`
/// (single-account) allow trailing multisig signer accounts.
static TOKEN_BASE: &[InstructionDef] = &[
    InstructionDef {
        discriminator: &[3],
        name: "Transfer",
        fields: &[FieldDef { name: "amount", ty: FieldType::U64 }],
        account_names: &["source", "destination", "owner"],
        account_rule: AccountRule::MinWithSigners,
    },
    InstructionDef {
        discriminator: &[4],
        name: "Approve",
        fields: &[FieldDef { name: "amount", ty: FieldType::U64 }],
        account_names: &["source", "delegate", "owner"],
        account_rule: AccountRule::MinWithSigners,
    },
    InstructionDef {
        discriminator: &[7],
        name: "MintTo",
        fields: &[FieldDef { name: "amount", ty: FieldType::U64 }],
        account_names: &["mint", "account", "owner"],
        account_rule: AccountRule::MinWithSigners,
    },
    InstructionDef {
        discriminator: &[8],
        name: "Burn",
        fields: &[FieldDef { name: "amount", ty: FieldType::U64 }],
        account_names: &["account", "mint", "owner"],
        account_rule: AccountRule::MinWithSigners,
    },
    InstructionDef {
        discriminator: &[12],
        name: "TransferChecked",
        fields: &[
            FieldDef { name: "amount", ty: FieldType::U64 },
            FieldDef { name: "decimals", ty: FieldType::U8 },
        ],
        account_names: &["source", "mint", "destination", "owner"],
        account_rule: AccountRule::MinWithSigners,
    },
    InstructionDef {
        discriminator: &[13],
        name: "ApproveChecked",
        fields: &[
            FieldDef { name: "amount", ty: FieldType::U64 },
            FieldDef { name: "decimals", ty: FieldType::U8 },
        ],
        account_names: &["source", "mint", "delegate", "owner"],
        account_rule: AccountRule::MinWithSigners,
    },
    InstructionDef {
        discriminator: &[14],
        name: "MintToChecked",
        fields: &[
            FieldDef { name: "amount", ty: FieldType::U64 },
            FieldDef { name: "decimals", ty: FieldType::U8 },
        ],
        account_names: &["mint", "account", "owner"],
        account_rule: AccountRule::MinWithSigners,
    },
    InstructionDef {
        discriminator: &[15],
        name: "BurnChecked",
        fields: &[
            FieldDef { name: "amount", ty: FieldType::U64 },
            FieldDef { name: "decimals", ty: FieldType::U8 },
        ],
        account_names: &["account", "mint", "owner"],
        account_rule: AccountRule::MinWithSigners,
    },
    InstructionDef {
        discriminator: &[5],
        name: "Revoke",
        fields: &[],
        account_names: &["source", "owner"],
        account_rule: AccountRule::MinWithSigners,
    },
    InstructionDef {
        discriminator: &[9],
        name: "CloseAccount",
        fields: &[],
        account_names: &["account", "destination", "owner"],
        account_rule: AccountRule::MinWithSigners,
    },
    InstructionDef {
        discriminator: &[10],
        name: "FreezeAccount",
        fields: &[],
        account_names: &["account", "mint", "freeze_authority"],
        account_rule: AccountRule::MinWithSigners,
    },
    InstructionDef {
        discriminator: &[11],
        name: "ThawAccount",
        fields: &[],
        account_names: &["account", "mint", "freeze_authority"],
        account_rule: AccountRule::MinWithSigners,
    },
    InstructionDef {
        discriminator: &[17],
        name: "SyncNative",
        fields: &[],
        account_names: &["account"],
        account_rule: AccountRule::Exact,
    },
    InstructionDef {
        discriminator: &[21],
        name: "GetAccountDataSize",
        fields: &[],
        account_names: &["mint"],
        account_rule: AccountRule::Exact,
    },
    InstructionDef {
        discriminator: &[22],
        name: "InitializeImmutableOwner",
        fields: &[],
        account_names: &["account"],
        account_rule: AccountRule::Exact,
    },
];

/// Parse a discriminator that's part of the shared SPL Token instruction set.
/// Returns `Ok(None)` if the discriminator is not shared.
pub(super) fn parse_base_instruction(
    instruction_id: u8,
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if let Some(parsed) = fixed_layout::parse(TOKEN_BASE, instruction)? {
        return Ok(Some(parsed));
    }

    match instruction_id {
        INITIALIZE_MINT_DISCRIMINATOR => parse_initialize_mint_instruction(data),
        INITIALIZE_ACCOUNT_DISCRIMINATOR => parse_initialize_account_instruction(instruction),
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
        INITIALIZE_MINT2_DISCRIMINATOR => parse_initialize_mint2_instruction(data),
        AMOUNT_TO_UI_AMOUNT_DISCRIMINATOR => {
            parse_amount_to_ui_amount_instruction(data, instruction)
        }
        UI_AMOUNT_TO_AMOUNT_DISCRIMINATOR => {
            parse_ui_amount_to_amount_instruction(data, instruction)
        }
        WITHDRAW_EXCESS_LAMPORTS_DISCRIMINATOR => {
            parse_withdraw_excess_lamports_instruction(instruction)
        }
        UNWRAP_LAMPORTS_DISCRIMINATOR => parse_unwrap_lamports_instruction(data, instruction),
        BATCH_DISCRIMINATOR => parse_batch_instruction(data, instruction),
        _ => Ok(None),
    }
}

// ── Shared per-instruction parsers ──

fn parse_initialize_mint_instruction(data: &[u8]) -> Result<Option<ParsedInstruction>> {
    parse_initialize_mint_variant(data, "InitializeMint", &["mint", "rent_sysvar"])
}

fn parse_initialize_account_instruction(
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 4 {
        return Ok(None);
    }
    Ok(Some(parsed_instruction(
        "InitializeAccount",
        vec![],
        owned_account_names(&["account", "mint", "owner", "rent_sysvar"]),
    )))
}

fn parse_initialize_multisig_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    parse_initialize_multisig_variant(data, instruction, "InitializeMultisig", &["rent_sysvar"])
}

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
        Ok(parsed_instruction(
            "SetAuthority",
            vec![
                ParsedField {
                    name: "authority_type".into(),
                    value: IdlValue::String(authority_type.to_string()),
                },
                ParsedField { name: "cleared".into(), value: IdlValue::Bool(cleared) },
            ],
            account_names_with_signers(&["account", "authority"], instruction.accounts.len()),
        ))
    })
}

fn parse_initialize_account2_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 32 || instruction.accounts.len() != 3 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let owner_pubkey = reader.read_pubkey_as_string()?;
        Ok(parsed_instruction(
            "InitializeAccount2",
            vec![ParsedField { name: "owner".into(), value: IdlValue::String(owner_pubkey) }],
            owned_account_names(&["account", "mint", "rent_sysvar"]),
        ))
    })
}

fn parse_initialize_account3_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 32 || instruction.accounts.len() < 2 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let owner_pubkey = reader.read_pubkey_as_string()?;
        let mut account_names = owned_account_names(&["account", "mint"]);
        extend_numbered_account_names(
            &mut account_names,
            "account_",
            3,
            instruction.accounts.len() - 2,
        );
        Ok(parsed_instruction(
            "InitializeAccount3",
            vec![ParsedField { name: "owner".into(), value: IdlValue::String(owner_pubkey) }],
            account_names,
        ))
    })
}

fn parse_initialize_multisig2_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    parse_initialize_multisig_variant(data, instruction, "InitializeMultisig2", &[])
}

fn parse_initialize_mint2_instruction(data: &[u8]) -> Result<Option<ParsedInstruction>> {
    parse_initialize_mint_variant(data, "InitializeMint2", &["mint"])
}

fn parse_amount_to_ui_amount_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 8 || instruction.accounts.len() != 1 {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let amount = reader.read_u64()?;
        Ok(parsed_instruction(
            "AmountToUiAmount",
            vec![ParsedField { name: "amount".into(), value: IdlValue::U64(amount) }],
            owned_account_names(&["mint"]),
        ))
    })
}

fn parse_ui_amount_to_amount_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 1 {
        return Ok(None);
    }
    let ui_amount = match std::str::from_utf8(data) {
        Ok(s) => s.to_string(),
        Err(_) => "invalid_utf8".to_string(),
    };
    Ok(Some(parsed_instruction(
        "UiAmountToAmount",
        vec![ParsedField { name: "ui_amount".into(), value: IdlValue::String(ui_amount) }],
        owned_account_names(&["mint"]),
    )))
}

pub(super) fn parse_withdraw_excess_lamports_instruction(
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 3 {
        return Ok(None);
    }
    Ok(Some(parsed_instruction(
        "WithdrawExcessLamports",
        vec![],
        account_names_with_signers(
            &["source", "destination", "authority"],
            instruction.accounts.len(),
        ),
    )))
}

/// Pinocchio SPL Token extension: UnwrapLamports (discriminator 45).
///
/// Body is a 1-byte amount discriminant followed by either nothing
/// (amount = "all") or 8 bytes (amount = u64). Anything else is rejected.
fn parse_unwrap_lamports_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 3 {
        return Ok(None);
    }

    let amount = match data {
        [0] => IdlValue::String("all".to_string()),
        [1, rest @ ..] if rest.len() == 8 => {
            IdlValue::U64(u64::from_le_bytes(rest.try_into().unwrap()))
        }
        _ => return Ok(None),
    };

    Ok(Some(parsed_instruction(
        "UnwrapLamports",
        vec![ParsedField { name: "amount".into(), value: amount }],
        account_names_with_signers(
            &["source", "destination", "authority"],
            instruction.accounts.len(),
        ),
    )))
}

/// Pinocchio SPL Token extension: Batch (discriminator 255).
///
/// Body is a sequence of `(account_count: u8, data_len: u8, data: [u8; data_len])`
/// tuples. We validate the framing and emit structured sub-instructions so
/// callers don't need to re-parse the raw bytes.
fn parse_batch_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    let mut offset = 0usize;
    let mut sub_instructions: Vec<IdlValue> = Vec::new();
    let mut account_count = 0usize;

    while offset < data.len() {
        if offset + 2 > data.len() {
            return Ok(None);
        }

        let instruction_account_count = data[offset] as usize;
        let instruction_data_len = data[offset + 1] as usize;
        let data_start = offset + 2;
        let data_end = data_start + instruction_data_len;

        if data_end > data.len() {
            return Ok(None);
        }

        sub_instructions.push(IdlValue::Struct(vec![
            ("account_count".to_string(), IdlValue::U8(instruction_account_count as u8)),
            ("data".to_string(), IdlValue::String(hex::encode(&data[data_start..data_end]))),
        ]));
        account_count = account_count.saturating_add(instruction_account_count);
        offset = data_end;
    }

    if account_count != instruction.accounts.len() {
        return Ok(None);
    }

    let instruction_count = sub_instructions.len() as u32;

    Ok(Some(parsed_instruction(
        "Batch",
        vec![
            ParsedField {
                name: "instruction_count".into(),
                value: IdlValue::U32(instruction_count),
            },
            ParsedField {
                name: "account_count".into(),
                value: IdlValue::U32(account_count as u32),
            },
            ParsedField { name: "instructions".into(), value: IdlValue::Array(sub_instructions) },
        ],
        generate_generic_account_names(instruction.accounts.len()),
    )))
}

// ── Variant helpers used by sibling parsers ──

fn parse_initialize_mint_variant(
    data: &[u8],
    name: &str,
    account_names: &[&str],
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
        Ok(parsed_instruction(
            name,
            vec![
                ParsedField { name: "decimals".into(), value: IdlValue::U8(decimals) },
                ParsedField {
                    name: "has_freeze_authority".into(),
                    value: IdlValue::Bool(has_freeze_authority),
                },
            ],
            owned_account_names(account_names),
        ))
    })
}

fn parse_initialize_multisig_variant(
    data: &[u8],
    instruction: &InstructionSummary,
    name: &str,
    trailing_accounts: &[&str],
) -> Result<Option<ParsedInstruction>> {
    let min_accounts = 2 + trailing_accounts.len();
    if data.len() != 1 || instruction.accounts.len() < min_accounts {
        return Ok(None);
    }
    binary_reader::try_parse(data, |reader| {
        let m = reader.read_u8()?;
        let num_signers = instruction.accounts.len().saturating_sub(1 + trailing_accounts.len());
        let mut account_names = owned_account_names(&["multisig"]);
        extend_numbered_account_names(&mut account_names, "signer_", 1, num_signers);
        account_names.extend(owned_account_names(trailing_accounts));
        Ok(parsed_instruction(
            name,
            vec![ParsedField { name: "m".into(), value: IdlValue::U8(m) }],
            account_names,
        ))
    })
}

// ── Shared formatting helpers ──

pub(super) fn parsed_instruction(
    name: &str,
    fields: Vec<ParsedField>,
    account_names: Vec<String>,
) -> ParsedInstruction {
    ParsedInstruction { name: name.to_string(), fields: fields.into(), account_names }
}

pub(super) fn owned_account_names(account_names: &[&str]) -> Vec<String> {
    account_names.iter().map(|name| (*name).to_string()).collect()
}

pub(super) fn account_names_with_signers(
    account_names: &[&str],
    total_accounts: usize,
) -> Vec<String> {
    let mut account_names = owned_account_names(account_names);
    let base_accounts = account_names.len();
    super::append_extra_account_names(
        &mut account_names,
        total_accounts,
        base_accounts,
        "additional_signer_",
    );
    account_names
}

pub(super) fn extend_numbered_account_names(
    account_names: &mut Vec<String>,
    prefix: &str,
    start: usize,
    count: usize,
) {
    account_names.extend((0..count).map(|offset| format!("{prefix}{}", start + offset)));
}

pub(super) fn generate_generic_account_names(len: usize) -> Vec<String> {
    (0..len).map(|i| format!("account_{}", i + 1)).collect()
}
