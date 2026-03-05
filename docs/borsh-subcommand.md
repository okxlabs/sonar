# `sonar borsh` Subcommand — Design Document

## Motivation

Solana programs use Borsh serialization extensively for account data and instruction arguments. Developers frequently need to inspect raw binary data or construct test payloads without writing Rust structs. The `sonar borsh` command provides a quick way to convert between Borsh binary and human-readable JSON using inline type descriptors.

## Usage

```bash
# Deserialize bytes → JSON
sonar borsh der "(u64,bool,vec<u32>)" 0x...

# Serialize JSON → bytes
sonar borsh ser "(u64,bool,vec<u32>)" '[1,true,[2,3]]'

# Stdin piping
echo '0x...' | sonar borsh der "u64"
echo '1' | sonar borsh ser "u64"

# Output format options for ser
sonar borsh ser "u64" '1' --output hex      # 0x0100000000000000 (default)
sonar borsh ser "u64" '1' --output base64   # AQAAAAAAAAA=
sonar borsh ser "u64" '1' --output bytes    # [1,0,0,0,0,0,0,0]
```

## Type Descriptor Grammar

```bnf
type       ::= primitive | generic | array | tuple
primitive  ::= "u8" | "u16" | "u32" | "u64" | "u128"
             | "i8" | "i16" | "i32" | "i64" | "i128"
             | "bool" | "string" | "str" | "pubkey"
generic    ::= ("vec" | "option") "<" type ">"
array      ::= "[" type ";" size "]"
tuple      ::= "(" type ("," type)* ")"
size       ::= [1-9][0-9]*
```

Whitespace is tolerated between all tokens. Type names are case-insensitive.

## Type Mapping

| Type | Borsh encoding | JSON representation |
|------|---------------|---------------------|
| `u8/u16/u32/u64` | 1/2/4/8 bytes LE | `Number` |
| `u128` | 16 bytes LE | `String` (decimal, for precision) |
| `i8/i16/i32/i64` | 1/2/4/8 bytes LE signed | `Number` |
| `i128` | 16 bytes LE signed | `String` (decimal, for precision) |
| `bool` | 1 byte (0 or 1) | `Bool` |
| `string` | 4-byte LE length + UTF-8 | `String` |
| `pubkey` | 32 bytes | `String` (base58) |
| `vec<T>` | 4-byte LE count + elements | `Array` |
| `option<T>` | 1-byte tag (0=None, 1=Some) + value | `Null` or value |
| `[T;N]` | N elements, no length prefix | `Array` |
| `(T1,T2,...)` | concatenated fields | `Array` |

## Parser Design

Recursive-descent parser operating on `&str` with a cursor index (`usize`). The parser:

1. Skips whitespace
2. Checks first character to determine structure (`(` → tuple, `[` → array)
3. For keywords, collects alphanumeric characters and matches against known types
4. For `vec` and `option`, expects `<`, recursively parses inner type, expects `>`
5. For arrays, expects `[`, recursively parses element type, expects `;`, parses size number, expects `]`
6. For tuples, expects `(`, parses comma-separated types, expects `)`

## Error Handling

- **Parser errors**: Include position information (e.g., "unknown type 'float32' at position 5")
- **Decode errors**: Include byte offset and context chain (e.g., "in vec element [3]: not enough bytes for u32")
- **Encode errors**: Include type and value mismatch details (e.g., "expected number for u64, got string")
- **Unconsumed bytes**: Warning (not error) when der doesn't consume all input bytes

## Input Formats (der)

The deserializer accepts bytes in three formats:
- **Hex**: `0x...` prefix
- **Byte array**: `[1,2,3,...]` decimal or `[0x01,0x02,...]` hex elements
- **Base64**: Standard base64 encoding (auto-detected as fallback)

## Output Formats (ser)

The serializer supports three output encodings via `--output`:
- `hex` (default): `0x` + hex string
- `base64`: Standard base64
- `bytes`: Decimal byte array `[1,2,3,...]`

## Implementation Files

| File | Purpose |
|------|---------|
| `src/converters/borsh_type.rs` | `BorshType` AST enum + recursive-descent parser |
| `src/converters/borsh_decode.rs` | `decode_borsh()`: bytes + type → JSON |
| `src/converters/borsh_encode.rs` | `encode_borsh()`: JSON + type → bytes |
| `src/cli/borsh.rs` | CLI argument structs (clap) |
| `src/handlers/borsh.rs` | Handler: input parsing, dispatch, output formatting |
