use anyhow::Result;
use solana_pubkey::Pubkey;
use std::convert::TryInto;

use super::{InstructionParser, ParsedField, ParsedInstruction};
use crate::core::transaction::InstructionSummary;

/// Token2022 program ID: TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb
const TOKEN2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

/// Instruction discriminators for Token2022 program
/// Token2022 uses the same discriminators as Token for basic instructions
const INITIALIZE_MINT_DISCRIMINATOR: u32 = 0;
const INITIALIZE_ACCOUNT_DISCRIMINATOR: u32 = 1;
const INITIALIZE_MULTISIG_DISCRIMINATOR: u32 = 2;
const TRANSFER_DISCRIMINATOR: u32 = 3;
const APPROVE_DISCRIMINATOR: u32 = 4;
const REVOKE_DISCRIMINATOR: u32 = 5;
const SET_AUTHORITY_DISCRIMINATOR: u32 = 6;
const MINT_TO_DISCRIMINATOR: u32 = 7;
const BURN_DISCRIMINATOR: u32 = 8;
const CLOSE_ACCOUNT_DISCRIMINATOR: u32 = 9;
const FREEZE_ACCOUNT_DISCRIMINATOR: u32 = 10;
const THAW_ACCOUNT_DISCRIMINATOR: u32 = 11;
const TRANSFER_CHECKED_DISCRIMINATOR: u32 = 12;
const APPROVE_CHECKED_DISCRIMINATOR: u32 = 13;
const MINT_TO_CHECKED_DISCRIMINATOR: u32 = 14;
const BURN_CHECKED_DISCRIMINATOR: u32 = 15;
const INITIALIZE_ACCOUNT2_DISCRIMINATOR: u32 = 16;
const SYNC_NATIVE_DISCRIMINATOR: u32 = 17;
const INITIALIZE_ACCOUNT3_DISCRIMINATOR: u32 = 18;
const INITIALIZE_MULTISIG2_DISCRIMINATOR: u32 = 19;
const INITIALIZE_MINT2_DISCRIMINATOR: u32 = 20;
const GET_ACCOUNT_DATA_SIZE_DISCRIMINATOR: u32 = 21;
const INITIALIZE_IMMUTABLE_OWNER_DISCRIMINATOR: u32 = 22;
const AMOUNT_TO_UI_AMOUNT_DISCRIMINATOR: u32 = 23;
const UI_AMOUNT_TO_AMOUNT_DISCRIMINATOR: u32 = 24;
const INITIALIZE_MINT_CLOSE_AUTHORITY_DISCRIMINATOR: u32 = 25;
const TRANSFER_FEE_EXTENSION_DISCRIMINATOR: u32 = 26;
const CONFIDENTIAL_TRANSFER_EXTENSION_DISCRIMINATOR: u32 = 27;
const DEFAULT_ACCOUNT_STATE_EXTENSION_DISCRIMINATOR: u32 = 28;
const REALLOCATE_DISCRIMINATOR: u32 = 29;
const MEMO_TRANSFER_EXTENSION_DISCRIMINATOR: u32 = 30;
const CREATE_NATIVE_MINT_DISCRIMINATOR: u32 = 31;
const INITIALIZE_NON_TRANSFERABLE_MINT_DISCRIMINATOR: u32 = 32;
const INTEREST_BEARING_MINT_EXTENSION_DISCRIMINATOR: u32 = 33;
const CPI_GUARD_EXTENSION_DISCRIMINATOR: u32 = 34;
const INITIALIZE_PERMANENT_DELEGATE_DISCRIMINATOR: u32 = 35;
const TRANSFER_HOOK_EXTENSION_DISCRIMINATOR: u32 = 36;
const CONFIDENTIAL_TRANSFER_FEE_EXTENSION_DISCRIMINATOR: u32 = 37;
const WITHDRAW_EXCESS_LAMPORTS_DISCRIMINATOR: u32 = 38;
const METADATA_POINTER_EXTENSION_DISCRIMINATOR: u32 = 39;
const GROUP_POINTER_EXTENSION_DISCRIMINATOR: u32 = 40;
const GROUP_MEMBER_POINTER_EXTENSION_DISCRIMINATOR: u32 = 41;
const CONFIDENTIAL_MINT_BURN_EXTENSION_DISCRIMINATOR: u32 = 42;
const SCALED_UI_AMOUNT_EXTENSION_DISCRIMINATOR: u32 = 43;
const PAUSABLE_EXTENSION_DISCRIMINATOR: u32 = 44;

