/// Domain-native value type for decoded IDL data.
///
/// Unlike `serde_json::Value`, this preserves the full range of Solana numeric
/// types (u128/i128) without forcing stringification for values that exceed
/// JSON's native integer range.
///
/// Neither `Serialize` nor JSON conversion is implemented here — consumers
/// decide how to render `IdlValue` via their own conversion logic.
#[derive(Debug, Clone, PartialEq)]
pub enum IdlValue {
    /// Unsigned integer (u8, u16, u32, u64, u128).
    Uint(u128),
    /// Signed integer (i8, i16, i32, i64, i128).
    Int(i128),
    /// Boolean.
    Bool(bool),
    /// UTF-8 string (pubkeys, seeds, string fields).
    String(String),
    /// Byte array (`bytes` type in Anchor IDL).
    Bytes(Vec<u8>),
    /// Ordered named fields (struct, enum variant payload).
    Struct(Vec<(String, IdlValue)>),
    /// Ordered values (vec, fixed-size array, tuple).
    Array(Vec<IdlValue>),
    /// None / null (Option::None).
    Null,
}
