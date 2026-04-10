# Account, PDA, Program ELF, IDL, Send

## Account

Fetch and decode a Solana account (`account` or alias `acc`):

```bash
sonar account <PUBKEY> --rpc-url https://api.mainnet-beta.solana.com
sonar acc <PUBKEY> --rpc-url <RPC_URL>

# Output raw account data as base64 JSON (skip all decoding)
sonar account <PUBKEY> --rpc-url <RPC_URL> --raw

# Load local IDLs first (<OWNER_PROGRAM_ID>.json), then fallback to on-chain fetch
sonar account <PUBKEY> --rpc-url <RPC_URL> --idl-dir /path/to/idls
```

Decoded types: BPF upgradeable, Address Lookup Table, SPL Token/Token-2022, Anchor IDL; optional Metaplex metadata enrichment.

## PDA

Derive a Program Derived Address from seeds:

Seed types: `string` (`str`), `pubkey` (`pk`), `bool`, `u8`, `u16`, `u32`, `u64`, `u128`, `i8`, `i16`, `i32`, `i64`, `i128`, `bytes` (`hex`).

```bash
sonar pda <PROGRAM_ID> string:hello pubkey:<PUBKEY>

# Numeric seeds (little-endian)
sonar pda <PROGRAM_ID> string:position u64:42 u8:7

# Bool, signed integers, and raw bytes
sonar pda <PROGRAM_ID> bool:true i64:-1 bytes:deadbeef
```

## Program ELF

Get raw program data (ELF bytecode) from an upgradeable Program / ProgramData / Buffer account.
You must explicitly choose one output mode: `-o` (use `-o -` for stdout) or `--verify-sha256`.

```bash
# Save to file
sonar program-elf <PROGRAM_ID> --rpc-url <RPC_URL> -o program.so

# Stream raw bytes to stdout with Unix-style dash output target
sonar program-elf <PROGRAM_ID> --rpc-url <RPC_URL> -o - | shasum -a 256

# Verify SHA256 hash
sonar program-elf <PROGRAM_ID> --rpc-url <RPC_URL> --verify-sha256 <HEX_HASH>

# Fetch from a ProgramData account directly
sonar program-elf <PROGRAM_DATA_ADDRESS> --rpc-url <RPC_URL> -o program.so

# Fetch from a Buffer account directly (auto-detected)
sonar program-elf <BUFFER_ADDRESS> --rpc-url <RPC_URL> -o buffer.so
```

## IDL

Manage Anchor IDLs (fetch/sync/address). `idl fetch` and `idl sync` always exit 0, even when some programs fail. stdout contains only successfully written file paths; failure details and a summary go to stderr. Use `--json` to get per-program status for programmatic consumption.

```bash
# Fetch one IDL
sonar idl fetch <PROGRAM_ID> --rpc-url <RPC_URL>

# Fetch multiple IDLs
sonar idl fetch <PROGRAM_ID_1> <PROGRAM_ID_2> --rpc-url <RPC_URL> -o ./idls/

# Sync using an existing IDL directory (scan `<PUBKEY>.json` names)
sonar idl sync ./idls/ --rpc-url <RPC_URL>

# Sync one IDL by filename
sonar idl sync ./idls/<PROGRAM_ID>.json --rpc-url <RPC_URL>

# Derive Anchor IDL account address for a program
sonar idl address <PROGRAM_ID>
```

## Send

Send a signed transaction to the network. Outputs the transaction signature and an explorer URL. The explorer URL infers the cluster from `--rpc-url` (devnet/testnet get `?cluster=devnet` or `?cluster=testnet`; mainnet and unrecognized RPCs use the default mainnet view):

```bash
sonar send <SIGNED_TX> --rpc-url <RPC_URL>

# Devnet / testnet: explorer link includes cluster param
sonar send <SIGNED_TX> --rpc-url https://api.devnet.solana.com

# Skip preflight checks
sonar send <SIGNED_TX> --rpc-url <RPC_URL> --skip-preflight

# Wait for confirmation (default: confirmed, 30s timeout)
sonar send <SIGNED_TX> --rpc-url <RPC_URL> --wait

# Wait with custom commitment/timeout
sonar send <SIGNED_TX> --rpc-url <RPC_URL> --wait --wait-commitment finalized --wait-timeout-secs 60
```
