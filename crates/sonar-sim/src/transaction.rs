use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bs58::decode::Error as Base58Error;
use serde::Serialize;
use solana_message::VersionedMessage;
use solana_pubkey::Pubkey;
use solana_transaction::versioned::{TransactionVersion, VersionedTransaction};

use crate::error::{Result, SonarSimError};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RawTransactionEncoding {
    Base58,
    Base64,
}

impl fmt::Display for RawTransactionEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Base58 => f.write_str("base58"),
            Self::Base64 => f.write_str("base64"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParsedTransaction {
    pub encoding: RawTransactionEncoding,
    pub version: TransactionVersion,
    pub transaction: VersionedTransaction,
    pub account_plan: MessageAccountPlan,
}

#[derive(Debug, Clone)]
pub struct MessageAccountPlan {
    pub static_accounts: Vec<Pubkey>,
    pub address_lookups: Vec<AddressLookupPlan>,
}

impl MessageAccountPlan {
    pub fn from_transaction(tx: &VersionedTransaction) -> Self {
        let static_accounts = tx.message.static_account_keys().to_vec();
        let address_lookups = build_address_lookup_plan(&tx.message);
        Self { static_accounts, address_lookups }
    }
}

#[derive(Debug, Clone)]
pub struct AddressLookupPlan {
    pub account_key: Pubkey,
    pub writable_indexes: Vec<u8>,
    pub readonly_indexes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct LookupLocation {
    pub table_account: Pubkey,
    pub table_index: u8,
    pub writable: bool,
}

pub fn parse_raw_transaction(raw: &str) -> Result<ParsedTransaction> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(SonarSimError::TransactionParse {
            reason: "Raw transaction string is empty".into(),
        });
    }

    let mut errors = Vec::new();

    for encoding in [RawTransactionEncoding::Base64, RawTransactionEncoding::Base58] {
        match decode_bytes(trimmed, encoding) {
            Ok(bytes) => match bincode::deserialize::<VersionedTransaction>(&bytes) {
                Ok(transaction) => {
                    let version = transaction.version();
                    let account_plan = MessageAccountPlan::from_transaction(&transaction);
                    return Ok(ParsedTransaction { encoding, version, transaction, account_plan });
                }
                Err(err) => errors.push(format!(
                    "{} deserialization failed: {err}",
                    match encoding {
                        RawTransactionEncoding::Base58 => "Base58",
                        RawTransactionEncoding::Base64 => "Base64",
                    }
                )),
            },
            Err(err) => errors.push(err.to_string()),
        }
    }

    let merged = errors.join("; ");
    Err(SonarSimError::TransactionParse {
        reason: format!("Failed to parse raw transaction: {merged}"),
    })
}