/// Parser for the Token2022 Program instructions
/// Supports all Token2022 Program instructions including Transfer and TransferChecked
pub struct Token2022ProgramParser {
    program_id: Pubkey,
}

impl Token2022ProgramParser {
    pub fn new() -> Self {
        Self::with_program_id(Pubkey::from_str_const(TOKEN2022_PROGRAM_ID))
    }

    pub fn with_program_id(program_id: Pubkey) -> Self {
        Self { program_id }
    }
}

impl Default for Token2022ProgramParser {
    fn default() -> Self {
        Self::new()
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

        // Token2022 program uses 1-byte instruction discriminator (same as Token)
        let instruction_id = instruction.data[0] as u32;
        let data = &instruction.data[1..];

        match instruction_id {
            INITIALIZE_MINT_DISCRIMINATOR => parse_initialize_mint_instruction(data, instruction),
            INITIALIZE_ACCOUNT_DISCRIMINATOR => {
                parse_initialize_account_instruction(data, instruction)
            }
            INITIALIZE_MULTISIG_DISCRIMINATOR => {
                parse_initialize_multisig_instruction(data, instruction)
            }
            TRANSFER_DISCRIMINATOR => parse_transfer_instruction(data, instruction),
            APPROVE_DISCRIMINATOR => parse_approve_instruction(data, instruction),
            REVOKE_DISCRIMINATOR => parse_revoke_instruction(data, instruction),
            SET_AUTHORITY_DISCRIMINATOR => parse_set_authority_instruction(data, instruction),
            MINT_TO_DISCRIMINATOR => parse_mint_to_instruction(data, instruction),
            BURN_DISCRIMINATOR => parse_burn_instruction(data, instruction),
            CLOSE_ACCOUNT_DISCRIMINATOR => parse_close_account_instruction(data, instruction),
            FREEZE_ACCOUNT_DISCRIMINATOR => parse_freeze_account_instruction(data, instruction),
            THAW_ACCOUNT_DISCRIMINATOR => parse_thaw_account_instruction(data, instruction),
            TRANSFER_CHECKED_DISCRIMINATOR => parse_transfer_checked_instruction(data, instruction),
            APPROVE_CHECKED_DISCRIMINATOR => parse_approve_checked_instruction(data, instruction),
            MINT_TO_CHECKED_DISCRIMINATOR => parse_mint_to_checked_instruction(data, instruction),
            BURN_CHECKED_DISCRIMINATOR => parse_burn_checked_instruction(data, instruction),
            INITIALIZE_ACCOUNT2_DISCRIMINATOR => {
                parse_initialize_account2_instruction(data, instruction)
            }
            SYNC_NATIVE_DISCRIMINATOR => parse_sync_native_instruction(data, instruction),
            INITIALIZE_ACCOUNT3_DISCRIMINATOR => {
                parse_initialize_account3_instruction(data, instruction)
            }
            INITIALIZE_MULTISIG2_DISCRIMINATOR => {
                parse_initialize_multisig2_instruction(data, instruction)
            }
            INITIALIZE_MINT2_DISCRIMINATOR => parse_initialize_mint2_instruction(data, instruction),
            GET_ACCOUNT_DATA_SIZE_DISCRIMINATOR => {
                parse_get_account_data_size_instruction(data, instruction)
            }
            INITIALIZE_IMMUTABLE_OWNER_DISCRIMINATOR => {
                parse_initialize_immutable_owner_instruction(data, instruction)
            }
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
            CONFIDENTIAL_TRANSFER_EXTENSION_DISCRIMINATOR => parse_extension_prefix_instruction(
                "ConfidentialTransferExtension",
                data,
                instruction,
            ),
            DEFAULT_ACCOUNT_STATE_EXTENSION_DISCRIMINATOR => parse_extension_prefix_instruction(
                "DefaultAccountStateExtension",
                data,
                instruction,
            ),
            REALLOCATE_DISCRIMINATOR => parse_reallocate_instruction(data, instruction),
            MEMO_TRANSFER_EXTENSION_DISCRIMINATOR => {
                parse_extension_prefix_instruction("MemoTransferExtension", data, instruction)
            }
            CREATE_NATIVE_MINT_DISCRIMINATOR => parse_create_native_mint_instruction(instruction),
            INITIALIZE_NON_TRANSFERABLE_MINT_DISCRIMINATOR => {
                parse_single_account_instruction("InitializeNonTransferableMint", instruction)
            }
            INTEREST_BEARING_MINT_EXTENSION_DISCRIMINATOR => parse_extension_prefix_instruction(
                "InterestBearingMintExtension",
                data,
                instruction,
            ),
            CPI_GUARD_EXTENSION_DISCRIMINATOR => {
                parse_extension_prefix_instruction("CpiGuardExtension", data, instruction)
            }
            INITIALIZE_PERMANENT_DELEGATE_DISCRIMINATOR => {
                parse_initialize_permanent_delegate_instruction(data, instruction)
            }
            TRANSFER_HOOK_EXTENSION_DISCRIMINATOR => {
                parse_extension_prefix_instruction("TransferHookExtension", data, instruction)
            }
            CONFIDENTIAL_TRANSFER_FEE_EXTENSION_DISCRIMINATOR => {
                parse_extension_prefix_instruction(
                    "ConfidentialTransferFeeExtension",
                    data,
                    instruction,
                )
            }
            WITHDRAW_EXCESS_LAMPORTS_DISCRIMINATOR => {
                parse_withdraw_excess_lamports_instruction(instruction)
            }
            METADATA_POINTER_EXTENSION_DISCRIMINATOR => {
                parse_extension_prefix_instruction("MetadataPointerExtension", data, instruction)
            }
            GROUP_POINTER_EXTENSION_DISCRIMINATOR => {
                parse_extension_prefix_instruction("GroupPointerExtension", data, instruction)
            }
            GROUP_MEMBER_POINTER_EXTENSION_DISCRIMINATOR => {
                parse_extension_prefix_instruction("GroupMemberPointerExtension", data, instruction)
            }
            CONFIDENTIAL_MINT_BURN_EXTENSION_DISCRIMINATOR => parse_extension_prefix_instruction(
                "ConfidentialMintBurnExtension",
                data,
                instruction,
            ),
            SCALED_UI_AMOUNT_EXTENSION_DISCRIMINATOR => {
                parse_extension_prefix_instruction("ScaledUiAmountExtension", data, instruction)
            }
            PAUSABLE_EXTENSION_DISCRIMINATOR => {
                parse_extension_prefix_instruction("PausableExtension", data, instruction)
            }
            _ => Ok(None), // Unknown instruction
        }
    }
}

