mod idl;
mod raw;
mod serde;
mod types;

pub use self::idl::{
    Idl, IdlAccount, IdlAccountItem, IdlAccounts, IdlArg, IdlEvent, IdlInstruction, IdlMetadata,
};
pub use self::raw::{LegacyIdl, RawAnchorIdl};
pub use self::types::{
    DefinedType, IdlArrayType, IdlEnumVariant, IdlField, IdlFields, IdlType, IdlTypeDefinition,
    IdlTypeDefinitionBody, IdlTypeDefinitionKind,
};

pub(crate) use self::serde::deserialize_optional_idl_fields;

#[cfg(test)]
mod tests;
