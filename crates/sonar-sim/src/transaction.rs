use std::collections::HashMap;
use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bs58::decode::Error as Base58Error;
use serde::Serialize;
use solana_instruction::Instruction;
use solana_message::MessageHeader;
use solana_message::VersionedMessage;
use solana_message::compiled_instruction::CompiledInstruction;
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_transaction::versioned::{TransactionVersion, VersionedTransaction};

use crate::error::{Result, SonarSimError};

/// Internal extension trait for mutable access to the shared fields of
/// `VersionedMessage`. Both `Legacy` and `V0` messages have identical
/// `account_keys`, `instructions`, and `header` layouts.
pub(crate) trait VersionedMessageExt {
    fn account_keys(&self) -> &Vec<Pubkey>;
    fn account_keys_mut(&mut self) -> &mut Vec<Pubkey>;
    fn instructions_mut(&mut self) -> &mut Vec<CompiledInstruction>;
    fn header_mut(&mut self) -> &mut MessageHeader;
}

impl VersionedMessageExt for VersionedMessage {
    fn account_keys(&self) -> &Vec<Pubkey> {
        match self {
            VersionedMessage::Legacy(m) => &m.account_keys,
            VersionedMessage::V0(m) => &m.account_keys,
        }
    }

    fn account_keys_mut(&mut self) -> &mut Vec<Pubkey> {
        match self {
            VersionedMessage::Legacy(m) => &mut m.account_keys,
            VersionedMessage::V0(m) => &mut m.account_keys,
        }
    }

    fn instructions_mut(&mut self) -> &mut Vec<CompiledInstruction> {
        match self {
            VersionedMessage::Legacy(m) => &mut m.instructions,
            VersionedMessage::V0(m) => &mut m.instructions,
        }
    }

    fn header_mut(&mut self) -> &mut MessageHeader {
        match self {
            VersionedMessage::Legacy(m) => &mut m.header,
            VersionedMessage::V0(m) => &mut m.header,
        }
    }
}

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

/// Apply a list of whole-instruction mutations (insert / remove) to a
/// transaction in-place.
///
/// Returns the updated `MessageAccountPlan` reflecting any keys added to the
/// message's static `account_keys` table.
///
/// Operations apply in the order given; indices and positions are interpreted
/// against the instruction list at the time each op runs. To express
/// positions relative to the pre-mutation list, list ops in descending
/// position order.
///
/// Intended to run *before* account-level ([`apply_ix_account_ops`]) and data
/// ([`apply_ix_data_patches`]) mutations, so those can target the
/// post-restructure instruction list.
///
/// `Remove` does not garbage-collect account keys (matching account-level
/// `Remove`). `Insert` merges the new instruction's accounts into the message
/// with **union** privileges: an existing key is never demoted, only promoted,
/// so the inserted instruction cannot weaken privileges an existing
/// instruction already requires. New non-signer accounts are appended (writable
/// before the read-only section), and new signer accounts are inserted into the
/// signer section with a placeholder signature. An account already present as
/// a non-signer cannot be promoted to signer.
///
/// **Limitation (v0 messages with ALTs):** like [`apply_ix_account_ops`],
/// duplicate detection only covers static account keys.
pub fn apply_instruction_ops(
    tx: &mut VersionedTransaction,
    ops: &[crate::types::InstructionOp],
) -> Result<MessageAccountPlan> {
    for op in ops {
        let count = tx.message.instructions().len();
        match op {
            crate::types::InstructionOp::Remove { index } => {
                if *index >= count {
                    return Err(SonarSimError::Validation {
                        reason: format!(
                            "Instruction index {index} is out of range \
                             (transaction has {count} instructions)"
                        ),
                    });
                }
                tx.message.instructions_mut().remove(*index);
            }
            crate::types::InstructionOp::Insert { position, instruction } => {
                if *position > count {
                    return Err(SonarSimError::Validation {
                        reason: format!(
                            "Insert position {position} is out of range for the \
                             instruction list (has {count} instructions; valid insert \
                             positions are 0..={count})"
                        ),
                    });
                }
                let compiled = compile_instruction(tx, instruction)?;
                tx.message.instructions_mut().insert(*position, compiled);
            }
        }
    }

    Ok(MessageAccountPlan::from_transaction(tx))
}

/// Compile a decoded [`Instruction`] into a [`CompiledInstruction`] against
/// the transaction's current message, inserting any missing account keys
/// (including new signers) and returning the resolved program/account indices.
///
/// All keys are inserted/ensured *before* resolving indices, so the returned
/// indices are stable regardless of the order in which keys get added.
fn compile_instruction(
    tx: &mut VersionedTransaction,
    instruction: &Instruction,
) -> Result<CompiledInstruction> {
    // Phase 1: merge every referenced key into the message with **union**
    // privileges — an existing key is never demoted, only promoted — so the
    // inserted instruction cannot weaken privileges an existing instruction
    // already requires (Solana message privileges are global across all
    // instructions). Programs are always read-only non-signers.
    //
    // Aggregate the signer/writable requirements PER PUBKEY before mutating,
    // because Solana unions duplicate account privileges within an
    // instruction regardless of meta order. Without aggregation, a pubkey
    // listed first as a non-signer and later as a signer would be inserted as
    // a non-signer by the first meta and then rejected ("unsupported
    // promotion") by the second. `order` preserves first-seen order purely
    // for deterministic placement; actual position is governed by privilege
    // region.
    let mut order: Vec<Pubkey> = Vec::new();
    let mut requirements: HashMap<Pubkey, (bool, bool)> = HashMap::new();
    // Program first as an implicit read-only non-signer meta; any explicit
    // account-list occurrence of the same key unions on top of it.
    if requirements.insert(instruction.program_id, (false, false)).is_none() {
        order.push(instruction.program_id);
    }
    for account in &instruction.accounts {
        match requirements.get_mut(&account.pubkey) {
            Some((want_signer, want_writable)) => {
                *want_signer |= account.is_signer;
                *want_writable |= account.is_writable;
            }
            None => {
                requirements.insert(account.pubkey, (account.is_signer, account.is_writable));
                order.push(account.pubkey);
            }
        }
    }
    for pubkey in &order {
        let (want_signer, want_writable) = requirements[pubkey];
        merge_or_insert_account_key(tx, pubkey, want_signer, want_writable)?;
    }

    // Phase 2: re-resolve every index by pubkey lookup so the result is
    // unaffected by any index shifting that the insertions performed above.
    let keys = tx.message.account_keys();
    let program_id_index =
        keys.iter().position(|k| *k == instruction.program_id).ok_or_else(|| {
            SonarSimError::Validation {
                reason: "Internal error: program key missing after insert".into(),
            }
        })?;
    let accounts = instruction
        .accounts
        .iter()
        .map(|account| {
            keys.iter().position(|k| *k == account.pubkey).map(|i| i as u8).ok_or_else(|| {
                SonarSimError::Validation {
                    reason: "Internal error: account key missing after insert".into(),
                }
            })
        })
        .collect::<Result<Vec<u8>>>()?;

    Ok(CompiledInstruction {
        program_id_index: program_id_index as u8,
        accounts,
        data: instruction.data.clone(),
    })
}

