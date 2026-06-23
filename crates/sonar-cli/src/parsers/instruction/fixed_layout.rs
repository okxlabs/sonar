//! Program-agnostic declarative table for fixed-layout instructions.
//!
//! A program describes its simple instructions — those whose data is a fixed
//! sequence of primitive fields and whose accounts follow one of a few naming
//! rules — as a `&[InstructionDef]`, and dispatches with [`parse`].
//!
//! The discriminator is matched as a raw byte prefix (`&[u8]`), so a 1-byte SPL
//! Token tag, a 4-byte little-endian System tag, or an 8-byte Anchor
//! discriminator all share the same table type. Variable-length or otherwise
//! irregular instructions stay in each program's bespoke parser.

use anyhow::Result;
use sonar_idl::IdlValue;

use super::{ParsedField, ParsedInstruction, append_extra_account_names};
use crate::core::transaction::InstructionSummary;
use crate::parsers::binary_reader::{self, BinaryReader};

/// A primitive field in a fixed-layout instruction's data.
pub(super) enum FieldType {
    U8,
    U32,
    U64,
    Pubkey,
}

impl FieldType {
    /// Encoded size in bytes; the sum across a def's fields is the exact
    /// expected data length.
    fn size(&self) -> usize {
        match self {
            FieldType::U8 => 1,
            FieldType::U32 => 4,
            FieldType::U64 => 8,
            FieldType::Pubkey => 32,
        }
    }

    fn read(&self, reader: &mut BinaryReader) -> Result<IdlValue> {
        Ok(match self {
            FieldType::U8 => IdlValue::U8(reader.read_u8()?),
            FieldType::U32 => IdlValue::U32(reader.read_u32()?),
            FieldType::U64 => IdlValue::U64(reader.read_u64()?),
            FieldType::Pubkey => IdlValue::Pubkey(reader.read_pubkey()?),
        })
    }
}

pub(super) struct FieldDef {
    pub(super) name: &'static str,
    pub(super) ty: FieldType,
}

/// How an instruction's accounts are validated and named.
pub(super) enum AccountRule {
    /// Use `account_names` verbatim; the actual account count is not checked.
    /// (System program instructions, which have fixed account lists.)
    Verbatim,
    /// Require exactly `account_names.len()` accounts.
    Exact,
    /// Require at least `account_names.len()` accounts; name any extras
    /// `additional_signer_N` (SPL multisig).
    MinWithSigners,
}

/// Declarative description of a fixed-layout instruction: a discriminator
/// (matched as a raw byte prefix), a name, a sequence of primitive data fields,
/// and an account-naming rule. Shared by every program parser.
pub(super) struct InstructionDef {
    pub(super) discriminator: &'static [u8],
    pub(super) name: &'static str,
    pub(super) fields: &'static [FieldDef],
    pub(super) account_names: &'static [&'static str],
    pub(super) account_rule: AccountRule,
}

/// Find the def whose discriminator prefixes `instruction.data` and decode it.
/// Returns `Ok(None)` if no def matches or the data/accounts don't fit.
pub(super) fn parse(
    defs: &[InstructionDef],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    let Some(def) = defs.iter().find(|d| instruction.data.starts_with(d.discriminator)) else {
        return Ok(None);
    };
    let data = &instruction.data[def.discriminator.len()..];
    decode(def, data, instruction)
}

fn decode(
    def: &InstructionDef,
    data: &[u8],
    instruction: &InstructionSummary,
) -> Result<Option<ParsedInstruction>> {
    let Some(account_names) = resolve_account_names(def, instruction.accounts.len()) else {
        return Ok(None);
    };

    // Field-bearing instructions require an exact data length; field-less ones
    // ignore any trailing bytes (matching the legacy per-program parsers).
    if def.fields.is_empty() {
        let fields: Vec<ParsedField> = Vec::new();
        return Ok(Some(ParsedInstruction {
            name: def.name.to_string(),
            fields: fields.into(),
            account_names,
        }));
    }

    let expected: usize = def.fields.iter().map(|f| f.ty.size()).sum();
    if data.len() != expected {
        return Ok(None);
    }

    binary_reader::try_parse(data, |reader| {
        let mut fields = Vec::with_capacity(def.fields.len());
        for field in def.fields {
            fields.push(ParsedField { name: field.name.into(), value: field.ty.read(reader)? });
        }
        Ok(ParsedInstruction { name: def.name.to_string(), fields: fields.into(), account_names })
    })
}

/// Apply the def's [`AccountRule`] to the actual account count, returning the
/// resolved account names or `None` if the count is invalid for the rule.
fn resolve_account_names(def: &InstructionDef, total_accounts: usize) -> Option<Vec<String>> {
    let named = def.account_names.len();
    match def.account_rule {
        AccountRule::Verbatim => Some(owned(def.account_names)),
        AccountRule::Exact => (total_accounts == named).then(|| owned(def.account_names)),
        AccountRule::MinWithSigners => (total_accounts >= named).then(|| {
            let mut names = owned(def.account_names);
            append_extra_account_names(&mut names, total_accounts, named, "additional_signer_");
            names
        }),
    }
}

fn owned(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).to_string()).collect()
}
