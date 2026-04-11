use solana_pubkey::Pubkey;

/// Domain-native value type for decoded IDL data.
///
/// Each variant mirrors the corresponding IDL/Solana type exactly,
/// preserving bit width, signedness, and semantic identity (e.g.
/// `Pubkey` vs `String`). Consumers use this information to decide
/// how to serialize, display, or validate values.
///
/// Neither `Serialize` nor JSON conversion is implemented here —
/// that decision belongs to consumers.
#[derive(Debug, Clone, PartialEq)]
pub enum IdlValue {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    Bool(bool),
    Pubkey(Pubkey),
    String(String),
    Bytes(Vec<u8>),
    /// Ordered named fields (struct, enum variant payload).
    Struct(Vec<(String, IdlValue)>),
    /// Ordered values (vec, fixed-size array, tuple).
    Array(Vec<IdlValue>),
    /// None / null (Option::None).
    Null,
}