fn append_additional_signer_accounts(
    account_names: &mut Vec<String>,
    base_accounts: usize,
    total_accounts: usize,
) {
    for i in base_accounts..total_accounts {
        account_names.push(format!("additional_signer_{}", i - base_accounts + 1));
    }
}

/// Parses a Transfer instruction: 3
///
/// Accounts: 0. source pubkey, 1. destination pubkey, 2. owner pubkey
/// Data: 8 bytes - amount (u64, little-endian)
fn parse_transfer_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 8 {
        return Ok(None); // Invalid data length for Transfer
    }

    if instruction.accounts.len() < 3 {
        return Ok(None); // Need source, destination, and owner/authority
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);

    let mut account_names =
        vec!["source".to_string(), "destination".to_string(), "owner".to_string()];
    append_additional_signer_accounts(&mut account_names, 3, instruction.accounts.len());

    Ok(Some(ParsedInstruction {
        name: "Transfer".to_string(),
        fields: vec![ParsedField::text("amount", amount.to_string())],
        account_names,
    }))
}

/// Parses a TransferChecked instruction: 12
///
/// Accounts: 0. source pubkey, 1. mint pubkey, 2. destination pubkey, 3. owner pubkey
/// Optional: 4+. signers (if owner is a PDA/multisig) - not explicitly listed in account names
/// Data: 8 bytes - amount (u64, little-endian), 1 byte - decimals (u8)
fn parse_transfer_checked_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 9 {
        return Ok(None); // Invalid data length for TransferChecked (amount + decimals)
    }

    if instruction.accounts.len() < 4 {
        return Ok(None); // Invalid number of accounts for TransferChecked
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);
    let decimals = data[8];

    let mut account_names = vec![
        "source".to_string(),
        "mint".to_string(),
        "destination".to_string(),
        "owner".to_string(),
    ];

    append_additional_signer_accounts(&mut account_names, 4, instruction.accounts.len());

    Ok(Some(ParsedInstruction {
        name: "TransferChecked".to_string(),
        fields: vec![
            ParsedField::text("amount", amount.to_string()),
            ParsedField::text("decimals", decimals.to_string()),
        ],
        account_names,
    }))
}

