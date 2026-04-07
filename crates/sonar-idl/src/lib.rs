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
//!
//! # Example
//!
//! ```
//! use sonar_idl::Idl;
//!
//! # fn example(json: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
//! let idl = Idl::parse(json, "ProgramAddress1111111111111111111111111111111")?;
//!
//! // Parse instruction data
//! if let Some(instruction) = idl.parse_instruction(&[/* instruction data */])? {
//!     println!("Instruction: {}", instruction.name);
//! }
//! # Ok(())
//! # }
//! ```

mod discriminator;
mod error;
mod models;
mod parser;

// ── Discriminator ──

pub use discriminator::{
    CPI_EVENT_DISCRIMINATOR, DISCRIMINATOR_LEN, Discriminator, NAMESPACE_ACCOUNT, NAMESPACE_EVENT,
    NAMESPACE_GLOBAL, sighash,
};

// ── Error ──

pub use error::{IdlError, Result};

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
