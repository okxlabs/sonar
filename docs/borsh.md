# Borsh

Serialize and deserialize Borsh-encoded binary data using a type descriptor DSL (`borsh`).

## Deserialize (`borsh de`)

Decode Borsh bytes into JSON:

```bash
# Primitives
sonar borsh de "u64" 0x2a00000000000000          # → 42
sonar borsh de "bool" 0x01                        # → true
sonar borsh de "string" 0x0500000068656c6c6f      # → "hello"

# Pubkey (32-byte Solana public key → base58)
sonar borsh de "pubkey" 0x$(printf '0%.0s' {1..64})  # → "1111...1111"

# Containers
sonar borsh de "vec<u32>" 0x020000000100000002000000     # → [1, 2]
sonar borsh de "option<u64>" 0x012a00000000000000         # → 42
sonar borsh de "option<u64>" 0x00                         # → null
sonar borsh de "[u8;4]" 0x01020304                        # → [1, 2, 3, 4]

# Tuple
sonar borsh de "(u64,bool,string)" 0x01000000000000000105000000776f726c64
# → [1, true, "world"]

# Struct (named fields → JSON object)
sonar borsh de "{amount:u64,active:bool}" 0x2a0000000000000001
# → {"amount": 42, "active": true}

# Enum (variant index → JSON with variant number)
sonar borsh de "enum<(),u64,(u32,bool)>" 0x012a00000000000000
# → {"variant": 1, "value": 42}
sonar borsh de "enum<(),u64,(u32,bool)>" 0x00
# → {"variant": 0}

# Result
sonar borsh de "result<u64,string>" 0x002a00000000000000
# → {"ok": 42}
sonar borsh de "result<u64,string>" 0x01040000006661696c
# → {"err": "fail"}

# HashSet and HashMap
sonar borsh de "hashset<u32>" 0x020000000100000002000000  # → [1, 2]
sonar borsh de "hashmap<u32,bool>" 0x0200000001000000010200000000
# → [[1, true], [2, false]]

# Skip discriminator bytes (common for Anchor accounts)
sonar borsh de --skip-bytes 8 "{owner:pubkey,balance:u64}" 0x...

# Read from stdin
echo '0x2a00000000000000' | sonar borsh de "u64"
```

Input formats for `de`: hex (`0x...`), base64, or byte array (`[1,2,3]`).

## Serialize (`borsh ser`)

Encode a JSON value into Borsh bytes:

```bash
# Primitives
sonar borsh ser "u64" '42'                # → 0x2a00000000000000
sonar borsh ser "bool" 'true'             # → 0x01
sonar borsh ser "string" '"hello"'        # → 0x0500000068656c6c6f

# Containers
sonar borsh ser "vec<u32>" '[1,2]'        # → 0x020000000100000002000000
sonar borsh ser "option<u64>" '42'        # → 0x012a00000000000000
sonar borsh ser "option<u64>" 'null'      # → 0x00

# Struct
sonar borsh ser "{amount:u64,active:bool}" '{"amount":42,"active":true}'
# → 0x2a0000000000000001

# Enum (use {"variant": N, "value": ...} for payloads, {"variant": N} for unit)
sonar borsh ser "enum<(),u64>" '{"variant":1,"value":42}'
# → 0x012a00000000000000
sonar borsh ser "enum<(),u64>" '{"variant":0}'
# → 0x00

# Result (use {"ok": ...} or {"err": ...})
sonar borsh ser "result<u64,string>" '{"ok":42}'
# → 0x002a00000000000000

# Nested: struct with enum and vec
sonar borsh ser "{action:enum<(),u64,vec<u8>>,tags:vec<string>}" \
  '{"action":{"variant":2,"value":[1,2,3]},"tags":["alpha","beta"]}'

# Prepend a hex prefix (e.g. an Anchor discriminator)
sonar borsh ser --prefix 0xaf2083f50e3050c6 "{amount:u64}" '{"amount":42}'
# → 0xaf2083f50e3050c62a00000000000000

# Read from stdin
echo '{"amount":100}' | sonar borsh ser "{amount:u64}"

# Convert output to other formats via pipe
sonar borsh ser "u64" '42' | sonar convert hex base64
sonar borsh ser "u64" '42' | sonar convert hex bytes
```

## Type Descriptor Reference

| Type | Syntax | JSON representation |
|------|--------|-------------------|
| Integers | `u8`, `u16`, `u32`, `u64`, `i8`–`i64` | number |
| Large integers | `u128`, `i128` | string (decimal) |
| Boolean | `bool` | `true` / `false` |
| String | `string` (alias: `str`) | string |
| Pubkey | `pubkey` | base58 string |
| Vec | `vec<T>` | array |
| Option | `option<T>` | value or `null` |
| Fixed array | `[T;N]` | array of N elements |
| Tuple | `(T1,T2,...)` | array |
| Unit | `()` | `null` |
| Struct | `{name:T,...}` | object with named fields |
| Enum | `enum<T0,T1,...>` | `{"variant":N}` or `{"variant":N,"value":...}` |
| Result | `result<T,E>` | `{"ok":...}` or `{"err":...}` |
| HashSet | `hashset<T>` (alias: `set`) | array |
| HashMap | `hashmap<K,V>` (alias: `map`) | array of `[key, value]` pairs |

Types compose arbitrarily: `vec<{owner:pubkey,balances:hashmap<string,u64>}>`.