/// Merge an account reference into the message applying **union** privileges.
///
/// Solana message privileges (signer / writable) are global across all
/// instructions: an account is a signer if *any* instruction needs it as one,
/// and writable if *any* instruction needs it writable. Consequently an
/// existing key is **never demoted** — only promoted — when a new instruction
/// references it. This is the correctness rule used while compiling an inserted
/// instruction, so it cannot silently strip write access that an existing
/// instruction already requires.
///
/// `want_signer` / `want_writable` express only what *this* reference needs;
/// the resulting privileges are the union with whatever the key already has.
///
/// Returns the key's resulting index (which may change if a promotion swaps it
/// to a different position).
fn merge_or_insert_account_key(
    tx: &mut VersionedTransaction,
    pubkey: &Pubkey,
    want_signer: bool,
    want_writable: bool,
) -> Result<usize> {
    let message = &mut tx.message;
    let Some(pos) = message.account_keys().iter().position(|k| *k == *pubkey) else {
        // Absent: insert with the exact privileges this reference needs.
        return if want_signer {
            insert_signer_account_key(tx, pubkey, want_writable)
        } else {
            find_or_insert_account_key(&mut tx.message, pubkey, want_writable)
        };
    };

    let num_sigs = message.header().num_required_signatures as usize;
    let cur_signer = pos < num_sigs;
    // Union of signer-ness: promoting a non-signer to a signer is unsupported
    // (it requires rebuilding the header and signature layout).
    if want_signer && !cur_signer {
        let key = message.static_account_keys()[pos];
        return Err(SonarSimError::Validation {
            reason: format!(
                "Cannot declare account {key} as a signer for the inserted instruction: \
                 it already appears in the message as a non-signer account. Promoting a \
                 non-signer to a signer is not supported."
            ),
        });
    }

    // Union of writability: never demote. Only promote readonly -> writable.
    let cur_writable = account_is_writable(message, pos);
    if cur_writable || !want_writable {
        // Already writable, or this reference doesn't need writable: keep the
        // current (possibly stronger) privileges untouched.
        return Ok(pos);
    }
    promote_to_writable(tx, pos, cur_signer)
}

/// Whether the key at `pos` is writable under the current message header.
///
/// Signers are writable in `[0, num_required_signatures -
/// num_readonly_signed_accounts)`; non-signers are writable before the
/// readonly-unsigned region.
fn account_is_writable(message: &VersionedMessage, pos: usize) -> bool {
    let header = message.header();
    let num_sigs = header.num_required_signatures as usize;
    let num_readonly_signed = header.num_readonly_signed_accounts as usize;
    if pos < num_sigs {
        pos < num_sigs - num_readonly_signed
    } else {
        let total = message.static_account_keys().len();
        let readonly_unsigned = header.num_readonly_unsigned_accounts as usize;
        pos < total - readonly_unsigned
    }
}

/// Promote an existing readonly key at `pos` to writable, relocating it into
/// the writable region and shrinking the corresponding readonly header count.
/// Signer and non-signer promotions are handled symmetrically. Returns the
/// key's new position after the swap.
///
/// Caller invariant: `pos` currently holds a readonly key (signer or
/// non-signer), so the relevant readonly header count is non-zero.
fn promote_to_writable(tx: &mut VersionedTransaction, pos: usize, signer: bool) -> Result<usize> {
    if signer {
        // Promote readonly-signer -> writable-signer: swap with the first
        // readonly signer (the writable/readonly signer boundary) and shrink
        // the readonly-signed region by one. Both positions are signers, so
        // the signature slots must move with their keys — use
        // [`swap_signer_positions`] to preserve the VersionedTransaction
        // invariant that signatures line up with signer keys.
        let (num_sigs, num_readonly_signed) = {
            let header = tx.message.header();
            (header.num_required_signatures as usize, header.num_readonly_signed_accounts as usize)
        };
        debug_assert!(
            num_readonly_signed > 0,
            "readonly-signer promotion requires a non-empty readonly-signed region"
        );
        let boundary = num_sigs - num_readonly_signed;
        swap_signer_positions(tx, pos, boundary);
        tx.message.header_mut().num_readonly_signed_accounts -= 1;
        Ok(boundary)
    } else {
        // Promote readonly non-signer -> writable non-signer: swap with the
        // first readonly non-signer and shrink the readonly-unsigned region.
        let message = &mut tx.message;
        let total = message.static_account_keys().len();
        let readonly_unsigned = message.header().num_readonly_unsigned_accounts as usize;
        debug_assert!(
            readonly_unsigned > 0,
            "readonly non-signer promotion requires a non-empty readonly-unsigned region"
        );
        let readonly_start = total - readonly_unsigned;
        swap_account_positions(message, pos, readonly_start);
        message.header_mut().num_readonly_unsigned_accounts -= 1;
        Ok(readonly_start)
    }
}

/// Insert a brand-new signer account into the message's signer section.
///
/// The caller MUST ensure `pubkey` is not already present — the present-key
/// union case is handled by [`merge_or_insert_account_key`]. The key is
/// inserted into the signer section (writable signers before readonly
/// signers), all existing instruction indices at or past the insertion point
/// are shifted by +1, the header's signer counts are bumped, and a placeholder
/// signature is appended at the matching signer slot so existing signatures
/// keep aligning with their signers.
fn insert_signer_account_key(
    tx: &mut VersionedTransaction,
    pubkey: &Pubkey,
    writable: bool,
) -> Result<usize> {
    let message = &mut tx.message;
    let total = message.account_keys().len();
    if total >= u8::MAX as usize {
        return Err(SonarSimError::Validation {
            reason: "Cannot add signer account key: would exceed u8 index limit".into(),
        });
    }

    let num_sigs = message.header().num_required_signatures as usize;
    let num_readonly_signed = message.header().num_readonly_signed_accounts as usize;
    // Writable signers precede readonly signers within the signer section.
    let insert_pos = if writable { num_sigs - num_readonly_signed } else { num_sigs };

    message.account_keys_mut().insert(insert_pos, *pubkey);
    shift_indices(message.instructions_mut(), insert_pos)?;
    message.header_mut().num_required_signatures += 1;
    if !writable {
        message.header_mut().num_readonly_signed_accounts += 1;
    }
    // Keep `signatures` length in sync with `num_required_signatures`, inserting
    // a placeholder at the same slot so existing signatures still align with
    // their signers. Simulation does not cryptographically verify these unless
    // the caller opts in, so a default signature is sufficient.
    tx.signatures.insert(insert_pos, Signature::default());
    Ok(insert_pos)
}

/// Apply a list of instruction-account mutations to a transaction in-place.
///
/// Returns the updated `MessageAccountPlan` reflecting any keys added to the
/// message's static `account_keys` table.
///
/// Operations apply in the order given; positions are interpreted against the
/// instruction's state at the time each op runs (not the original list). To
/// express positions relative to the pre-mutation list, list ops in
/// descending position order.
///
/// When a new pubkey must be added to the message's static `account_keys`:
/// - **Writable**: inserted just before the read-only non-signer section,
///   and all existing indices ≥ the insertion point are shifted by +1.
/// - **Read-only**: appended at the end and `num_readonly_unsigned_accounts`
///   is incremented so the new key falls into the read-only range.
///
/// `Remove` does **not** garbage-collect the underlying key from
/// `account_keys`; the key remains loadable by the runtime even if no
/// instruction references it (matching how `Patch` leaves the replaced key
/// in place).
///
/// **Limitation (v0 messages with ALTs):** duplicate detection only covers
/// static account keys. If a `Patch`/`Insert` pubkey is already loaded
/// through an address lookup table, it will be added as a static key too,
/// and the Solana runtime will reject the message with `AccountLoadedTwice`.
pub fn apply_ix_account_ops(
    tx: &mut VersionedTransaction,
    ops: &[crate::types::InstructionAccountOp],
) -> Result<MessageAccountPlan> {
    use crate::types::InstructionAccountOp;

    for op in ops {
        let ix_index = op.instruction_index();
        let pos = op.account_position();
        let ix_count = tx.message.instructions().len();
        if ix_index >= ix_count {
            return Err(SonarSimError::Validation {
                reason: format!(
                    "Instruction index {ix_index} is out of range \
                     (transaction has {ix_count} instructions)"
                ),
            });
        }
        let current_len = tx.message.instructions()[ix_index].accounts.len();

        match op {
            InstructionAccountOp::Patch { new_pubkey, writable, .. } => {
                if pos >= current_len {
                    return Err(SonarSimError::Validation {
                        reason: format!(
                            "Account position {pos} is out of range for instruction \
                             {ix_index} (has {current_len} accounts)"
                        ),
                    });
                }
                let new_index = find_or_insert_account_key(&mut tx.message, new_pubkey, *writable)?;
                tx.message.instructions_mut()[ix_index].accounts[pos] = new_index as u8;
            }
            InstructionAccountOp::Insert { new_pubkey, writable, .. } => {
                if pos > current_len {
                    return Err(SonarSimError::Validation {
                        reason: format!(
                            "Account position {pos} is out of range for instruction \
                             {ix_index} (has {current_len} accounts; valid insert \
                             positions are 0..={current_len})"
                        ),
                    });
                }
                let new_index = find_or_insert_account_key(&mut tx.message, new_pubkey, *writable)?;
                tx.message.instructions_mut()[ix_index].accounts.insert(pos, new_index as u8);
            }
            InstructionAccountOp::Remove { .. } => {
                if pos >= current_len {
                    return Err(SonarSimError::Validation {
                        reason: format!(
                            "Account position {pos} is out of range for instruction \
                             {ix_index} (has {current_len} accounts)"
                        ),
                    });
                }
                tx.message.instructions_mut()[ix_index].accounts.remove(pos);
            }
        }
    }

    Ok(MessageAccountPlan::from_transaction(tx))
}