fn build_address_lookup_plan(message: &VersionedMessage) -> Vec<AddressLookupPlan> {
    message
        .address_table_lookups()
        .map(|lookups| {
            lookups
                .iter()
                .map(|lookup| AddressLookupPlan {
                    account_key: lookup.account_key,
                    writable_indexes: lookup.writable_indexes.clone(),
                    readonly_indexes: lookup.readonly_indexes.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Builds lookup table account position mapping table.
///
/// According to Solana v0 message ordering rules, all writable lookup
/// addresses from all tables must come first, followed by all readonly
/// lookup addresses from all tables. This function preserves that order.
pub fn build_lookup_locations(plan: &[AddressLookupPlan]) -> Vec<LookupLocation> {
    let mut locations = Vec::new();

    for entry in plan {
        for &idx in &entry.writable_indexes {
            locations.push(LookupLocation {
                table_account: entry.account_key,
                table_index: idx,
                writable: true,
            });
        }
    }

    for entry in plan {
        for &idx in &entry.readonly_indexes {
            locations.push(LookupLocation {
                table_account: entry.account_key,
                table_index: idx,
                writable: false,
            });
        }
    }

    locations
}

/// Mutate a transaction in-place to swap account pubkeys within specific instructions.
///
/// Returns the updated `MessageAccountPlan` reflecting any newly appended keys.
///
/// When a new pubkey must be added to the message's account keys:
/// - **Writable**: inserted just before the read-only non-signer section,
///   and all existing indices ≥ the insertion point are shifted by +1.
/// - **Read-only**: appended at the end and `num_readonly_unsigned_accounts`
///   is incremented so the new key falls into the read-only range.
pub fn apply_ix_account_swaps(
    tx: &mut VersionedTransaction,
    swaps: &[crate::types::InstructionAccountSwap],
) -> Result<MessageAccountPlan> {
    for swap in swaps {
        let instructions = tx.message.instructions();
        if swap.instruction_index >= instructions.len() {
            return Err(SonarSimError::Validation {
                reason: format!(
                    "Instruction index {} is out of range (transaction has {} instructions)",
                    swap.instruction_index,
                    instructions.len()
                ),
            });
        }
        if swap.account_position >= instructions[swap.instruction_index].accounts.len() {
            return Err(SonarSimError::Validation {
                reason: format!(
                    "Account position {} is out of range for instruction {} (has {} accounts)",
                    swap.account_position,
                    swap.instruction_index,
                    instructions[swap.instruction_index].accounts.len()
                ),
            });
        }

        // Find or insert the new pubkey in the static account keys
        let account_keys = match &tx.message {
            VersionedMessage::Legacy(m) => &m.account_keys,
            VersionedMessage::V0(m) => &m.account_keys,
        };
        let new_index = if let Some(pos) = account_keys.iter().position(|k| *k == swap.new_pubkey) {
            // Key exists — ensure its writability matches what was requested.
            ensure_account_writability(&mut tx.message, pos, swap.writable)
        } else {
            let total = account_keys.len();
            if total > u8::MAX as usize {
                return Err(SonarSimError::Validation {
                    reason: "Cannot add more account keys: would exceed u8 index limit".into(),
                });
            }
            if swap.writable {
                // Insert before the read-only non-signer section to keep the
                // new key in the writable range.
                let readonly_unsigned = tx.message.header().num_readonly_unsigned_accounts as usize;
                let insert_pos = total - readonly_unsigned;

                match &mut tx.message {
                    VersionedMessage::Legacy(m) => {
                        m.account_keys.insert(insert_pos, swap.new_pubkey);
                        shift_indices(&mut m.instructions, insert_pos);
                    }
                    VersionedMessage::V0(m) => {
                        m.account_keys.insert(insert_pos, swap.new_pubkey);
                        shift_indices(&mut m.instructions, insert_pos);
                    }
                }
                insert_pos
            } else {
                // Append at end and widen the read-only section.
                // In v0 messages this insertion happens before lookup-derived
                // account indices, so all existing indices >= `total` must shift.
                match &mut tx.message {
                    VersionedMessage::Legacy(m) => {
                        m.account_keys.push(swap.new_pubkey);
                        m.header.num_readonly_unsigned_accounts += 1;
                        shift_indices(&mut m.instructions, total);
                    }
                    VersionedMessage::V0(m) => {
                        m.account_keys.push(swap.new_pubkey);
                        m.header.num_readonly_unsigned_accounts += 1;
                        shift_indices(&mut m.instructions, total);
                    }
                }
                total
            }
        };

        // Update the instruction's account index
        match &mut tx.message {
            VersionedMessage::Legacy(m) => {
                m.instructions[swap.instruction_index].accounts[swap.account_position] =
                    new_index as u8;
            }
            VersionedMessage::V0(m) => {
                m.instructions[swap.instruction_index].accounts[swap.account_position] =
                    new_index as u8;
            }
        }
    }

    Ok(MessageAccountPlan::from_transaction(tx))
}

/// Shift all instruction account indices and program_id_index values that are
/// >= `insert_pos` by +1, to account for a key insertion in the account_keys vec.
fn shift_indices(
    instructions: &mut [solana_message::compiled_instruction::CompiledInstruction],
    insert_pos: usize,
) {
    let threshold = insert_pos as u8;
    for ix in instructions.iter_mut() {
        if ix.program_id_index >= threshold {
            ix.program_id_index += 1;
        }
        for acc in &mut ix.accounts {
            if *acc >= threshold {
                *acc += 1;
            }
        }
    }
}

/// Ensure an existing account key at `pos` has the requested writability.
///
/// If the key is already at the correct writability level, returns `pos` unchanged.
/// Otherwise swaps it with the account at the writability boundary and adjusts
/// the message header, returning the key's new position.
///
/// Signer accounts (pos < num_required_signatures) are left unchanged because
/// their writability is governed by `num_readonly_signed_accounts` and moving
/// them would invalidate signature verification.
fn ensure_account_writability(message: &mut VersionedMessage, pos: usize, writable: bool) -> usize {
    let num_sigs = message.header().num_required_signatures as usize;
    if pos < num_sigs {
        // Don't relocate signer accounts.
        return pos;
    }

    let total = message.static_account_keys().len();
    let readonly_unsigned = message.header().num_readonly_unsigned_accounts as usize;
    let readonly_start = total - readonly_unsigned;
    let is_currently_writable = pos < readonly_start;

    if is_currently_writable == writable {
        return pos;
    }

    if writable {
        // Move from read-only to writable: swap with the first read-only non-signer
        // (at `readonly_start`) and shrink the read-only section by 1.
        swap_account_positions(message, pos, readonly_start);
        match message {
            VersionedMessage::Legacy(m) => m.header.num_readonly_unsigned_accounts -= 1,
            VersionedMessage::V0(m) => m.header.num_readonly_unsigned_accounts -= 1,
        }
        readonly_start
    } else {
        // Move from writable to read-only: swap with the last writable non-signer
        // (at `readonly_start - 1`) and grow the read-only section by 1.
        let last_writable = readonly_start - 1;
        swap_account_positions(message, pos, last_writable);
        match message {
            VersionedMessage::Legacy(m) => m.header.num_readonly_unsigned_accounts += 1,
            VersionedMessage::V0(m) => m.header.num_readonly_unsigned_accounts += 1,
        }
        last_writable
    }
}

/// Swap two positions in the account_keys and update all instruction references.
fn swap_account_positions(message: &mut VersionedMessage, a: usize, b: usize) {
    if a == b {
        return;
    }
    let (a_u8, b_u8) = (a as u8, b as u8);
    match message {
        VersionedMessage::Legacy(m) => {
            m.account_keys.swap(a, b);
            remap_indices(&mut m.instructions, a_u8, b_u8);
        }
        VersionedMessage::V0(m) => {
            m.account_keys.swap(a, b);
            remap_indices(&mut m.instructions, a_u8, b_u8);
        }
    }
}

/// Swap all instruction references between indices `a` and `b`.
fn remap_indices(
    instructions: &mut [solana_message::compiled_instruction::CompiledInstruction],
    a: u8,
    b: u8,
) {
    for ix in instructions.iter_mut() {
        if ix.program_id_index == a {
            ix.program_id_index = b;
        } else if ix.program_id_index == b {
            ix.program_id_index = a;
        }
        for acc in &mut ix.accounts {
            if *acc == a {
                *acc = b;
            } else if *acc == b {
                *acc = a;
            }
        }
    }
}

/// Patch instruction data in-place at a given offset.
pub fn apply_ix_data_patches(
    tx: &mut VersionedTransaction,
    patches: &[crate::types::InstructionDataPatch],
) -> Result<()> {
    for patch in patches {
        let ix_count = tx.message.instructions().len();
        if patch.instruction_index >= ix_count {
            return Err(SonarSimError::Validation {
                reason: format!(
                    "Instruction index {} is out of range (transaction has {} instructions)",
                    patch.instruction_index, ix_count
                ),
            });
        }

        let ix_data_len = tx.message.instructions()[patch.instruction_index].data.len();
        let end = patch.offset.checked_add(patch.data.len()).ok_or_else(|| {
            SonarSimError::Validation { reason: "Patch offset + length overflows".into() }
        })?;
        if end > ix_data_len {
            return Err(SonarSimError::Validation {
                reason: format!(
                    "Patch range {}..{} exceeds instruction {} data length ({})",
                    patch.offset, end, patch.instruction_index, ix_data_len
                ),
            });
        }

        match &mut tx.message {
            VersionedMessage::Legacy(m) => {
                m.instructions[patch.instruction_index].data[patch.offset..end]
                    .copy_from_slice(&patch.data);
            }
            VersionedMessage::V0(m) => {
                m.instructions[patch.instruction_index].data[patch.offset..end]
                    .copy_from_slice(&patch.data);
            }
        }
    }
    Ok(())
}

fn decode_bytes(input: &str, encoding: RawTransactionEncoding) -> Result<Vec<u8>> {
    match encoding {
        RawTransactionEncoding::Base58 => {
            bs58::decode(input).into_vec().map_err(|err| map_base58_error(input, err))
        }
        RawTransactionEncoding::Base64 => BASE64_STANDARD.decode(input.as_bytes()).map_err(|err| {
            SonarSimError::TransactionParse { reason: format!("Base64 decode failed: {err}") }
        }),
    }
}

fn map_base58_error(input: &str, err: Base58Error) -> SonarSimError {
    let base_message = match err {
        Base58Error::InvalidCharacter { character, index } => {
            format!(
                "Base58 decode failed: position {index} contains invalid character `{character}`"
            )
        }
        other => format!("Base58 decode failed: {other}"),
    };

    if input.contains(['+', '/', '=']) {
        SonarSimError::TransactionParse {
            reason: format!(
                "{base_message}. Base64 characteristic characters detected, you may need to try Base64 encoding"
            ),
        }
    } else {
        SonarSimError::TransactionParse { reason: base_message }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use solana_hash::Hash;
    use solana_keypair::Keypair;
    use solana_message::MessageHeader;
    use solana_message::compiled_instruction::CompiledInstruction;
    use solana_message::v0::{Message as MessageV0, MessageAddressTableLookup};
    use solana_message::{Message, VersionedMessage};
    use solana_pubkey::Pubkey;
    use solana_signature::Signature;
    use solana_signer::Signer;
    use solana_system_interface::instruction as system_instruction;
    use solana_transaction::Transaction;

    fn sample_transaction() -> (VersionedTransaction, Pubkey) {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();
        let blockhash = Hash::new_unique();
        let instruction = system_instruction::transfer(&payer.pubkey(), &recipient, 42);
        let message = Message::new(&[instruction], Some(&payer.pubkey()));
        let transaction = Transaction::new(&[&payer], message, blockhash);
        (VersionedTransaction::from(transaction), payer.pubkey())
    }

    fn sample_v0_transaction_with_lookup_accounts() -> (VersionedTransaction, Pubkey) {
        let payer = Pubkey::new_unique();
        let program = Pubkey::new_unique();
        let lookup_table = Pubkey::new_unique();
        let message = MessageV0 {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![payer, program],
            recent_blockhash: Hash::new_unique(),
            instructions: vec![CompiledInstruction {
                // Program IDs must be static keys in v0.
                program_id_index: 1,
                // These point to looked-up accounts:
                // 2 -> first writable lookup account, 3 -> first readonly lookup account.
                accounts: vec![2, 3],
                data: vec![],
            }],
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: lookup_table,
                writable_indexes: vec![0],
                readonly_indexes: vec![1],
            }],
        };
        (
            VersionedTransaction {
                signatures: vec![Signature::default()],
                message: VersionedMessage::V0(message),
            },
            payer,
        )
    }

    #[test]
    fn parse_base64_transaction() {
        let (versioned, payer) = sample_transaction();
        let bytes = bincode::serialize(&versioned).unwrap();
        let base64 = BASE64_STANDARD.encode(&bytes);

        let parsed = parse_raw_transaction(&base64).expect("parse base64");
        assert_eq!(parsed.encoding, RawTransactionEncoding::Base64);
        assert_eq!(parsed.account_plan.static_accounts.len(), 3);
        assert_eq!(parsed.account_plan.static_accounts[0], payer);
    }

    #[test]
    fn parse_base58_transaction() {
        let (versioned, _) = sample_transaction();
        let bytes = bincode::serialize(&versioned).unwrap();
        let base58 = bs58::encode(&bytes).into_string();

        let parsed = parse_raw_transaction(&base58).expect("parse base58");
        assert_eq!(parsed.encoding, RawTransactionEncoding::Base58);
    }

    #[test]
    fn raw_transaction_encoding_display() {
        assert_eq!(RawTransactionEncoding::Base58.to_string(), "base58");
        assert_eq!(RawTransactionEncoding::Base64.to_string(), "base64");
    }

    #[test]
    fn test_build_lookup_locations_ordering() {
        let table1 = Pubkey::new_unique();
        let table2 = Pubkey::new_unique();

        let plan = vec![
            AddressLookupPlan {
                account_key: table1,
                writable_indexes: vec![0, 1],
                readonly_indexes: vec![2, 3],
            },
            AddressLookupPlan {
                account_key: table2,
                writable_indexes: vec![5, 6],
                readonly_indexes: vec![7],
            },
        ];

        let locations = build_lookup_locations(&plan);

        assert_eq!(locations.len(), 7);

        assert_eq!(locations[0].table_account, table1);
        assert_eq!(locations[0].table_index, 0);
        assert!(locations[0].writable);

        assert_eq!(locations[1].table_account, table1);
        assert_eq!(locations[1].table_index, 1);
        assert!(locations[1].writable);

        assert_eq!(locations[2].table_account, table2);
        assert_eq!(locations[2].table_index, 5);
        assert!(locations[2].writable);

        assert_eq!(locations[3].table_account, table2);
        assert_eq!(locations[3].table_index, 6);
        assert!(locations[3].writable);

        assert_eq!(locations[4].table_account, table1);
        assert_eq!(locations[4].table_index, 2);
        assert!(!locations[4].writable);

        assert_eq!(locations[5].table_account, table1);
        assert_eq!(locations[5].table_index, 3);
        assert!(!locations[5].writable);

        assert_eq!(locations[6].table_account, table2);
        assert_eq!(locations[6].table_index, 7);
        assert!(!locations[6].writable);
    }

    #[test]
    fn test_build_lookup_locations_empty() {
        let locations = build_lookup_locations(&[]);
        assert_eq!(locations.len(), 0);
    }

    #[test]
    fn apply_ix_account_swaps_writable_inserts_before_readonly() {
        let (mut tx, _payer) = sample_transaction();
        // sample tx: account_keys = [payer, recipient, system_program]
        // header: num_required_signatures=1, num_readonly_signed=0, num_readonly_unsigned=1
        // (system_program is readonly unsigned)
        // Layout: [payer(ws)] [recipient(wu)] [system_program(ru)]
        let new_key = Pubkey::new_unique();
        let swaps = vec![crate::types::InstructionAccountSwap {
            instruction_index: 0,
            account_position: 1,
            new_pubkey: new_key,
            writable: true,
        }];
        let plan = apply_ix_account_swaps(&mut tx, &swaps).unwrap();
        // Writable key inserted at position 2 (before readonly system_program),
        // system_program shifted to index 3.
        assert_eq!(plan.static_accounts.len(), 4);
        assert_eq!(plan.static_accounts[2], new_key);
        // system_program moved to index 3
        assert_eq!(tx.message.instructions()[0].accounts[1], 2);
        // program_id_index should have shifted from 2 to 3
        assert_eq!(tx.message.instructions()[0].program_id_index, 3);
    }

    #[test]
    fn apply_ix_account_swaps_readonly_appends_at_end() {
        let (mut tx, _payer) = sample_transaction();
        let new_key = Pubkey::new_unique();
        let swaps = vec![crate::types::InstructionAccountSwap {
            instruction_index: 0,
            account_position: 1,
            new_pubkey: new_key,
            writable: false,
        }];
        let plan = apply_ix_account_swaps(&mut tx, &swaps).unwrap();
        // Read-only key appended at the end (index 3)
        assert_eq!(plan.static_accounts.len(), 4);
        assert_eq!(plan.static_accounts[3], new_key);
        assert_eq!(tx.message.instructions()[0].accounts[1], 3);
        // num_readonly_unsigned should have increased from 1 to 2
        assert_eq!(tx.message.header().num_readonly_unsigned_accounts, 2);
    }

    #[test]
    fn apply_ix_account_swaps_reuses_existing_key() {
        let (mut tx, payer) = sample_transaction();
        let swaps = vec![crate::types::InstructionAccountSwap {
            instruction_index: 0,
            account_position: 1,
            new_pubkey: payer,
            writable: true,
        }];
        let plan = apply_ix_account_swaps(&mut tx, &swaps).unwrap();
        assert_eq!(plan.static_accounts.len(), 3);
        assert_eq!(tx.message.instructions()[0].accounts[1], 0);
    }

    #[test]
    fn apply_ix_account_swaps_promotes_readonly_to_writable() {
        let (mut tx, _payer) = sample_transaction();
        // sample tx: [payer(ws), recipient(wu), system_program(ru)]
        // system_program is at index 2, readonly unsigned.
        // Swap instruction's account position 1 to system_program with :w.
        let system_program = tx.message.static_account_keys()[2];
        let swaps = vec![crate::types::InstructionAccountSwap {
            instruction_index: 0,
            account_position: 1,
            new_pubkey: system_program,
            writable: true,
        }];
        let plan = apply_ix_account_swaps(&mut tx, &swaps).unwrap();
        // system_program should now be writable. Since it was the only readonly
        // non-signer, num_readonly_unsigned goes from 1 to 0.
        assert_eq!(tx.message.header().num_readonly_unsigned_accounts, 0);
        // No keys added or removed.
        assert_eq!(plan.static_accounts.len(), 3);
    }

    #[test]
    fn apply_ix_account_swaps_demotes_writable_to_readonly() {
        let (mut tx, _payer) = sample_transaction();
        // sample tx: [payer(ws), recipient(wu), system_program(ru)]
        // recipient is at index 1, writable unsigned.
        let recipient = tx.message.static_account_keys()[1];
        let swaps = vec![crate::types::InstructionAccountSwap {
            instruction_index: 0,
            account_position: 0,
            new_pubkey: recipient,
            writable: false,
        }];
        let plan = apply_ix_account_swaps(&mut tx, &swaps).unwrap();
        // recipient moved to readonly. num_readonly_unsigned goes from 1 to 2.
        assert_eq!(tx.message.header().num_readonly_unsigned_accounts, 2);
        assert_eq!(plan.static_accounts.len(), 3);
    }

    #[test]
    fn apply_ix_account_swaps_rejects_invalid_ix_index() {
        let (mut tx, _) = sample_transaction();
        let swaps = vec![crate::types::InstructionAccountSwap {
            instruction_index: 99,
            account_position: 0,
            new_pubkey: Pubkey::new_unique(),
            writable: true,
        }];
        let err = apply_ix_account_swaps(&mut tx, &swaps).unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn apply_ix_account_swaps_rejects_invalid_account_position() {
        let (mut tx, _) = sample_transaction();
        let swaps = vec![crate::types::InstructionAccountSwap {
            instruction_index: 0,
            account_position: 99,
            new_pubkey: Pubkey::new_unique(),
            writable: true,
        }];
        let err = apply_ix_account_swaps(&mut tx, &swaps).unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn apply_ix_account_swaps_v0_writable_insert_shifts_lookup_indices() {
        let (mut tx, _payer) = sample_v0_transaction_with_lookup_accounts();
        let new_key = Pubkey::new_unique();
        let swaps = vec![crate::types::InstructionAccountSwap {
            instruction_index: 0,
            account_position: 0,
            new_pubkey: new_key,
            writable: true,
        }];

        let plan = apply_ix_account_swaps(&mut tx, &swaps).unwrap();

        // New key inserted into static keys before readonly unsigned section:
        // [payer, new_key, program]
        assert_eq!(plan.static_accounts.len(), 3);
        assert_eq!(plan.static_accounts[1], new_key);

        // Program index shifts (1 -> 2); lookup indices (2,3) shift to (3,4),
        // then account_position 0 is rewritten to the new key index 1.
        let ix = &tx.message.instructions()[0];
        assert_eq!(ix.program_id_index, 2);
        assert_eq!(ix.accounts, vec![1, 4]);
    }

    #[test]
    fn apply_ix_account_swaps_v0_readonly_append_shifts_lookup_indices() {
        let (mut tx, _payer) = sample_v0_transaction_with_lookup_accounts();
        let new_key = Pubkey::new_unique();
        let swaps = vec![crate::types::InstructionAccountSwap {
            instruction_index: 0,
            account_position: 1,
            new_pubkey: new_key,
            writable: false,
        }];

        let plan = apply_ix_account_swaps(&mut tx, &swaps).unwrap();

        // New readonly unsigned static key is appended at index 2.
        assert_eq!(plan.static_accounts.len(), 3);
        assert_eq!(plan.static_accounts[2], new_key);
        assert_eq!(tx.message.header().num_readonly_unsigned_accounts, 2);

        // Existing lookup-derived indices must shift by +1 (2,3 -> 3,4),
        // then account_position 1 is rewritten to 2.
        let ix = &tx.message.instructions()[0];
        assert_eq!(ix.program_id_index, 1);
        assert_eq!(ix.accounts, vec![3, 2]);
    }

    #[test]
    fn apply_ix_data_patches_basic() {
        let (mut tx, _) = sample_transaction();
        let original_data = tx.message.instructions()[0].data.clone();
        assert!(original_data.len() >= 4);
        let patches = vec![crate::types::InstructionDataPatch {
            instruction_index: 0,
            offset: 0,
            data: vec![0xaa, 0xbb, 0xcc, 0xdd],
        }];
        apply_ix_data_patches(&mut tx, &patches).unwrap();
        let patched = &tx.message.instructions()[0].data;
        assert_eq!(&patched[..4], &[0xaa, 0xbb, 0xcc, 0xdd]);
        assert_eq!(&patched[4..], &original_data[4..]);
    }

    #[test]
    fn apply_ix_data_patches_rejects_out_of_range_ix() {
        let (mut tx, _) = sample_transaction();
        let patches = vec![crate::types::InstructionDataPatch {
            instruction_index: 99,
            offset: 0,
            data: vec![0x00],
        }];
        let err = apply_ix_data_patches(&mut tx, &patches).unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn apply_ix_data_patches_rejects_out_of_bounds_offset() {
        let (mut tx, _) = sample_transaction();
        let data_len = tx.message.instructions()[0].data.len();
        let patches = vec![crate::types::InstructionDataPatch {
            instruction_index: 0,
            offset: data_len,
            data: vec![0x00],
        }];
        let err = apply_ix_data_patches(&mut tx, &patches).unwrap_err();
        assert!(err.to_string().contains("exceeds"));
    }

    #[test]
    fn test_build_lookup_locations_single_table() {
        let table = Pubkey::new_unique();
        let plan = vec![AddressLookupPlan {
            account_key: table,
            writable_indexes: vec![10],
            readonly_indexes: vec![20, 21],
        }];

        let locations = build_lookup_locations(&plan);

        assert_eq!(locations.len(), 3);
        assert_eq!(locations[0].table_index, 10);
        assert!(locations[0].writable);
        assert_eq!(locations[1].table_index, 20);
        assert!(!locations[1].writable);
        assert_eq!(locations[2].table_index, 21);
        assert!(!locations[2].writable);
    }
}
