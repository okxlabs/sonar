# Account, PDA, Program ELF, IDL, Send

## Account

Fetch and decode a Solana account (`account` or alias `acc`):

```bash
sonar account <PUBKEY> --rpc-url https://api.mainnet-beta.solana.com
sonar acc <PUBKEY> --rpc-url <RPC_URL>

# Output raw account data as base64 JSON (skip all decoding)
sonar account <PUBKEY> --rpc-url <RPC_URL> --raw

# Load local IDLs first, then fallback to on-chain fetch.
# Files are matched by name (<PROGRAM_ID>.json) or by the `address` field
# declared inside them, so arbitrarily-named IDL files work too.
sonar account <PUBKEY> --rpc-url <RPC_URL> --idl-dir /path/to/idls
```

Decoded types: BPF upgradeable, Address Lookup Table, SPL Token/Token-2022, Anchor IDL; optional Metaplex metadata enrichment.

## Historical Account Queries

Use `--at-slot <SLOT>` with a Helius RPC endpoint that supports Historical Accounts:

```bash
sonar account <PUBKEY> --at-slot 469732029 --rpc-url <HELIUS_RPC_URL>
sonar account <PUBKEY> --at-slot 469732029 --rpc-url <HELIUS_RPC_URL> --raw
sonar program-elf <PROGRAM_ID> --at-slot 469732029 --rpc-url <HELIUS_RPC_URL> -o old.so
sonar idl fetch <PROGRAM_ID> --at-slot 469732029 --rpc-url <HELIUS_RPC_URL> -o ./old-idls
```

- Queries return account state at the **end of the slot**.
- Historical reads use `clamp(logical cores × 2, 4, 16)` concurrent requests
  and at most 80 requests/second, shared across historical providers and batches
  in one process. Fallback and retry requests count toward the same limits.
  `RUST_LOG=debug` shows the selected concurrency.
- `account` requires a pubkey in historical mode; file/stdin input is rejected.
  Metadata and on-chain IDL requests use the same slot. Explicit local IDLs
  remain user overrides.
- `program-elf` reads both Program and ProgramData at the selected slot.
  Direct ProgramData/Buffer input and hash verification also support this option.
- `idl fetch` reads the historical on-chain IDL, not a historical off-chain IDL.
  A missing account remains not found; no current IDL is substituted.
  Without `-o`, output goes under `historical/<SLOT>/` within the normal
  output directory, so current IDLs are not overwritten.
- An endpoint that does not implement Historical Accounts returns a concise
  error explaining that `--at-slot` needs a Helius endpoint with this feature.
  This does not trigger current-state fallback.
- When the API explicitly reports that account history is not tracked,
  Sonar reads current state and warns. Other RPC failures remain errors.
  A historical `null` is not replaced with current state.
- Unlike simulation, account inspection does not construct Clock or use VM
  SlotHashes defaults: unsupported Sysvars use current-state fallback.
- `idl sync`, `idl address`, `replay`, and `send` do not accept this option.

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