/// Shift all instruction account indices and program_id_index values that are
/// >= `insert_pos` by +1, to account for a key insertion in the account_keys vec.
///
/// Returns an error if any index that needs shifting is already at `u8::MAX`,
/// since incrementing it would overflow (possible on dense v0 messages that
/// use the full compiled-index space via lookup accounts).
fn shift_indices(
    instructions: &mut [solana_message::compiled_instruction::CompiledInstruction],
    insert_pos: usize,
) -> Result<()> {
    let threshold = insert_pos as u8;
    // Pre-check: ensure no index at u8::MAX would need to shift.
    for ix in instructions.iter() {
        if ix.program_id_index >= threshold && ix.program_id_index == u8::MAX {
            return Err(SonarSimError::Validation {
                reason: "Cannot insert account key: shifting indices would overflow u8 \
                         (program_id_index is already at 255)"
                    .into(),
            });
        }
        for acc in &ix.accounts {
            if *acc >= threshold && *acc == u8::MAX {
                return Err(SonarSimError::Validation {
                    reason: "Cannot insert account key: shifting indices would overflow u8 \
                             (an instruction account index is already at 255)"
                        .into(),
                });
            }
        }
    }

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
    Ok(())
}

/// Find an existing account key, or insert a new one at the correct position.
///
/// Returns the index of the key in the static account keys list.
///
/// - If the key already exists, ensures its writability matches the `writable`
///   parameter (promoting/demoting as needed) and returns its (possibly new)
///   position.
/// - If the key does not exist:
///   - **Writable**: inserted just before the read-only non-signer section;
///     existing indices >= the insertion point are shifted by +1.
///   - **Read-only**: appended at the end and `num_readonly_unsigned_accounts`
///     is incremented.
fn find_or_insert_account_key(
    message: &mut VersionedMessage,
    pubkey: &Pubkey,
    writable: bool,
) -> Result<usize> {
    if let Some(pos) = message.account_keys().iter().position(|k| *k == *pubkey) {
        return ensure_account_writability(message, pos, writable);
    }

    let total = message.account_keys().len();
    if total > u8::MAX as usize {
        return Err(SonarSimError::Validation {
            reason: "Cannot add more account keys: would exceed u8 index limit".into(),
        });
    }

    if writable {
        let readonly_unsigned = message.header().num_readonly_unsigned_accounts as usize;
        let insert_pos = total - readonly_unsigned;
        message.account_keys_mut().insert(insert_pos, *pubkey);
        shift_indices(message.instructions_mut(), insert_pos)?;
        Ok(insert_pos)
    } else {
        message.account_keys_mut().push(*pubkey);
        message.header_mut().num_readonly_unsigned_accounts += 1;
        shift_indices(message.instructions_mut(), total)?;
        Ok(total)
    }
}

/// Ensure an existing account key at `pos` has the requested writability.
///
/// If the key is already at the correct writability level, returns `pos` unchanged.
/// Otherwise swaps it with the account at the writability boundary and adjusts
/// the message header, returning the key's new position.
///
/// Signer accounts (pos < num_required_signatures) cannot be relocated because
/// their writability is governed by `num_readonly_signed_accounts` and moving
/// them would invalidate signature verification. Returns an error if the
/// requested writability differs from the signer's current writability.
fn ensure_account_writability(
    message: &mut VersionedMessage,
    pos: usize,
    writable: bool,
) -> Result<usize> {
    let num_sigs = message.header().num_required_signatures as usize;
    if pos < num_sigs {
        // Signer accounts: check if the requested writability matches.
        let num_readonly_signed = message.header().num_readonly_signed_accounts as usize;
        let is_currently_writable = pos < (num_sigs - num_readonly_signed);
        if is_currently_writable != writable {
            let key = message.static_account_keys()[pos];
            return Err(SonarSimError::Validation {
                reason: format!(
                    "Cannot change writability of signer account {key}: \
                     signer writability is fixed by the message header"
                ),
            });
        }
        return Ok(pos);
    }

    let total = message.static_account_keys().len();
    let readonly_unsigned = message.header().num_readonly_unsigned_accounts as usize;
    let readonly_start = total - readonly_unsigned;
    let is_currently_writable = pos < readonly_start;

    if is_currently_writable == writable {
        return Ok(pos);
    }

    if writable {
        // Move from read-only to writable: swap with the first read-only non-signer
        // (at `readonly_start`) and shrink the read-only section by 1.
        swap_account_positions(message, pos, readonly_start);
        message.header_mut().num_readonly_unsigned_accounts -= 1;
        Ok(readonly_start)
    } else {
        // Move from writable to read-only: swap with the last writable non-signer
        // (at `readonly_start - 1`) and grow the read-only section by 1.
        let last_writable = readonly_start - 1;
        swap_account_positions(message, pos, last_writable);
        message.header_mut().num_readonly_unsigned_accounts += 1;
        Ok(last_writable)
    }
}

/// Swap two positions in the account_keys and update all instruction references.
///
/// Only safe for positions outside the signer region (`>=
/// num_required_signatures`): swapping signer keys without also swapping their
/// signature slots would violate the [`VersionedTransaction`] invariant that
/// `signatures[i]` belongs to the signer at `account_keys[i]`. Use
/// [`swap_signer_positions`] when either position is a signer.
fn swap_account_positions(message: &mut VersionedMessage, a: usize, b: usize) {
    if a == b {
        return;
    }
    let (a_u8, b_u8) = (a as u8, b as u8);
    message.account_keys_mut().swap(a, b);
    remap_indices(message.instructions_mut(), a_u8, b_u8);
}