/// Parses an InitializeMint instruction: 0
fn parse_initialize_mint_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() < 34 {
        return Ok(None); // Invalid data length for InitializeMint
    }

    let decimals = data[0];

    let mut fields = vec![ParsedField::text("decimals", decimals.to_string())];

    let has_freeze_authority = match data[33] {
        0 => false,
        1 => {
            if data.len() < 66 {
                return Ok(None);
            }
            true
        }
        _ => return Ok(None),
    };
    fields.push(ParsedField::text("has_freeze_authority", has_freeze_authority.to_string()));

    Ok(Some(ParsedInstruction {
        name: "InitializeMint".to_string(),
        fields,
        account_names: vec!["mint".to_string(), "rent_sysvar".to_string()],
    }))
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
        fields: vec![],
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
    if data.len() != 1 {
        return Ok(None); // Invalid data length for InitializeMultisig
    }

    if instruction.accounts.len() < 3 {
        return Ok(None); // Need at least multisig + one signer + rent sysvar
    }

    let m = data[0];

    let mut account_names = vec!["multisig".to_string(), "rent_sysvar".to_string()];
    let num_signers = instruction.accounts.len().saturating_sub(2);
    for i in 0..num_signers {
        account_names.insert(1 + i, format!("signer_{}", i + 1));
    }

    Ok(Some(ParsedInstruction {
        name: "InitializeMultisig".to_string(),
        fields: vec![ParsedField::text("m", m.to_string())],
        account_names,
    }))
}

/// Parses an Approve instruction: 4
fn parse_approve_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 8 {
        return Ok(None); // Invalid data length for Approve
    }

    if instruction.accounts.len() < 3 {
        return Ok(None); // Invalid number of accounts for Approve
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);

    let mut account_names = vec!["source".to_string(), "delegate".to_string(), "owner".to_string()];
    append_additional_signer_accounts(&mut account_names, 3, instruction.accounts.len());

    Ok(Some(ParsedInstruction {
        name: "Approve".to_string(),
        fields: vec![ParsedField::text("amount", amount.to_string())],
        account_names,
    }))
}

/// Parses a Revoke instruction: 5
fn parse_revoke_instruction(
    _data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 2 {
        return Ok(None); // Invalid number of accounts for Revoke
    }

    let mut account_names = vec!["source".to_string(), "owner".to_string()];
    append_additional_signer_accounts(&mut account_names, 2, instruction.accounts.len());

    Ok(Some(ParsedInstruction { name: "Revoke".to_string(), fields: vec![], account_names }))
}

/// Parses a SetAuthority instruction: 6
fn parse_set_authority_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() < 2 {
        return Ok(None); // Invalid data length for SetAuthority
    }
    if instruction.accounts.len() < 2 {
        return Ok(None); // Need account and current authority
    }

    let authority_type = match data[0] {
        0 => "MintTokens",
        1 => "FreezeAccount",
        2 => "AccountOwner",
        3 => "CloseAccount",
        _ => "Unknown",
    };

    let mut fields = vec![ParsedField::text("authority_type", authority_type.to_string())];

    let cleared = match data[1] {
        0 => true,
        1 => {
            if data.len() < 34 {
                return Ok(None);
            }
            false
        }
        _ => return Ok(None),
    };
    fields.push(ParsedField::text("cleared", cleared.to_string()));

    let mut account_names = vec!["account".to_string(), "authority".to_string()];
    append_additional_signer_accounts(&mut account_names, 2, instruction.accounts.len());

    Ok(Some(ParsedInstruction { name: "SetAuthority".to_string(), fields, account_names }))
}

/// Parses a MintTo instruction: 7
fn parse_mint_to_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 8 {
        return Ok(None); // Invalid data length for MintTo
    }

    if instruction.accounts.len() < 3 {
        return Ok(None); // Invalid number of accounts for MintTo
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);

    let mut account_names = vec!["mint".to_string(), "account".to_string(), "owner".to_string()];
    append_additional_signer_accounts(&mut account_names, 3, instruction.accounts.len());

    Ok(Some(ParsedInstruction {
        name: "MintTo".to_string(),
        fields: vec![ParsedField::text("amount", amount.to_string())],
        account_names,
    }))
}

