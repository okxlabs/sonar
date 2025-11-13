use anyhow::Result;
use solana_pubkey::Pubkey;

use super::{InstructionParser, ParsedInstruction};
use crate::transaction::InstructionSummary;

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

/// Parser for the Token2022 Program instructions
/// Supports all Token2022 Program instructions including Transfer and TransferChecked
pub struct Token2022ProgramParser {
    program_id: Pubkey,
}

impl Token2022ProgramParser {
    pub fn new() -> Self {
        Self { program_id: Pubkey::from_str_const(TOKEN2022_PROGRAM_ID) }
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
            _ => Ok(None), // Unknown instruction
        }
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

    if instruction.accounts.len() < 2 {
        return Ok(None); // Need at least source and destination
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);

    Ok(Some(ParsedInstruction {
        name: "Transfer".to_string(),
        fields: vec![("amount".to_string(), amount.to_string())],
        account_names: vec!["source".to_string(), "destination".to_string(), "owner".to_string()],
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

    // When owner is a multisig or PDA, additional signers are included
    // Base setup has 4 accounts: source, mint, dest, owner
    let has_multiple_signers = instruction.accounts.len() > 4;

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

    // Add additional signer accounts if present
    if has_multiple_signers {
        for i in 4..instruction.accounts.len() {
            account_names.push(format!("additional_signer_{}", i - 3));
        }
    }

    Ok(Some(ParsedInstruction {
        name: "TransferChecked".to_string(),
        fields: vec![
            ("amount".to_string(), amount.to_string()),
            ("decimals".to_string(), decimals.to_string()),
        ],
        account_names,
    }))
}

/// Parses an InitializeMint instruction: 0
fn parse_initialize_mint_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() < 4 {
        return Ok(None); // Invalid data length for InitializeMint
    }

    let decimals = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);

    let mut fields = vec![("decimals".to_string(), decimals.to_string())];

    // Check for mint authority (4 bytes + 32 bytes = 36 bytes minimum)
    if data.len() >= 36 {
        let has_freeze_authority = data[35] != 0;
        fields.push(("has_freeze_authority".to_string(), has_freeze_authority.to_string()));
    }

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
        fields: vec![("m".to_string(), m.to_string())],
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

    if instruction.accounts.len() != 3 {
        return Ok(None); // Invalid number of accounts for Approve
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);

    Ok(Some(ParsedInstruction {
        name: "Approve".to_string(),
        fields: vec![("amount".to_string(), amount.to_string())],
        account_names: vec!["source".to_string(), "delegate".to_string(), "owner".to_string()],
    }))
}

/// Parses a Revoke instruction: 5
fn parse_revoke_instruction(
    _data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 2 {
        return Ok(None); // Invalid number of accounts for Revoke
    }

    Ok(Some(ParsedInstruction {
        name: "Revoke".to_string(),
        fields: vec![],
        account_names: vec!["source".to_string(), "owner".to_string()],
    }))
}

/// Parses a SetAuthority instruction: 6
fn parse_set_authority_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.is_empty() {
        return Ok(None); // Invalid data length for SetAuthority
    }

    let authority_type = match data[0] {
        0 => "MintTokens",
        1 => "FreezeAccount",
        2 => "AccountOwner",
        3 => "CloseAccount",
        _ => "Unknown",
    };

    let mut fields = vec![("authority_type".to_string(), authority_type.to_string())];

    // Check if new authority is present
    if data.len() > 1 && data[1] != 0 {
        fields.push(("cleared".to_string(), "false".to_string()));
    } else {
        fields.push(("cleared".to_string(), "true".to_string()));
    }

    Ok(Some(ParsedInstruction {
        name: "SetAuthority".to_string(),
        fields,
        account_names: vec!["account".to_string()],
    }))
}