/// Swap two signer positions, keeping `signatures` aligned with their signer
/// keys.
///
/// [`VersionedTransaction`] requires `signatures[i]` to be the signature for
/// the signer at `account_keys[i]` (for `i < num_required_signatures`). When
/// two signer keys are swapped in `account_keys`, their signature slots must
/// be swapped in lockstep, or downstream signature verification would
/// associate each signature with the wrong signer. Both `a` and `b` must be
/// in the signer region.
fn swap_signer_positions(tx: &mut VersionedTransaction, a: usize, b: usize) {
    if a == b {
        return;
    }
    debug_assert!(
        a < tx.signatures.len() && b < tx.signatures.len(),
        "swap_signer_positions indices must be within the signer region"
    );
    swap_account_positions(&mut tx.message, a, b);
    tx.signatures.swap(a, b);
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

        tx.message.instructions_mut()[patch.instruction_index].data[patch.offset..end]
            .copy_from_slice(&patch.data);
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
    fn apply_ix_account_patches_writable_inserts_before_readonly() {
        let (mut tx, _payer) = sample_transaction();
        // sample tx: account_keys = [payer, recipient, system_program]
        // header: num_required_signatures=1, num_readonly_signed=0, num_readonly_unsigned=1
        // (system_program is readonly unsigned)
        // Layout: [payer(ws)] [recipient(wu)] [system_program(ru)]
        let new_key = Pubkey::new_unique();
        let patches = vec![crate::types::InstructionAccountOp::Patch {
            instruction_index: 0,
            account_position: 1,
            new_pubkey: new_key,
            writable: true,
        }];
        let plan = apply_ix_account_ops(&mut tx, &patches).unwrap();
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
    fn apply_ix_account_patches_readonly_appends_at_end() {
        let (mut tx, _payer) = sample_transaction();
        let new_key = Pubkey::new_unique();
        let patches = vec![crate::types::InstructionAccountOp::Patch {
            instruction_index: 0,
            account_position: 1,
            new_pubkey: new_key,
            writable: false,
        }];
        let plan = apply_ix_account_ops(&mut tx, &patches).unwrap();
        // Read-only key appended at the end (index 3)
        assert_eq!(plan.static_accounts.len(), 4);
        assert_eq!(plan.static_accounts[3], new_key);
        assert_eq!(tx.message.instructions()[0].accounts[1], 3);
        // num_readonly_unsigned should have increased from 1 to 2
        assert_eq!(tx.message.header().num_readonly_unsigned_accounts, 2);
    }

    #[test]
    fn apply_ix_account_patches_reuses_existing_key() {
        let (mut tx, payer) = sample_transaction();
        let patches = vec![crate::types::InstructionAccountOp::Patch {
            instruction_index: 0,
            account_position: 1,
            new_pubkey: payer,
            writable: true,
        }];
        let plan = apply_ix_account_ops(&mut tx, &patches).unwrap();
        assert_eq!(plan.static_accounts.len(), 3);
        assert_eq!(tx.message.instructions()[0].accounts[1], 0);
    }

    #[test]
    fn apply_ix_account_patches_promotes_readonly_to_writable() {
        let (mut tx, _payer) = sample_transaction();
        // sample tx: [payer(ws), recipient(wu), system_program(ru)]
        // system_program is at index 2, readonly unsigned.
        // Patch instruction's account position 1 to system_program with :w.
        let system_program = tx.message.static_account_keys()[2];
        let patches = vec![crate::types::InstructionAccountOp::Patch {
            instruction_index: 0,
            account_position: 1,
            new_pubkey: system_program,
            writable: true,
        }];
        let plan = apply_ix_account_ops(&mut tx, &patches).unwrap();
        // system_program should now be writable. Since it was the only readonly
        // non-signer, num_readonly_unsigned goes from 1 to 0.
        assert_eq!(tx.message.header().num_readonly_unsigned_accounts, 0);
        // No keys added or removed.
        assert_eq!(plan.static_accounts.len(), 3);
    }

    #[test]
    fn apply_ix_account_patches_demotes_writable_to_readonly() {
        let (mut tx, _payer) = sample_transaction();
        // sample tx: [payer(ws), recipient(wu), system_program(ru)]
        // recipient is at index 1, writable unsigned.
        let recipient = tx.message.static_account_keys()[1];
        let patches = vec![crate::types::InstructionAccountOp::Patch {
            instruction_index: 0,
            account_position: 0,
            new_pubkey: recipient,
            writable: false,
        }];
        let plan = apply_ix_account_ops(&mut tx, &patches).unwrap();
        // recipient moved to readonly. num_readonly_unsigned goes from 1 to 2.
        assert_eq!(tx.message.header().num_readonly_unsigned_accounts, 2);
        assert_eq!(plan.static_accounts.len(), 3);
    }

    #[test]
    fn apply_ix_account_patches_rejects_invalid_ix_index() {
        let (mut tx, _) = sample_transaction();
        let patches = vec![crate::types::InstructionAccountOp::Patch {
            instruction_index: 99,
            account_position: 0,
            new_pubkey: Pubkey::new_unique(),
            writable: true,
        }];
        let err = apply_ix_account_ops(&mut tx, &patches).unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn apply_ix_account_patches_rejects_invalid_account_position() {
        let (mut tx, _) = sample_transaction();
        let patches = vec![crate::types::InstructionAccountOp::Patch {
            instruction_index: 0,
            account_position: 99,
            new_pubkey: Pubkey::new_unique(),
            writable: true,
        }];
        let err = apply_ix_account_ops(&mut tx, &patches).unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn apply_ix_account_patches_v0_writable_insert_shifts_lookup_indices() {
        let (mut tx, _payer) = sample_v0_transaction_with_lookup_accounts();
        let new_key = Pubkey::new_unique();
        let patches = vec![crate::types::InstructionAccountOp::Patch {
            instruction_index: 0,
            account_position: 0,
            new_pubkey: new_key,
            writable: true,
        }];

        let plan = apply_ix_account_ops(&mut tx, &patches).unwrap();

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
    fn apply_ix_account_patches_v0_readonly_append_shifts_lookup_indices() {
        let (mut tx, _payer) = sample_v0_transaction_with_lookup_accounts();
        let new_key = Pubkey::new_unique();
        let patches = vec![crate::types::InstructionAccountOp::Patch {
            instruction_index: 0,
            account_position: 1,
            new_pubkey: new_key,
            writable: false,
        }];

        let plan = apply_ix_account_ops(&mut tx, &patches).unwrap();

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
    fn apply_ix_account_patches_rejects_signer_writability_change() {
        let (mut tx, payer) = sample_transaction();
        // payer is a writable signer; patching to :r should error
        let patches = vec![crate::types::InstructionAccountOp::Patch {
            instruction_index: 0,
            account_position: 0,
            new_pubkey: payer,
            writable: false,
        }];
        let err = apply_ix_account_ops(&mut tx, &patches).unwrap_err();
        assert!(err.to_string().contains("signer"));
    }

    #[test]
    fn shift_indices_rejects_u8_overflow() {
        // Create a v0 message with an instruction referencing index 255
        let payer = Pubkey::new_unique();
        let program = Pubkey::new_unique();
        let message = MessageV0 {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![payer, program],
            recent_blockhash: Hash::new_unique(),
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![u8::MAX], // index 255
                data: vec![],
            }],
            address_table_lookups: vec![],
        };
        let mut tx = VersionedTransaction {
            signatures: vec![Signature::default()],
            message: VersionedMessage::V0(message),
        };
        // Inserting a writable key adds it before the readonly section (at static
        // pos 1), which would shift the existing 255 index to 256 — must error.
        let inserts = vec![crate::types::InstructionAccountOp::Insert {
            instruction_index: 0,
            account_position: 1,
            new_pubkey: Pubkey::new_unique(),
            writable: true,
        }];
        let err = apply_ix_account_ops(&mut tx, &inserts).unwrap_err();
        assert!(err.to_string().contains("overflow"));
    }

    #[test]
    fn apply_ix_account_inserts_at_front() {
        let (mut tx, _payer) = sample_transaction();
        // sample tx: [payer(ws), recipient(wu), system_program(ru)]
        // Instruction 0 accounts: [0, 1] with program_id_index=2
        let new_key = Pubkey::new_unique();
        let inserts = vec![crate::types::InstructionAccountOp::Insert {
            instruction_index: 0,
            account_position: 0,
            new_pubkey: new_key,
            writable: true,
        }];
        let plan = apply_ix_account_ops(&mut tx, &inserts).unwrap();
        // Writable key inserted into static keys at position 2 (before readonly section)
        assert_eq!(plan.static_accounts.len(), 4);
        assert_eq!(plan.static_accounts[2], new_key);
        let ix = &tx.message.instructions()[0];
        // Original [0, 1] shifted by static-keys insertion: 0 stays, 1 stays.
        // New key (static index 2) inserted at instruction account_position 0.
        assert_eq!(ix.accounts.len(), 3);
        assert_eq!(ix.accounts, vec![2, 0, 1]);
        // program_id_index should have shifted from 2 to 3
        assert_eq!(ix.program_id_index, 3);
    }

    #[test]
    fn apply_ix_account_inserts_in_middle() {
        let (mut tx, _payer) = sample_transaction();
        let new_key = Pubkey::new_unique();
        let inserts = vec![crate::types::InstructionAccountOp::Insert {
            instruction_index: 0,
            account_position: 1,
            new_pubkey: new_key,
            writable: false,
        }];
        let plan = apply_ix_account_ops(&mut tx, &inserts).unwrap();
        // Read-only key appended at the end of static keys (index 3)
        assert_eq!(plan.static_accounts.len(), 4);
        assert_eq!(plan.static_accounts[3], new_key);
        let ix = &tx.message.instructions()[0];
        // Original [0, 1] -> [0, 3, 1]
        assert_eq!(ix.accounts, vec![0, 3, 1]);
        assert_eq!(tx.message.header().num_readonly_unsigned_accounts, 2);
    }

    #[test]
    fn apply_ix_account_inserts_at_end_equals_append() {
        let (mut tx, _payer) = sample_transaction();
        let new_key = Pubkey::new_unique();
        let current_len = tx.message.instructions()[0].accounts.len();
        let inserts = vec![crate::types::InstructionAccountOp::Insert {
            instruction_index: 0,
            account_position: current_len,
            new_pubkey: new_key,
            writable: false,
        }];
        let plan = apply_ix_account_ops(&mut tx, &inserts).unwrap();
        assert_eq!(plan.static_accounts.len(), 4);
        let ix = &tx.message.instructions()[0];
        assert_eq!(ix.accounts.len(), current_len + 1);
        assert_eq!(*ix.accounts.last().unwrap(), 3);
    }

    #[test]
    fn apply_ix_account_inserts_reuses_existing_key() {
        let (mut tx, payer) = sample_transaction();
        let inserts = vec![crate::types::InstructionAccountOp::Insert {
            instruction_index: 0,
            account_position: 0,
            new_pubkey: payer,
            writable: true,
        }];
        let plan = apply_ix_account_ops(&mut tx, &inserts).unwrap();
        assert_eq!(plan.static_accounts.len(), 3);
        let ix = &tx.message.instructions()[0];
        assert_eq!(ix.accounts.len(), 3);
        assert_eq!(ix.accounts[0], 0); // payer reused at static index 0
    }

    #[test]
    fn apply_ix_account_inserts_rejects_invalid_ix_index() {
        let (mut tx, _) = sample_transaction();
        let inserts = vec![crate::types::InstructionAccountOp::Insert {
            instruction_index: 99,
            account_position: 0,
            new_pubkey: Pubkey::new_unique(),
            writable: true,
        }];
        let err = apply_ix_account_ops(&mut tx, &inserts).unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn apply_ix_account_inserts_rejects_position_past_end() {
        let (mut tx, _) = sample_transaction();
        let current_len = tx.message.instructions()[0].accounts.len();
        let inserts = vec![crate::types::InstructionAccountOp::Insert {
            instruction_index: 0,
            account_position: current_len + 1,
            new_pubkey: Pubkey::new_unique(),
            writable: true,
        }];
        let err = apply_ix_account_ops(&mut tx, &inserts).unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn apply_ix_account_inserts_v0_writable_shifts_lookup_indices() {
        let (mut tx, _payer) = sample_v0_transaction_with_lookup_accounts();
        let new_key = Pubkey::new_unique();
        let inserts = vec![crate::types::InstructionAccountOp::Insert {
            instruction_index: 0,
            account_position: 1,
            new_pubkey: new_key,
            writable: true,
        }];
        let plan = apply_ix_account_ops(&mut tx, &inserts).unwrap();
        // [payer, new_key, program] — new key inserted before readonly section
        assert_eq!(plan.static_accounts.len(), 3);
        assert_eq!(plan.static_accounts[1], new_key);
        let ix = &tx.message.instructions()[0];
        // Program index shifts (1 -> 2). Original instruction accounts (2,3) shift to (3,4),
        // and new account index 1 is inserted at position 1.
        assert_eq!(ix.program_id_index, 2);
        assert_eq!(ix.accounts, vec![3, 1, 4]);
    }

    #[test]
    fn apply_ix_account_removes_basic() {
        let (mut tx, _payer) = sample_transaction();
        // sample tx instruction 0: accounts = [0, 1] (payer, recipient)
        let removes = vec![crate::types::InstructionAccountOp::Remove {
            instruction_index: 0,
            account_position: 1,
        }];
        let plan = apply_ix_account_ops(&mut tx, &removes).unwrap();
        // Static account_keys is left intact: [payer, recipient, system_program]
        assert_eq!(plan.static_accounts.len(), 3);
        let ix = &tx.message.instructions()[0];
        // Instruction account at position 1 removed; only payer reference remains.
        assert_eq!(ix.accounts, vec![0]);
        // program_id_index untouched.
        assert_eq!(ix.program_id_index, 2);
    }

    #[test]
    fn apply_ix_account_removes_at_front() {
        let (mut tx, _payer) = sample_transaction();
        let removes = vec![crate::types::InstructionAccountOp::Remove {
            instruction_index: 0,
            account_position: 0,
        }];
        apply_ix_account_ops(&mut tx, &removes).unwrap();
        let ix = &tx.message.instructions()[0];
        // [0, 1] -> [1]
        assert_eq!(ix.accounts, vec![1]);
    }

    #[test]
    fn apply_ix_account_removes_sequential_apply() {
        // Insert two extra accounts so we have a 4-account list, then remove
        // positions in user-listed (highest-first) order to demonstrate the
        // recommended pattern.
        let (mut tx, _payer) = sample_transaction();
        let k1 = Pubkey::new_unique();
        let k2 = Pubkey::new_unique();
        // Insert at the end of the current accounts list (equivalent to appending).
        let initial_len = tx.message.instructions()[0].accounts.len();
        apply_ix_account_ops(
            &mut tx,
            &[
                crate::types::InstructionAccountOp::Insert {
                    instruction_index: 0,
                    account_position: initial_len,
                    new_pubkey: k1,
                    writable: false,
                },
                crate::types::InstructionAccountOp::Insert {
                    instruction_index: 0,
                    account_position: initial_len + 1,
                    new_pubkey: k2,
                    writable: false,
                },
            ],
        )
        .unwrap();
        // Now instruction 0 has 4 accounts.
        let original = tx.message.instructions()[0].accounts.clone();
        assert_eq!(original.len(), 4);

        // Remove positions 3 then 1 (sequentially): drops original[3] and original[1].
        let removes = vec![
            crate::types::InstructionAccountOp::Remove {
                instruction_index: 0,
                account_position: 3,
            },
            crate::types::InstructionAccountOp::Remove {
                instruction_index: 0,
                account_position: 1,
            },
        ];
        apply_ix_account_ops(&mut tx, &removes).unwrap();
        let ix = &tx.message.instructions()[0];
        assert_eq!(ix.accounts, vec![original[0], original[2]]);
    }

    #[test]
    fn apply_ix_account_removes_rejects_invalid_ix_index() {
        let (mut tx, _) = sample_transaction();
        let removes = vec![crate::types::InstructionAccountOp::Remove {
            instruction_index: 99,
            account_position: 0,
        }];
        let err = apply_ix_account_ops(&mut tx, &removes).unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn apply_ix_account_removes_rejects_position_out_of_bounds() {
        let (mut tx, _) = sample_transaction();
        let current_len = tx.message.instructions()[0].accounts.len();
        let removes = vec![crate::types::InstructionAccountOp::Remove {
            instruction_index: 0,
            account_position: current_len,
        }];
        let err = apply_ix_account_ops(&mut tx, &removes).unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn apply_ix_account_removes_v0_preserves_lookup_indices() {
        let (mut tx, _payer) = sample_v0_transaction_with_lookup_accounts();
        // Original instruction 0 accounts: [2, 3] (lookup-derived).
        let removes = vec![crate::types::InstructionAccountOp::Remove {
            instruction_index: 0,
            account_position: 0,
        }];
        apply_ix_account_ops(&mut tx, &removes).unwrap();
        let ix = &tx.message.instructions()[0];
        // Static keys untouched, so the surviving lookup index 3 is still valid.
        assert_eq!(ix.accounts, vec![3]);
        assert_eq!(ix.program_id_index, 1);
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

    // ── apply_instruction_ops: remove ──

    #[test]
    fn apply_instruction_remove_drops_instruction() {
        let (mut tx, _payer) = sample_transaction();
        // Start with one instruction; remove it.
        assert_eq!(tx.message.instructions().len(), 1);
        apply_instruction_ops(&mut tx, &[crate::types::InstructionOp::Remove { index: 0 }])
            .unwrap();
        assert!(tx.message.instructions().is_empty());
    }

    #[test]
    fn apply_instruction_remove_shifts_subsequent_indices() {
        // Build a 2-instruction transaction so we can observe index shifting.
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();
        let blockhash = Hash::new_unique();
        let ix1 = system_instruction::transfer(&payer.pubkey(), &recipient, 1);
        let ix2 = system_instruction::transfer(&payer.pubkey(), &recipient, 2);
        let message = Message::new(&[ix1, ix2], Some(&payer.pubkey()));
        let transaction = Transaction::new(&[&payer], message, blockhash);
        let mut tx = VersionedTransaction::from(transaction);
        assert_eq!(tx.message.instructions().len(), 2);

        // Remove instruction 0; the second instruction becomes the first.
        apply_instruction_ops(&mut tx, &[crate::types::InstructionOp::Remove { index: 0 }])
            .unwrap();
        assert_eq!(tx.message.instructions().len(), 1);
        // The surviving instruction carries ix2's data (amount = 2 lamports,
        // encoded as little-endian u64 at the start of the data after the
        // 4-byte transfer discriminator).
        let data = &tx.message.instructions()[0].data;
        let amount = u64::from_le_bytes(data[4..12].try_into().unwrap());
        assert_eq!(amount, 2);
    }

    #[test]
    fn apply_instruction_remove_rejects_out_of_range() {
        let (mut tx, _payer) = sample_transaction();
        let err =
            apply_instruction_ops(&mut tx, &[crate::types::InstructionOp::Remove { index: 5 }])
                .unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }

    // ── apply_instruction_ops: insert ──

    fn memo_instruction(payer: Pubkey, text: &str) -> Instruction {
        // Memo program: no accounts, data is the raw UTF-8 text. Add the payer
        // as a writable signer so we exercise the signer-insert path.
        Instruction {
            program_id: solana_pubkey::pubkey!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"),
            accounts: vec![solana_instruction::AccountMeta::new(payer, true)],
            data: text.as_bytes().to_vec(),
        }
    }

    #[test]
    fn apply_instruction_insert_appends_with_existing_keys() {
        let (mut tx, payer) = sample_transaction();
        // Insert a transfer that reuses the payer (existing signer) and the
        // existing system program — no new keys should be added.
        let recipient = tx.message.static_account_keys()[1];
        let system_program = tx.message.static_account_keys()[2];
        let ix = Instruction {
            program_id: system_program,
            accounts: vec![
                solana_instruction::AccountMeta::new(payer, true),
                solana_instruction::AccountMeta::new(recipient, false),
            ],
            data: vec![2, 0, 0, 0, 99, 0, 0, 0, 0, 0, 0, 0],
        };
        let count = tx.message.instructions().len();
        apply_instruction_ops(
            &mut tx,
            &[crate::types::InstructionOp::Insert { position: count, instruction: ix }],
        )
        .unwrap();

        assert_eq!(tx.message.instructions().len(), count + 1);
        // No new keys: account table unchanged.
        assert_eq!(tx.message.static_account_keys().len(), 3);
        let inserted = &tx.message.instructions()[count];
        assert_eq!(inserted.program_id_index, 2);
        assert_eq!(inserted.accounts, vec![0, 1]);
    }

    #[test]
    fn apply_instruction_insert_new_non_signer_keys() {
        let (mut tx, payer) = sample_transaction();
        let new_program = Pubkey::new_unique();
        let new_writable = Pubkey::new_unique();
        let new_readonly = Pubkey::new_unique();
        let ix = Instruction {
            program_id: new_program,
            accounts: vec![
                solana_instruction::AccountMeta::new(payer, true),
                solana_instruction::AccountMeta::new(new_writable, false),
                solana_instruction::AccountMeta::new_readonly(new_readonly, false),
            ],
            data: vec![0xab],
        };
        apply_instruction_ops(
            &mut tx,
            &[crate::types::InstructionOp::Insert { position: 0, instruction: ix }],
        )
        .unwrap();

        // Original: [payer(ws,0), recipient(wu,1), system_program(ru,2)]
        // Adds: new_program(readonly unsigned, appended), new_writable (wu,
        //   inserted before readonly section), new_readonly (readonly unsigned).
        // Final static layout:
        //   [payer(0), recipient(1), new_writable(2), system_program(3),
        //    new_program(4), new_readonly(5)]
        let keys = tx.message.static_account_keys();
        assert_eq!(keys.len(), 6);
        assert_eq!(keys[2], new_writable);
        assert_eq!(keys[4], new_program);
        assert_eq!(keys[5], new_readonly);
        let ix0 = &tx.message.instructions()[0];
        assert_eq!(ix0.program_id_index, 4);
        assert_eq!(ix0.accounts, vec![0, 2, 5]);
        // num_readonly_unsigned grew by 3 (system_program + new_program + new_readonly
        // all readonly unsigned): originally 1, now 3.
        assert_eq!(tx.message.header().num_readonly_unsigned_accounts, 3);
    }

    #[test]
    fn apply_instruction_insert_new_signer_bumps_header_and_signature() {
        let (mut tx, _payer) = sample_transaction();
        // Original header: num_required_signatures=1, readonly_signed=0.
        let new_signer = Pubkey::new_unique();
        let ix = Instruction {
            program_id: tx.message.static_account_keys()[2], // system program
            accounts: vec![solana_instruction::AccountMeta::new(new_signer, true)],
            data: vec![],
        };
        assert_eq!(tx.signatures.len(), 1);
        apply_instruction_ops(
            &mut tx,
            &[crate::types::InstructionOp::Insert { position: 0, instruction: ix }],
        )
        .unwrap();

        // New signer inserted into the writable-signer region (before any
        // readonly signers); num_required_signatures bumps to 2.
        assert_eq!(tx.message.header().num_required_signatures, 2);
        assert_eq!(tx.message.header().num_readonly_signed_accounts, 0);
        assert_eq!(tx.signatures.len(), 2);
        // The new signer lands at index 1 (after the existing writable payer at 0).
        assert_eq!(tx.message.static_account_keys()[1], new_signer);
        let ix0 = &tx.message.instructions()[0];
        assert_eq!(ix0.accounts, vec![1]);
    }

    #[test]
    fn apply_instruction_insert_readonly_signer_appended_to_signer_section() {
        let (mut tx, _payer) = sample_transaction();
        let new_signer = Pubkey::new_unique();
        let system_program = tx.message.static_account_keys()[2];
        let ix = Instruction {
            program_id: system_program,
            accounts: vec![solana_instruction::AccountMeta::new_readonly(new_signer, true)],
            data: vec![],
        };
        apply_instruction_ops(
            &mut tx,
            &[crate::types::InstructionOp::Insert { position: 0, instruction: ix }],
        )
        .unwrap();

        // Read-only signer sits at the end of the signer section (index 1),
        // so readonly_signed bumps to 1 while num_required_signatures bumps to 2.
        assert_eq!(tx.message.header().num_required_signatures, 2);
        assert_eq!(tx.message.header().num_readonly_signed_accounts, 1);
        assert_eq!(tx.message.static_account_keys()[1], new_signer);
        assert_eq!(tx.signatures.len(), 2);
        let ix0 = &tx.message.instructions()[0];
        assert_eq!(ix0.accounts, vec![1]);
    }

    #[test]
    fn apply_instruction_insert_rejects_promoting_non_signer_to_signer() {
        let (mut tx, _payer) = sample_transaction();
        // recipient is a writable non-signer at index 1; try to insert an ix
        // that declares it as a signer.
        let recipient = tx.message.static_account_keys()[1];
        let system_program = tx.message.static_account_keys()[2];
        let ix = Instruction {
            program_id: system_program,
            accounts: vec![solana_instruction::AccountMeta::new(recipient, true)],
            data: vec![],
        };
        let err = apply_instruction_ops(
            &mut tx,
            &[crate::types::InstructionOp::Insert { position: 0, instruction: ix }],
        )
        .unwrap_err();
        assert!(err.to_string().contains("non-signer"), "got: {err}");
    }

    #[test]
    fn apply_instruction_insert_rejects_out_of_range_position() {
        let (mut tx, _payer) = sample_transaction();
        let count = tx.message.instructions().len();
        let program = tx.message.static_account_keys()[2];
        let ix = Instruction { program_id: program, accounts: vec![], data: vec![] };
        let err = apply_instruction_ops(
            &mut tx,
            &[crate::types::InstructionOp::Insert { position: count + 1, instruction: ix }],
        )
        .unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }

    // ── apply_instruction_ops: union privileges (never demote, only promote) ──

    /// Builds a tx whose message header has both a writable signer and a
    /// readonly signer, returning the two signer keys plus the (readonly)
    /// system program key. Layout: [writable_signer(ws), readonly_signer(rs),
    /// system_program(ru)].
    fn sample_transaction_with_readonly_signer() -> (VersionedTransaction, Pubkey, Pubkey) {
        let writable_signer = Keypair::new();
        let readonly_signer = Keypair::new();
        let blockhash = Hash::new_unique();
        // A harmless instruction to system program listing both signers so the
        // message header reflects their roles. Empty data keeps it inert.
        let ix = Instruction {
            program_id: solana_sdk_ids::system_program::id(),
            accounts: vec![
                solana_instruction::AccountMeta::new(writable_signer.pubkey(), true),
                solana_instruction::AccountMeta::new_readonly(readonly_signer.pubkey(), true),
                solana_instruction::AccountMeta::new_readonly(
                    solana_sdk_ids::system_program::id(),
                    false,
                ),
            ],
            data: vec![],
        };
        let message = Message::new(&[ix], Some(&writable_signer.pubkey()));
        let transaction =
            Transaction::new(&[&writable_signer, &readonly_signer], message, blockhash);
        (
            VersionedTransaction::from(transaction),
            writable_signer.pubkey(),
            readonly_signer.pubkey(),
        )
    }

    /// Like [`sample_transaction_with_readonly_signer`] but with **two** readonly
    /// signers, so layout is:
    /// `[writable_signer(ws,0), readonly_signer_a(rs,1), readonly_signer_b(rs,2),
    /// system_program(ru,3)]`. The second readonly signer (index 2) is NOT at the
    /// writable/readonly-signer boundary (index 1), which is the exact shape that
    /// makes a signer-promotion swap non-trivial and can misalign signatures.
    fn sample_transaction_with_two_readonly_signers() -> (VersionedTransaction, Pubkey, Pubkey) {
        let writable_signer = Keypair::new();
        let readonly_signer_a = Keypair::new();
        let readonly_signer_b = Keypair::new();
        let blockhash = Hash::new_unique();
        let ix = Instruction {
            program_id: solana_sdk_ids::system_program::id(),
            accounts: vec![
                solana_instruction::AccountMeta::new(writable_signer.pubkey(), true),
                solana_instruction::AccountMeta::new_readonly(readonly_signer_a.pubkey(), true),
                solana_instruction::AccountMeta::new_readonly(readonly_signer_b.pubkey(), true),
                solana_instruction::AccountMeta::new_readonly(
                    solana_sdk_ids::system_program::id(),
                    false,
                ),
            ],
            data: vec![],
        };
        let message = Message::new(&[ix], Some(&writable_signer.pubkey()));
        let transaction = Transaction::new(
            &[&writable_signer, &readonly_signer_a, &readonly_signer_b],
            message,
            blockhash,
        );
        (
            VersionedTransaction::from(transaction),
            readonly_signer_a.pubkey(),
            readonly_signer_b.pubkey(),
        )
    }

    #[test]
    fn apply_instruction_insert_does_not_demote_writable_non_signer() {
        let (mut tx, _payer) = sample_transaction();
        // sample tx: [payer(ws,0), recipient(wu,1), system_program(ru,2)].
        // The original transfer needs `recipient` writable. Insert an
        // instruction that references `recipient` as read-only: union rules
        // must keep it writable so the existing instruction keeps working.
        let recipient = tx.message.static_account_keys()[1];
        let system_program = tx.message.static_account_keys()[2];
        let ix = Instruction {
            program_id: system_program,
            accounts: vec![solana_instruction::AccountMeta::new_readonly(recipient, false)],
            data: vec![],
        };
        let readonly_unsigned_before = tx.message.header().num_readonly_unsigned_accounts;
        apply_instruction_ops(
            &mut tx,
            &[crate::types::InstructionOp::Insert { position: 0, instruction: ix }],
        )
        .unwrap();

        // No new keys added, no writability header change.
        assert_eq!(tx.message.static_account_keys().len(), 3);
        assert_eq!(tx.message.header().num_readonly_unsigned_accounts, readonly_unsigned_before);
        let recipient_pos =
            tx.message.static_account_keys().iter().position(|k| *k == recipient).unwrap();
        assert!(account_is_writable(&tx.message, recipient_pos));
    }

    #[test]
    fn apply_instruction_insert_does_not_demote_writable_signer() {
        let (mut tx, payer) = sample_transaction();
        // payer is a writable signer. Insert an instruction referencing it as a
        // readonly signer: before the fix this errored ("signer writability is
        // fixed"); union rules must keep it writable with no error.
        let system_program = tx.message.static_account_keys()[2];
        let ix = Instruction {
            program_id: system_program,
            accounts: vec![solana_instruction::AccountMeta::new_readonly(payer, true)],
            data: vec![],
        };
        apply_instruction_ops(
            &mut tx,
            &[crate::types::InstructionOp::Insert { position: 0, instruction: ix }],
        )
        .expect("referencing a writable signer as readonly-signer must not error");

        let payer_pos = tx.message.static_account_keys().iter().position(|k| *k == payer).unwrap();
        assert!(account_is_writable(&tx.message, payer_pos));
        // Still a single signer; not promoted/demoted.
        assert_eq!(tx.message.header().num_required_signatures, 1);
        assert_eq!(tx.message.header().num_readonly_signed_accounts, 0);
    }

    #[test]
    fn apply_instruction_insert_promotes_readonly_non_signer_to_writable() {
        let (mut tx, _payer) = sample_transaction();
        // system_program is readonly unsigned (index 2). Insert an instruction
        // referencing it as writable: union must promote it.
        let system_program = tx.message.static_account_keys()[2];
        let ix = Instruction {
            program_id: system_program,
            accounts: vec![solana_instruction::AccountMeta::new(system_program, false)],
            data: vec![],
        };
        assert_eq!(tx.message.header().num_readonly_unsigned_accounts, 1);
        apply_instruction_ops(
            &mut tx,
            &[crate::types::InstructionOp::Insert { position: 0, instruction: ix }],
        )
        .unwrap();

        let pos =
            tx.message.static_account_keys().iter().position(|k| *k == system_program).unwrap();
        assert!(account_is_writable(&tx.message, pos));
        assert_eq!(tx.message.header().num_readonly_unsigned_accounts, 0);
    }

    #[test]
    fn apply_instruction_insert_promotes_readonly_signer_to_writable_signer() {
        let (mut tx, _writable_signer, readonly_signer) = sample_transaction_with_readonly_signer();
        // Layout: [writable_signer(ws,0), readonly_signer(rs,1), system_program(ru,2)].
        assert_eq!(tx.message.header().num_readonly_signed_accounts, 1);
        let system_program = tx.message.static_account_keys()[2];
        let ix = Instruction {
            program_id: system_program,
            accounts: vec![solana_instruction::AccountMeta::new(readonly_signer, true)],
            data: vec![],
        };
        apply_instruction_ops(
            &mut tx,
            &[crate::types::InstructionOp::Insert { position: 0, instruction: ix }],
        )
        .expect("promoting a readonly signer to writable-signer must succeed");

        let pos =
            tx.message.static_account_keys().iter().position(|k| *k == readonly_signer).unwrap();
        assert!(account_is_writable(&tx.message, pos), "readonly signer should now be writable");
        // The readonly-signed region shrank to zero.
        assert_eq!(tx.message.header().num_readonly_signed_accounts, 0);
        assert_eq!(tx.message.header().num_required_signatures, 2);
    }

    #[test]
    fn apply_instruction_insert_signer_promotion_keeps_signatures_aligned() {
        // Regression: promoting a readonly signer that is NOT at the
        // writable/readonly-signer boundary performs a non-trivial key swap.
        // Swapping keys without also swapping the matching signature slots
        // breaks the VersionedTransaction invariant that `signatures[i]`
        // belongs to the signer at `account_keys[i]`.
        let (mut tx, _rs_a, rs_b) = sample_transaction_with_two_readonly_signers();
        // Layout: [ws(0), rs_a(1), rs_b(2), system_program(3,ru)].
        // Promoting rs_b (pos 2): boundary = num_sigs(3) - readonly_signed(2)
        // = 1, so swap(2, 1) is non-trivial — the case where a key-only swap
        // would misalign signatures.
        assert_eq!(tx.message.header().num_required_signatures, 3);
        assert_eq!(tx.message.header().num_readonly_signed_accounts, 2);

        // Record each signer's original signature by key.
        let orig: Vec<(Pubkey, Signature)> = (0..tx.message.header().num_required_signatures
            as usize)
            .map(|i| (tx.message.static_account_keys()[i], tx.signatures[i]))
            .collect();

        let system_program = tx.message.static_account_keys()[3];
        let ix = Instruction {
            program_id: system_program,
            accounts: vec![solana_instruction::AccountMeta::new(rs_b, true)],
            data: vec![],
        };
        apply_instruction_ops(
            &mut tx,
            &[crate::types::InstructionOp::Insert { position: 0, instruction: ix }],
        )
        .expect("promoting a readonly signer to writable-signer must succeed");

        // After promotion, each signer key must still carry its ORIGINAL
        // signature (they just moved position together).
        assert_eq!(tx.message.header().num_required_signatures, 3);
        for i in 0..tx.message.header().num_required_signatures as usize {
            let key = tx.message.static_account_keys()[i];
            let sig = tx.signatures[i];
            let expected = orig
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, s)| *s)
                .expect("signer key should have a recorded signature");
            assert_eq!(
                sig, expected,
                "signature at position {i} (key {key}) is misaligned after signer promotion"
            );
        }

        // rs_b is now writable and landed at the writable/readonly boundary (pos 1).
        let pos = tx.message.static_account_keys().iter().position(|k| *k == rs_b).unwrap();
        assert_eq!(pos, 1);
        assert!(account_is_writable(&tx.message, pos));
        assert_eq!(tx.message.header().num_readonly_signed_accounts, 1);
    }

    #[test]
    fn apply_instruction_insert_keeps_signer_when_referenced_as_non_signer() {
        // Union: an existing signer referenced as a non-signer stays a signer
        // (never demoted), and a non-signer meta never adds signer privilege.
        let (mut tx, payer) = sample_transaction();
        let system_program = tx.message.static_account_keys()[2];
        // Reference payer as a writable non-signer.
        let ix = Instruction {
            program_id: system_program,
            accounts: vec![solana_instruction::AccountMeta::new(payer, false)],
            data: vec![],
        };
        apply_instruction_ops(
            &mut tx,
            &[crate::types::InstructionOp::Insert { position: 0, instruction: ix }],
        )
        .unwrap();
        assert_eq!(tx.message.header().num_required_signatures, 1);
        let payer_pos = tx.message.static_account_keys().iter().position(|k| *k == payer).unwrap();
        assert!(account_is_writable(&tx.message, payer_pos));
    }

    #[test]
    fn apply_instruction_insert_unions_duplicate_metas_regardless_of_order() {
        // Solana unions duplicate account privileges within an instruction
        // regardless of meta order. An account listed first as a non-signer and
        // later as a signer must end up a signer; without per-pubkey
        // aggregation, the first meta inserts it as a non-signer and the
        // second rejects the promotion as unsupported.
        let (mut tx, _payer) = sample_transaction();
        let dup = Pubkey::new_unique();
        let system_program = tx.message.static_account_keys()[2];
        // Same pubkey twice: readonly non-signer FIRST, then writable signer.
        let ix = Instruction {
            program_id: system_program,
            accounts: vec![
                solana_instruction::AccountMeta::new_readonly(dup, false),
                solana_instruction::AccountMeta::new(dup, true),
            ],
            data: vec![],
        };
        apply_instruction_ops(
            &mut tx,
            &[crate::types::InstructionOp::Insert { position: 0, instruction: ix }],
        )
        .expect("duplicate metas must be unioned, not rejected by meta order");

        let pos = tx.message.static_account_keys().iter().position(|k| *k == dup).unwrap();
        // Union: signer AND writable.
        assert!(pos < tx.message.header().num_required_signatures as usize, "must be a signer");
        assert!(account_is_writable(&tx.message, pos), "must be writable");
        // One new key, one new signature.
        assert_eq!(tx.message.header().num_required_signatures, 2);
        assert_eq!(tx.signatures.len(), 2);
        // The compiled instruction references the single unioned index for both
        // metas.
        let compiled = &tx.message.instructions()[0];
        assert_eq!(compiled.accounts.len(), 2);
        assert_eq!(compiled.accounts[0], compiled.accounts[1]);
    }

    #[test]
    fn apply_instruction_ops_runs_in_order() {
        // Insert two instructions (positions 0 and 1), then remove index 0.
        let (mut tx, payer) = sample_transaction();
        let first = memo_instruction(payer, "first");
        let second = memo_instruction(payer, "second");

        apply_instruction_ops(
            &mut tx,
            &[
                crate::types::InstructionOp::Insert { position: 0, instruction: first },
                crate::types::InstructionOp::Insert { position: 1, instruction: second },
                crate::types::InstructionOp::Remove { index: 0 },
            ],
        )
        .unwrap();

        // After inserting two at the front then removing the first, only the
        // "second" memo survives at index 0.
        assert_eq!(tx.message.instructions().len(), 2);
        assert_eq!(tx.message.instructions()[0].data, b"second");
    }
}