/// Parses a Burn instruction: 8
fn parse_burn_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 8 {
        return Ok(None); // Invalid data length for Burn
    }

    if instruction.accounts.len() < 3 {
        return Ok(None); // Invalid number of accounts for Burn
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);

    let mut account_names = vec!["account".to_string(), "mint".to_string(), "owner".to_string()];
    append_additional_signer_accounts(&mut account_names, 3, instruction.accounts.len());

    Ok(Some(ParsedInstruction {
        name: "Burn".to_string(),
        fields: vec![ParsedField::text("amount", amount.to_string())],
        account_names,
    }))
}

/// Parses a CloseAccount instruction: 9
fn parse_close_account_instruction(
    _data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 3 {
        return Ok(None); // Invalid number of accounts for CloseAccount
    }

    let mut account_names =
        vec!["account".to_string(), "destination".to_string(), "owner".to_string()];
    append_additional_signer_accounts(&mut account_names, 3, instruction.accounts.len());

    Ok(Some(ParsedInstruction { name: "CloseAccount".to_string(), fields: vec![], account_names }))
}

/// Parses a FreezeAccount instruction: 10
fn parse_freeze_account_instruction(
    _data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 3 {
        return Ok(None); // Invalid number of accounts for FreezeAccount
    }

    let mut account_names =
        vec!["account".to_string(), "mint".to_string(), "freeze_authority".to_string()];
    append_additional_signer_accounts(&mut account_names, 3, instruction.accounts.len());

    Ok(Some(ParsedInstruction { name: "FreezeAccount".to_string(), fields: vec![], account_names }))
}

/// Parses a ThawAccount instruction: 11
fn parse_thaw_account_instruction(
    _data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 3 {
        return Ok(None); // Invalid number of accounts for ThawAccount
    }

    let mut account_names =
        vec!["account".to_string(), "mint".to_string(), "freeze_authority".to_string()];
    append_additional_signer_accounts(&mut account_names, 3, instruction.accounts.len());

    Ok(Some(ParsedInstruction { name: "ThawAccount".to_string(), fields: vec![], account_names }))
}

/// Parses an ApproveChecked instruction: 13
fn parse_approve_checked_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 9 {
        return Ok(None); // Invalid data length for ApproveChecked
    }

    if instruction.accounts.len() < 4 {
        return Ok(None); // Invalid number of accounts for ApproveChecked
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);
    let decimals = data[8];

    let mut account_names =
        vec!["source".to_string(), "mint".to_string(), "delegate".to_string(), "owner".to_string()];

    append_additional_signer_accounts(&mut account_names, 4, instruction.accounts.len());

    Ok(Some(ParsedInstruction {
        name: "ApproveChecked".to_string(),
        fields: vec![
            ParsedField::text("amount", amount.to_string()),
            ParsedField::text("decimals", decimals.to_string()),
        ],
        account_names,
    }))
}

/// Parses a MintToChecked instruction: 14
fn parse_mint_to_checked_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 9 {
        return Ok(None); // Invalid data length for MintToChecked
    }

    if instruction.accounts.len() < 3 {
        return Ok(None); // Invalid number of accounts for MintToChecked
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);
    let decimals = data[8];

    let mut account_names = vec!["mint".to_string(), "account".to_string(), "owner".to_string()];
    append_additional_signer_accounts(&mut account_names, 3, instruction.accounts.len());

    Ok(Some(ParsedInstruction {
        name: "MintToChecked".to_string(),
        fields: vec![
            ParsedField::text("amount", amount.to_string()),
            ParsedField::text("decimals", decimals.to_string()),
        ],
        account_names,
    }))
}

/// Parses a BurnChecked instruction: 15
fn parse_burn_checked_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 9 {
        return Ok(None); // Invalid data length for BurnChecked
    }

    if instruction.accounts.len() < 3 {
        return Ok(None); // Invalid number of accounts for BurnChecked
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);
    let decimals = data[8];

    let mut account_names = vec!["account".to_string(), "mint".to_string(), "owner".to_string()];
    append_additional_signer_accounts(&mut account_names, 3, instruction.accounts.len());

    Ok(Some(ParsedInstruction {
        name: "BurnChecked".to_string(),
        fields: vec![
            ParsedField::text("amount", amount.to_string()),
            ParsedField::text("decimals", decimals.to_string()),
        ],
        account_names,
    }))
}