/// Parses a MintTo instruction: 7
fn parse_mint_to_instruction(
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() != 8 {
        return Ok(None); // Invalid data length for MintTo
    }

    if instruction.accounts.len() != 3 {
        return Ok(None); // Invalid number of accounts for MintTo
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);

    Ok(Some(ParsedInstruction {
        name: "MintTo".to_string(),
        fields: vec![("amount".to_string(), amount.to_string())],
        account_names: vec!["mint".to_string(), "account".to_string(), "owner".to_string()],
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

    if instruction.accounts.len() != 3 {
        return Ok(None); // Invalid number of accounts for Burn
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);

    Ok(Some(ParsedInstruction {
        name: "Burn".to_string(),
        fields: vec![("amount".to_string(), amount.to_string())],
        account_names: vec!["account".to_string(), "mint".to_string(), "owner".to_string()],
    }))
}

/// Parses a CloseAccount instruction: 9
fn parse_close_account_instruction(
    _data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 3 {
        return Ok(None); // Invalid number of accounts for CloseAccount
    }

    Ok(Some(ParsedInstruction {
        name: "CloseAccount".to_string(),
        fields: vec![],
        account_names: vec!["account".to_string(), "destination".to_string(), "owner".to_string()],
    }))
}

/// Parses a FreezeAccount instruction: 10
fn parse_freeze_account_instruction(
    _data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 3 {
        return Ok(None); // Invalid number of accounts for FreezeAccount
    }

    Ok(Some(ParsedInstruction {
        name: "FreezeAccount".to_string(),
        fields: vec![],
        account_names: vec![
            "account".to_string(),
            "mint".to_string(),
            "freeze_authority".to_string(),
        ],
    }))
}

/// Parses a ThawAccount instruction: 11
fn parse_thaw_account_instruction(
    _data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if instruction.accounts.len() != 3 {
        return Ok(None); // Invalid number of accounts for ThawAccount
    }

    Ok(Some(ParsedInstruction {
        name: "ThawAccount".to_string(),
        fields: vec![],
        account_names: vec![
            "account".to_string(),
            "mint".to_string(),
            "freeze_authority".to_string(),
        ],
    }))
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

    let has_multiple_signers = instruction.accounts.len() > 4;
    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);
    let decimals = data[8];

    let mut account_names =
        vec!["source".to_string(), "mint".to_string(), "delegate".to_string(), "owner".to_string()];

    if has_multiple_signers {
        for i in 4..instruction.accounts.len() {
            account_names.push(format!("additional_signer_{}", i - 3));
        }
    }

    Ok(Some(ParsedInstruction {
        name: "ApproveChecked".to_string(),
        fields: vec![
            ("amount".to_string(), amount.to_string()),
            ("decimals".to_string(), decimals.to_string()),
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

    if instruction.accounts.len() != 3 {
        return Ok(None); // Invalid number of accounts for MintToChecked
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);
    let decimals = data[8];

    Ok(Some(ParsedInstruction {
        name: "MintToChecked".to_string(),
        fields: vec![
            ("amount".to_string(), amount.to_string()),
            ("decimals".to_string(), decimals.to_string()),
        ],
        account_names: vec!["mint".to_string(), "account".to_string(), "owner".to_string()],
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

    if instruction.accounts.len() != 3 {
        return Ok(None); // Invalid number of accounts for BurnChecked
    }

    let amount = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);
    let decimals = data[8];

    Ok(Some(ParsedInstruction {
        name: "BurnChecked".to_string(),
        fields: vec![
            ("amount".to_string(), amount.to_string()),
            ("decimals".to_string(), decimals.to_string()),
        ],
        account_names: vec!["account".to_string(), "mint".to_string(), "owner".to_string()],
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

    if instruction.accounts.len() != 4 {
        return Ok(None); // Invalid number of accounts for InitializeAccount2
    }

    // The data contains the owner pubkey for validation
    let owner_pubkey = bs58::encode(&data[..32]).into_string();

    Ok(Some(ParsedInstruction {
        name: "InitializeAccount2".to_string(),
        fields: vec![("owner".to_string(), owner_pubkey)],
        account_names: vec![
            "account".to_string(),
            "mint".to_string(),
            "owner".to_string(),
            "rent_sysvar".to_string(),
        ],
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

    if instruction.accounts.len() != 3 {
        return Ok(None); // Invalid number of accounts for InitializeAccount3
    }

    // The data contains the owner pubkey for validation
    let owner_pubkey = bs58::encode(&data[..32]).into_string();

    Ok(Some(ParsedInstruction {
        name: "InitializeAccount3".to_string(),
        fields: vec![("owner".to_string(), owner_pubkey)],
        account_names: vec!["account".to_string(), "mint".to_string(), "owner".to_string()],
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
        fields: vec![("m".to_string(), m.to_string())],
        account_names,
    }))
}

/// Parses an InitializeMint2 instruction: 20
fn parse_initialize_mint2_instruction(
    data: &[u8],
    _instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    if data.len() < 4 {
        return Ok(None); // Invalid data length for InitializeMint2
    }

    let decimals = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);

    let mut fields = vec![("decimals".to_string(), decimals.to_string())];

    // Check for mint authority (4 bytes + 32 bytes = 36 bytes minimum)
    if data.len() >= 36 {
        let has_freeze_authority = data[35] != 0;
        fields.push(("has_freeze_authority".to_string(), has_freeze_authority.to_string()));
    }

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
        fields: vec![("amount".to_string(), amount.to_string())],
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
        fields: vec![("ui_amount".to_string(), ui_amount)],
        account_names: vec!["mint".to_string()],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{AccountReferenceSummary, AccountSourceSummary};

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

        assert!(parsed.fields.iter().any(|(k, v)| k == "amount" && v == "1000000"));
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
    fn test_initialize_mint_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "MintPubkey1111111111111111111111111111111111", false, true),
            create_test_account(1, "RentSysvar111111111111111111111111111111111", false, false),
        ];

        // InitializeMint instruction with 1-byte discriminator (0) + 4 bytes decimals + authority data
        let mut data = vec![0]; // 1-byte discriminator
        data.extend_from_slice(&9_u32.to_le_bytes()); // 4 bytes decimals
        data.extend_from_slice(&[0u8; 33]); // 33 bytes of authority data

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "InitializeMint");
        assert_eq!(parsed.account_names.len(), 2);
        assert_eq!(parsed.account_names[0], "mint");
        assert_eq!(parsed.account_names[1], "rent_sysvar");

        assert!(parsed.fields.iter().any(|(k, v)| k == "decimals" && v == "9"));
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

        assert!(parsed.fields.iter().any(|(k, v)| k == "m" && v == "2"));
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

        assert!(parsed.fields.iter().any(|(k, v)| k == "amount" && v == "500000"));
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

        let accounts = vec![create_test_account(
            0,
            "AccountPubkey11111111111111111111111111111111",
            false,
            true,
        )];

        // SetAuthority instruction with 1-byte discriminator (6) + authority_type
        let mut data = vec![6]; // 1-byte discriminator for SetAuthority
        data.push(0); // authority_type = MintTokens
        data.push(1); // new_authority_flag = present

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "SetAuthority");
        assert_eq!(parsed.account_names.len(), 1);
        assert_eq!(parsed.account_names[0], "account");

        assert!(parsed.fields.iter().any(|(k, v)| k == "authority_type" && v == "MintTokens"));
        assert!(parsed.fields.iter().any(|(k, v)| k == "cleared" && v == "false"));
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

        assert!(parsed.fields.iter().any(|(k, v)| k == "amount" && v == "1000000"));
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

        assert!(parsed.fields.iter().any(|(k, v)| k == "amount" && v == "250000"));
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

        assert!(parsed.fields.iter().any(|(k, v)| k == "amount" && v == "300000"));
        assert!(parsed.fields.iter().any(|(k, v)| k == "decimals" && v == "6"));
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

        assert!(parsed.fields.iter().any(|(k, v)| k == "amount" && v == "2000000"));
        assert!(parsed.fields.iter().any(|(k, v)| k == "decimals" && v == "9"));
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

        assert!(parsed.fields.iter().any(|(k, v)| k == "amount" && v == "100000"));
        assert!(parsed.fields.iter().any(|(k, v)| k == "decimals" && v == "6"));
    }

    #[test]
    fn test_initialize_account2_instruction_parsing() {
        let parser = Token2022ProgramParser::new();

        let accounts = vec![
            create_test_account(0, "AccountPubkey11111111111111111111111111111111", false, true),
            create_test_account(1, "MintPubkey111111111111111111111111111111111", false, false),
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", false, false),
            create_test_account(3, "RentSysvar111111111111111111111111111111111", false, false),
        ];

        // InitializeAccount2 instruction with 1-byte discriminator (16) + 32 bytes owner pubkey
        let mut data = vec![16]; // 1-byte discriminator for InitializeAccount2
        data.extend_from_slice(&[0u8; 32]); // 32 bytes owner pubkey

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "InitializeAccount2");
        assert_eq!(parsed.account_names.len(), 4);
        assert_eq!(parsed.account_names[0], "account");
        assert_eq!(parsed.account_names[1], "mint");
        assert_eq!(parsed.account_names[2], "owner");
        assert_eq!(parsed.account_names[3], "rent_sysvar");

        // Check that owner field is present
        assert!(parsed.fields.iter().any(|(k, _)| k == "owner"));
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
            create_test_account(2, "OwnerPubkey111111111111111111111111111111111", false, false),
        ];

        // InitializeAccount3 instruction with 1-byte discriminator (18) + 32 bytes owner pubkey
        let mut data = vec![18]; // 1-byte discriminator for InitializeAccount3
        data.extend_from_slice(&[0u8; 32]); // 32 bytes owner pubkey

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "InitializeAccount3");
        assert_eq!(parsed.account_names.len(), 3);
        assert_eq!(parsed.account_names[0], "account");
        assert_eq!(parsed.account_names[1], "mint");
        assert_eq!(parsed.account_names[2], "owner");

        // Check that owner field is present
        assert!(parsed.fields.iter().any(|(k, _)| k == "owner"));
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

        assert!(parsed.fields.iter().any(|(k, v)| k == "m" && v == "2"));
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

        // InitializeMint2 instruction with 1-byte discriminator (20) + 4 bytes decimals + authority data
        let mut data = vec![20]; // 1-byte discriminator
        data.extend_from_slice(&6_u32.to_le_bytes()); // 4 bytes decimals
        data.extend_from_slice(&[0u8; 33]); // 33 bytes of authority data

        let instruction = create_test_instruction(data, accounts);

        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.name, "InitializeMint2");
        assert_eq!(parsed.account_names.len(), 1);
        assert_eq!(parsed.account_names[0], "mint");

        assert!(parsed.fields.iter().any(|(k, v)| k == "decimals" && v == "6"));
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

        assert!(parsed.fields.iter().any(|(k, v)| k == "amount" && v == "5000000"));
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

        assert!(parsed.fields.iter().any(|(k, v)| k == "ui_amount" && v == "100.50"));
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

        assert!(parsed.fields.iter().any(|(k, v)| k == "amount" && v == "750000"));
        assert!(parsed.fields.iter().any(|(k, v)| k == "decimals" && v == "9"));
    }

    #[test]
    fn test_program_id() {
        let parser = Token2022ProgramParser::new();
        let expected_id = Pubkey::from_str_const(TOKEN2022_PROGRAM_ID);
        assert_eq!(parser.program_id(), &expected_id);
    }
}
