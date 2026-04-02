# Convert

Format conversion with explicit syntax (`convert`, alias: `conv`):

```bash
# Syntax
sonar convert <FROM> <TO> <INPUT>

# Common examples
sonar convert hex text 0x48656c6c6f
sonar convert bytes int "[12,34]"
sonar convert sol lamports 1.5
sonar convert pubkey hex 11111111111111111111111111111111
sonar convert signature bytes <SIGNATURE>
sonar convert keypair pubkey 0x<64-byte-keypair-hex>
sonar convert u64 hex 1000000000

# Read INPUT from stdin
echo "0x48656c6c6f" | sonar convert hex text

# Optional flags
sonar convert int hex 305419896 --le
sonar convert hex bytes 0x48656c6c6f --sep " "
sonar convert hex hex-bytes 0x48656c6c6f --no-prefix
sonar convert hex text 0x48656cff6f --escape
```

## Supported Formats

- Generic (input & output): `int`, `hex`, `hex-bytes` (`hb`), `bytes`, `text`, `binary` (`bin`), `base64` (`b64`), `base58` (`b58`), `lamports` (`lam`), `sol`
- Solana (input & output): `pubkey` (`pk`, 32-byte), `signature` (`sig`, 64-byte)
- Solana (input only): `keypair` (`kp`, 64-byte)
- Fixed-width integers: `u8`, `u16`, `u32`, `u64`, `u128`, `u256`, `i8`, `i16`, `i32`, `i64`, `i128`, `i256`

## Validation Rules

- `TO=pubkey` requires 32 bytes.
- `TO=signature` requires 64 bytes.
- `FROM=keypair` requires 64 bytes (`secret[32] + pubkey[32]`).
- `TO=u/iN` enforces exact width for byte-oriented input (`hex`, `bytes`, `base64`, `base58`).