/// Parses an InitializeAccount2 instruction: 16
fn parse_initialize_account2_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 32 {
        return Ok(None); // Invalid data length for InitializeAccount2
    }

    if instruction.accounts.len() != 3 {
        return Ok(None); // Invalid number of accounts for InitializeAccount2
    }

    // The data contains the owner pubkey for validation
    let owner_pubkey = bs58::encode(&data[..32]).into_string();

    Ok(Some(ParsedInstruction {
        name: "InitializeAccount2".to_string(),
        fields: vec![ParsedField::text("owner", owner_pubkey)],
        account_names: vec!["account".to_string(), "mint".to_string(), "rent_sysvar".to_string()],
    }))
}

/// Parses a SyncNative instruction: 17
fn parse_sync_native_instruction(
    _data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 1 {
        return Ok(None); // Invalid number of accounts for SyncNative
    }

    Ok(Some(ParsedInstruction {
        name: "SyncNative".to_string(),
        fields: vec![],
        account_names: vec!["account".to_string()],
    }))
}

/// Parses an InitializeAccount3 instruction: 18
fn parse_initialize_account3_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 32 {
        return Ok(None); // Invalid data length for InitializeAccount3
    }

    if instruction.accounts.len() < 2 {
        return Ok(None); // Invalid number of accounts for InitializeAccount3
    }

    // The data contains the owner pubkey for validation
    let owner_pubkey = bs58::encode(&data[..32]).into_string();

    let mut account_names = vec!["account".to_string(), "mint".to_string()];
    for i in 2..instruction.accounts.len() {
        account_names.push(format!("account_{}", i + 1));
    }

    Ok(Some(ParsedInstruction {
        name: "InitializeAccount3".to_string(),
        fields: vec![ParsedField::text("owner", owner_pubkey)],
        account_names,
    }))
}

/// Parses an InitializeMultisig2 instruction: 19
fn parse_initialize_multisig2_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 1 {
        return Ok(None); // Invalid data length for InitializeMultisig2
    }

    if instruction.accounts.len() < 2 {
        return Ok(None); // Need at least multisig + one signer
    }

    let m = data[0];

    let mut account_names = vec!["multisig".to_string()];
    let num_signers = instruction.accounts.len().saturating_sub(1);
    for i in 0..num_signers {
        account_names.push(format!("signer_{}", i + 1));
    }

    Ok(Some(ParsedInstruction {
        name: "InitializeMultisig2".to_string(),
        fields: vec![ParsedField::text("m", m.to_string())],
        account_names,
    }))
}

/// Parses an InitializeMint2 instruction: 20
fn parse_initialize_mint2_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() < 34 {
        return Ok(None); // Invalid data length for InitializeMint2
    }

    let decimals = data[0];

    let mut fields = vec![ParsedField::text("decimals", decimals.to_string())];

    let has_freeze_authority = match data[33] {
        0 => false,
        1 => {
            if data.len() < 66 {
                return Ok(None);
            }
            true
        }
        _ => return Ok(None),
    };
    fields.push(ParsedField::text("has_freeze_authority", has_freeze_authority.to_string()));

    Ok(Some(ParsedInstruction {
        name: "InitializeMint2".to_string(),
        fields,
        account_names: vec!["mint".to_string()],
    }))
}

/// Parses a GetAccountDataSize instruction: 21
fn parse_get_account_data_size_instruction(
    _data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 1 {
        return Ok(None); // Invalid number of accounts for GetAccountDataSize
    }

    Ok(Some(ParsedInstruction {
        name: "GetAccountDataSize".to_string(),
        fields: vec![],
        account_names: vec!["mint".to_string()],
    }))
}

/// Parses an InitializeImmutableOwner instruction: 22
fn parse_initialize_immutable_owner_instruction(
    _data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 1 {
        return Ok(None); // Invalid number of accounts for InitializeImmutableOwner
    }

    Ok(Some(ParsedInstruction {
        name: "InitializeImmutableOwner".to_string(),
        fields: vec![],
        account_names: vec!["account".to_string()],
    }))
}

