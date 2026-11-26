# Sonar — Solana transaction simulator & utilities

A CLI for local Solana transaction simulation (LiteSVM) plus developer utilities.

- Simulate transactions locally without deploying programs (raw tx, signature, bundle, or raw instructions)
- Parse legacy, v0, and v1 (SIMD-0385) transaction formats
- Override programs/accounts, fund SOL or tokens, patch data
- Store accounts once, replay without hitting RPC
- Serialize/deserialize Borsh with a type descriptor DSL
- Decode, convert, derive PDAs, inspect accounts, pull program ELF, send transactions, write program buffers

## Installation

```bash
git clone https://github.com/user/sonar.git
cd sonar
cargo build --release
# Binary at target/release/sonar
```

## Quick start

```bash
# Simulate a transaction locally
sonar simulate <BASE58_OR_BASE64_TX> --rpc-url https://api.mainnet-beta.solana.com

# Simulate from historical end-of-slot accounts (Helius RPC required)
sonar simulate <TX> --at-slot 460000000 --rpc-url <HELIUS_RPC_URL>

# Or fetch by signature (auto-detected)
sonar simulate <SIGNATURE> --rpc-url https://api.mainnet-beta.solana.com

# Or synthesize a transaction from one or more instruction specs
sonar simulate --payer <PUBKEY> \
  --ix 'program=MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr data=0x68656c6c6f'

# Decode an account
sonar account <PUBKEY> --rpc-url https://api.mainnet-beta.solana.com
```

`--at-slot <SLOT>` supports `simulate`, `decode`, `account`, `program-elf`,
and `idl fetch` with a Helius RPC endpoint. Accounts whose history is not tracked
use current state with a warning. See [limitations](docs/simulate.md#historical-slot-simulation)
and [historical inspection](docs/commands.md#historical-account-queries).
Historical reads use 4–16 concurrent requests based on logical core count,
with an 80 requests/second limit shared within the process.

## Command reference

| Command | Use when |
|---------|----------|
| `simulate` | Local execution logs, balance changes, and failure reasons |
| `decode` | Transaction structure (instructions/accounts) without execution |
| `replay` | Replay a confirmed transaction from on-chain metadata (no local simulation) |
| `account` | Decoded account metadata/data for a pubkey |
| `program-elf` | Raw ELF bytes from upgradeable program/buffer accounts |
| `idl` | Fetch/sync Anchor IDLs or derive an IDL account address |
| `send` | Submit a signed transaction to the network |
| `write-buffer` | Upload a program ELF into a buffer account with v1 transactions (like `solana program write-buffer`) |
| `borsh` | Serialize JSON to Borsh bytes or deserialize Borsh bytes to JSON |
| `convert` | Format conversion |
| `pda` | Derive a PDA from seeds |
| `config` | Inspect or update the config file (`SONAR_CONFIG` or `~/.config/sonar/config.toml`) |
| `cache` | List, clean, or inspect cached account data |
| `completions` | Generate shell completion scripts |

`write-buffer` prints the buffer address on stderr before any RPC request or transaction,
then reports upload stages and confirmed transaction counts.
Progress remains visible in pipes and logs. The final result includes elapsed time
from the start of command processing through transaction confirmation (`Elapsed: 12.34s`).
`--json` keeps stdout as JSON only and includes `elapsed_secs`.

## Documentation

- [Simulate & Decode](docs/simulate.md): full usage, overrides, funding, patching, cache
- [Transaction Formats](docs/transaction-formats.md): legacy / v0 / v1 parsing, v1 wire format and config
- [Account, PDA, Program ELF, IDL, Send, Write Buffer](docs/commands.md): per-command reference
- [Borsh](docs/borsh.md): serialization/deserialization with type descriptor DSL
- [Convert](docs/convert.md): format conversion reference
- [Configuration](docs/configuration.md): config file, cache, completions
- [Output Conventions](docs/output-conventions.md): stdout/stderr contract, permission markers, diagnostics

## License

MIT
