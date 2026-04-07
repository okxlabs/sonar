//! IDL model types.
//!
//! This module contains the canonical types for representing Anchor IDL files.

mod account;
mod core;
mod legacy;
mod types;

pub use account::{IdlAccount, IdlAccountItem, IdlAccounts};
pub use core::{Idl, IdlEvent, IdlInstruction, IdlMetadata};
pub use legacy::{LegacyIdl, RawAnchorIdl};
pub use types::{
    DefinedType, IdlArg, IdlArrayType, IdlEnumVariant, IdlField, IdlFields, IdlType,
    IdlTypeDefinition, IdlTypeDefinitionBody, IdlTypeDefinitionKind,
};