/// Parses an AmountToUiAmount instruction: 23
fn parse_amount_to_ui_amount_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 8 {
        return Ok(None); // Invalid data length for AmountToUiAmount
    }

    if instruction.accounts.len() != 1 {
        return Ok(None); // Invalid number of accounts for AmountToUiAmount
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);

    Ok(Some(ParsedInstruction {
        name: "AmountToUiAmount".to_string(),
        fields: vec![ParsedField::text("amount", amount.to_string())],
        account_names: vec!["mint".to_string()],
    }))
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
        fields: vec![ParsedField::text("ui_amount", ui_amount)],
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

    let (close_authority, _) = match read_coption_pubkey_string(data) {
        Some(value) => value,
        None => return Ok(None),
    };

    let close_authority_value = close_authority.unwrap_or_else(|| "none".to_string());

    Ok(Some(ParsedInstruction {
        name: "InitializeMintCloseAuthority".to_string(),
        fields: vec![ParsedField::text("close_authority", close_authority_value)],
        account_names: vec!["mint".to_string()],
    }))
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
            fields: vec![ParsedField::text("raw_extension_data", hex::encode(payload))],
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

    let (config_authority, rest) = match read_coption_pubkey_string(data) {
        Some(value) => value,
        None => return Ok(None),
    };
    let (withdraw_authority, rest) = match read_coption_pubkey_string(rest) {
        Some(value) => value,
        None => return Ok(None),
    };
    let (bps, rest) = match read_u16_le(rest) {
        Some(value) => value,
        None => return Ok(None),
    };
    let (maximum_fee, _) = match read_u64_le(rest) {
        Some(value) => value,
        None => return Ok(None),
    };

    Ok(Some(ParsedInstruction {
        name: "InitializeTransferFeeConfig".to_string(),
        fields: vec![
            ParsedField::text(
                "transfer_fee_config_authority",
                config_authority.unwrap_or_else(|| "none".to_string()),
            ),
            ParsedField::text(
                "withdraw_withheld_authority",
                withdraw_authority.unwrap_or_else(|| "none".to_string()),
            ),
            ParsedField::text("transfer_fee_basis_points", bps.to_string()),
            ParsedField::text("maximum_fee", maximum_fee.to_string()),
        ],
        account_names: vec!["mint".to_string()],
    }))
}

fn parse_transfer_checked_with_fee_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 3 {
        return Ok(None);
    }

    let (amount, rest) = match read_u64_le(data) {
        Some(value) => value,
        None => return Ok(None),
    };
    if rest.is_empty() {
        return Ok(None);
    }
    let decimals = rest[0];
    let (fee, _) = match read_u64_le(&rest[1..]) {
        Some(value) => value,
        None => return Ok(None),
    };

    let mut account_names = Vec::new();
    if !instruction.accounts.is_empty() {
        account_names.push("source".to_string());
    }
    if instruction.accounts.len() > 1 {
        account_names.push("mint".to_string());
    }
    if instruction.accounts.len() > 2 {
        account_names.push("destination".to_string());
    }
    if instruction.accounts.len() > 3 {
        account_names.push("authority".to_string());
    }
    for i in 4..instruction.accounts.len() {
        account_names.push(format!("additional_signer_{}", i - 3));
    }

    Ok(Some(ParsedInstruction {
        name: "TransferCheckedWithFee".to_string(),
        fields: vec![
            ParsedField::text("amount", amount.to_string()),
            ParsedField::text("decimals", decimals.to_string()),
            ParsedField::text("fee", fee.to_string()),
        ],
        account_names,
    }))
}

