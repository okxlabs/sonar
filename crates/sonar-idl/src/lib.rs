//! sonar-idl: Anchor IDL parsing and type resolution for Solana programs.
//!
//! This crate provides a pure, CLI-agnostic decoder for Anchor IDL files:
//!
//! - **Input**: `IndexedIdl` deserializes both current and legacy
//!   (pre-0.30) Anchor IDL JSON.
//! - **Parsing**: `IndexedIdl` normalizes and indexes IDL definitions
//!   before decoding instruction args, account data, and CPI events.
//! - **Discriminator**: `sighash` for computing Anchor 8-byte discriminators.
//! - **Values**: Parsed fields are returned as ordered `serde_json::Value`.
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

mod discriminator;
mod models;
mod parser;

// ── Discriminator ──

pub use discriminator::sighash;

// ── Parsing ──

pub use parser::{IdlParsedField, IdlParsedInstruction, IndexedIdl, is_cpi_event_data};
