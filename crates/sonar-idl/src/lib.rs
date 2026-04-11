//! sonar-idl: Anchor IDL parsing and type resolution for Solana programs.
//!
//! This crate provides a pure, CLI-agnostic decoder for Anchor IDL files:
//!
//! - **Input**: `IndexedIdl` deserializes both current and legacy
//!   (pre-0.30) Anchor IDL JSON.
//! - **Parsing**: `IndexedIdl` normalizes and indexes IDL definitions
//!   before decoding instruction args, account data, and CPI events.
//! - **Discriminator**: `sighash` for computing Anchor 8-byte discriminators.
//! - **Values**: Parsed fields use the domain-native `IdlValue` enum.
//!
//! ```rust
//! use sonar_idl::IndexedIdl;
//!
//! let indexed: IndexedIdl = serde_json::from_str(
//!     r#"{
//!         "address": "11111111111111111111111111111111",
//!         "metadata": { "name": "demo", "version": "0.1.0", "spec": "0.1.0" },
//!         "instructions": []
//!     }"#,
//! )?;
//! # let _ = indexed;
//! # Ok::<(), serde_json::Error>(())
//! ```

mod decode;
mod discriminator;
mod idl;
mod indexed;
mod value;

#[cfg(test)]
mod tests;

pub use discriminator::sighash;
pub use indexed::{
    IdlInstructionFields, IdlParsedField, IdlParsedInstruction, IndexedIdl, is_cpi_event_data,
};
pub use value::IdlValue;