fn parse_transfer_fee_withdraw_from_mint_instruction(
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 2 {
        return Ok(None);
    }

    let mut account_names = Vec::new();
    if !instruction.accounts.is_empty() {
        account_names.push("mint".to_string());
    }
    if instruction.accounts.len() >= 2 {
        account_names.push("destination".to_string());
    }
    if instruction.accounts.len() >= 3 {
        account_names.push("authority".to_string());
    }
    for i in 3..instruction.accounts.len() {
        account_names.push(format!("additional_signer_{}", i - 2));
    }

    Ok(Some(ParsedInstruction {
        name: "WithdrawWithheldTokensFromMint".to_string(),
        fields: vec![],
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
    let num_token_accounts = data[0] as usize;
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
        fields: vec![ParsedField::text("num_token_accounts", num_token_accounts.to_string())],
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
        fields: vec![],
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

    let (transfer_fee_basis_points, rest) = match read_u16_le(data) {
        Some(value) => value,
        None => return Ok(None),
    };
    let (maximum_fee, _) = match read_u64_le(rest) {
        Some(value) => value,
        None => return Ok(None),
    };

    let mut account_names = Vec::new();
    if !instruction.accounts.is_empty() {
        account_names.push("mint".to_string());
    }
    if instruction.accounts.len() >= 2 {
        account_names.push("authority".to_string());
    }
    for i in 2..instruction.accounts.len() {
        account_names.push(format!("additional_signer_{}", i - 1));
    }

    Ok(Some(ParsedInstruction {
        name: "SetTransferFee".to_string(),
        fields: vec![
            ParsedField::text("transfer_fee_basis_points", transfer_fee_basis_points.to_string()),
            ParsedField::text("maximum_fee", maximum_fee.to_string()),
        ],
        account_names,
    }))
}

fn parse_reallocate_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if !data.len().is_multiple_of(2) {
        return Ok(None);
    }

    let mut extension_types = Vec::new();
    for chunk in data.chunks(2) {
        if chunk.len() == 2 {
            extension_types.push(u16::from_le_bytes([chunk[0], chunk[1]]).to_string());
        }
    }

    let mut account_names = Vec::new();
    if !instruction.accounts.is_empty() {
        account_names.push("account".to_string());
    }
    if instruction.accounts.len() >= 2 {
        account_names.push("payer".to_string());
    }
    if instruction.accounts.len() >= 3 {
        account_names.push("system_program".to_string());
    }
    if instruction.accounts.len() >= 4 {
        account_names.push("owner_or_delegate".to_string());
    }
    for i in 4..instruction.accounts.len() {
        account_names.push(format!("additional_signer_{}", i - 3));
    }

    Ok(Some(ParsedInstruction {
        name: "Reallocate".to_string(),
        fields: vec![ParsedField::text(
            "extension_types",
            if extension_types.is_empty() { "none".to_string() } else { extension_types.join(",") },
        )],
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
        fields: vec![],
        account_names: vec![
            "funding_account".to_string(),
            "native_mint".to_string(),
            "system_program".to_string(),
        ],
    }))
}

fn parse_single_account_instruction(
    name: &str,
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 1 {
        return Ok(None);
    }

    Ok(Some(ParsedInstruction {
        name: name.to_string(),
        fields: vec![],
        account_names: vec!["mint".to_string()],
    }))
}

fn parse_initialize_permanent_delegate_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 1 {
        return Ok(None);
    }

    let (delegate, _) = match read_pubkey_string(data) {
        Some(value) => value,
        None => return Ok(None),
    };

    Ok(Some(ParsedInstruction {
        name: "InitializePermanentDelegate".to_string(),
        fields: vec![ParsedField::text("delegate", delegate)],
        account_names: vec!["mint".to_string()],
    }))
}

fn parse_withdraw_excess_lamports_instruction(
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() < 3 {
        return Ok(None);
    }

    let mut account_names =
        vec!["source".to_string(), "destination".to_string(), "authority".to_string()];
    for i in 3..instruction.accounts.len() {
        account_names.push(format!("additional_signer_{}", i - 2));
    }

    Ok(Some(ParsedInstruction {
        name: "WithdrawExcessLamports".to_string(),
        fields: vec![],
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
        fields.push(ParsedField::text("raw_extension_data", hex::encode(data)));
    }

    Ok(Some(ParsedInstruction {
        name: name.to_string(),
        fields,
        account_names: generate_generic_account_names(instruction.accounts.len()),
    }))
}

fn read_pubkey_string(data: &[u8]) -> Option<(String, &[u8])> {
    if data.len() < 32 {
        return None;
    }
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&data[..32]);
    let key = Pubkey::new_from_array(bytes);
    Some((key.to_string(), &data[32..]))
}

fn read_coption_pubkey_string(data: &[u8]) -> Option<(Option<String>, &[u8])> {
    if data.is_empty() {
        return None;
    }
    match data[0] {
        0 => Some((None, &data[1..])),
        1 => {
            let (key, rest) = read_pubkey_string(&data[1..])?;
            Some((Some(key), rest))
        }
        _ => None,
    }
}

fn read_u16_le(data: &[u8]) -> Option<(u16, &[u8])> {
    if data.len() < 2 {
        return None;
    }
    let value = u16::from_le_bytes([data[0], data[1]]);
    Some((value, &data[2..]))
}

fn read_u64_le(data: &[u8]) -> Option<(u64, &[u8])> {
    if data.len() < 8 {
        return None;
    }
    let value = u64::from_le_bytes(data[..8].try_into().ok()?);
    Some((value, &data[8..]))
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
