//! sonar-idl: Anchor IDL parsing and type resolution for Solana programs.
//!
//! This crate provides a pure, CLI-agnostic library for working with
//! Anchor IDL files:
//!
//! - **Models**: Canonical IDL types (`Idl`, `IdlInstruction`, etc.) plus
//!   backward-compatible deserialization of legacy (pre-0.30) IDL JSON.
//! - **Parsing**: Binary deserialization of instruction args, account data,
//!   and CPI events using IDL type definitions.
//! - **Discriminator**: `sighash` for computing Anchor 8-byte discriminators.
//! - **Value**: Internal JSON-like AST used by parser internals.

mod discriminator;
mod models;
mod parser;

// ── Discriminator ──

pub use discriminator::sighash;

// ── Models ──

pub use models::{
    DefinedType, Idl, IdlAccount, IdlAccountItem, IdlAccounts, IdlArg, IdlArrayType,
    IdlEnumVariant, IdlEvent, IdlField, IdlFields, IdlInstruction, IdlMetadata, IdlType,
    IdlTypeDefinition, IdlTypeDefinitionBody, IdlTypeDefinitionKind, LegacyIdl, RawAnchorIdl,
};

// ── Parsing ──

pub use parser::{
    IdlParsedField, IdlParsedInstruction, ResolvedIdl, find_event_by_discriminator,
    find_instruction_by_discriminator, is_cpi_event_data, parse_account_data, parse_cpi_event_data,
    parse_instruction,
};
