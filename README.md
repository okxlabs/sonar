# Sonar - Solana Transaction Simulator & Utilities

A command-line tool for simulating Solana transactions locally using LiteSVM, bundled with a handful of small utilities for everyday Solana development tasks.

## Features

### Transaction Simulation (core)

- Simulate transactions in a local SVM environment — no deployment needed
- Parse transactions from raw encoding (Base58/Base64) or fetch by signature from RPC
- Bundle simulation (multiple transactions in one run)
- Program and account replacement for testing custom behavior
- Fund system/token accounts with arbitrary amounts before simulation
- Patch account data, override clock/slot for fine-grained control
- Dump/load accounts for offline replay
- Decode transactions without simulation via `decode` subcommand
- Support for address lookup tables (ALT)
- Multiple output formats (text and JSON)

### Utilities

- **account** — Fetch and decode on-chain accounts (SPL Token, Token-2022, Anchor IDL, BPF Upgradeable, optional Metaplex metadata for mint accounts)
- **convert** — Data format conversion (hex, base58, base64, arrays, UTF-8, lamports, SOL)
- **pda** — PDA (Program Derived Address) derivation
- **program-data** — Extract program ELF bytecode from upgradeable programs/buffers
- **send** — Submit signed transactions to the network
- **fetch-idl** — Download Anchor IDLs from on-chain accounts
- **completions** — Shell completion scripts (bash, zsh, fish, elvish, powershell)

## Installation

```bash
# Clone the repository
git clone https://github.com/user/sonar.git
cd sonar

# Build release version
cargo build --release

# The binary will be at target/release/sonar
```

## Usage

### Command Overview

| Command | Use when |
|---------|----------|
| **`simulate`** | **You want local execution logs, balance changes, and failure reasons** |
| `decode` | You only need transaction structure (instructions/accounts) without execution |
| `account` | You want decoded account metadata/data for a pubkey |
| `program-data` | You need raw ELF bytes from upgradeable program/buffer accounts |
| `fetch-idl` | You want to download and persist Anchor IDLs locally |
| `send` | You want to submit a signed transaction to the network |
| `convert` | You want pure format conversion (hex/base58/base64/utf8/lamports/sol) |
| `pda` | You want to derive a PDA from seeds |

### Simulate

Simulate a Solana transaction locally using LiteSVM.

```bash
# Simulate a transaction with raw Base58/Base64 data
sonar simulate <BASE58_OR_BASE64_STRING> --rpc-url https://api.mainnet-beta.solana.com

# Simulate using transaction signature (auto-detected)
sonar simulate 2gTzNX3zLNhhmJaY44LycEgF8UMadrKeDLHz8rgcQVbXWVU4bs8fLBzWKhvAqKBeo2ttqyXsCeqUW47dfW6775Wu \
  --rpc-url https://api.mainnet-beta.solana.com

# Bundle simulation (multiple transactions)
sonar simulate <TX1> <TX2> <TX3> --rpc-url https://api.mainnet-beta.solana.com

# Read transaction from file via stdin
cat ./transaction.txt | sonar simulate --rpc-url <RPC_URL>

# Show balance changes and instruction details
sonar simulate <TX> --rpc-url <RPC_URL> -b -d

# JSON output
sonar simulate <TX> --rpc-url <RPC_URL> --json
```

#### Program & Account Replacement

Replace on-chain programs or accounts with local files for testing:

```bash
# Replace a program with a local .so file
sonar simulate <TX> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --replace TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA=./custom_token.so

# Replace an account with a local .json file
sonar simulate <TX> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --replace <PUBKEY>=./account.json
```

#### Account Funding

Fund system accounts with SOL or token accounts for testing:

```bash
# Fund a single account with SOL
sonar simulate <TX> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --fund-sol DPLezAkFZ5sFaBXMWt3J2StQwYtcqecUipWSP7YfrLth=10.5sol

# Fund multiple accounts
sonar simulate <TX> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --fund-sol <PUBKEY1>=100.0sol \
  --fund-sol <PUBKEY2>=2.75sol

# Fund a token account (raw amount)
sonar simulate <TX> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --fund-token <TOKEN_ACCOUNT>=1000000
```

#### Advanced Options

```bash
# Patch account data before simulation
sonar simulate <TX> --rpc-url <RPC_URL> \
  --patch-data <PUBKEY>=<OFFSET>:<HEX_DATA>

# Override clock timestamp and slot (Unix or RFC3339)
sonar simulate <TX> --rpc-url <RPC_URL> \
  --timestamp 1700000000 --slot 250000000
sonar simulate <TX> --rpc-url <RPC_URL> \
  --timestamp 2024-01-01T00:00:00Z --slot 250000000

# Dump/load accounts for offline simulation
sonar simulate <TX> --rpc-url <RPC_URL> --dump-accounts ./accounts/
sonar simulate <TX> --load-accounts ./accounts/ --offline

# Always print raw instruction data, even when parser succeeds
sonar simulate <TX> --rpc-url <RPC_URL> --raw-ix-data

# Verify transaction signatures during simulation
sonar simulate <TX> --rpc-url <RPC_URL> --check-sig

# Load Anchor IDL files from a custom directory
sonar simulate <TX> --rpc-url <RPC_URL> --idl-dir /path/to/idl/files/
```

### Decode

Decode and display a raw transaction without simulation:

```bash
sonar decode <TX> --rpc-url https://api.mainnet-beta.solana.com
sonar decode <TX> --rpc-url <RPC_URL> --json

# Always print raw instruction data, even when parser succeeds
sonar decode <TX> --rpc-url <RPC_URL> --raw-ix-data
```

### Account

Fetch and decode a Solana account:

```bash
sonar account <PUBKEY> --rpc-url https://api.mainnet-beta.solana.com

# Output raw account data as base64 JSON
sonar account <PUBKEY> --rpc-url <RPC_URL> --raw

# Skip account metadata
sonar account <PUBKEY> --rpc-url <RPC_URL> --no-account-meta

# For mint accounts, also try Metaplex metadata PDA decoding (opt-in)
sonar account <MINT_PUBKEY> --rpc-url <RPC_URL> --metadata
```

`--metadata` is opt-in and defaults to disabled. When enabled, Sonar will attempt to fetch and decode
the Metaplex metadata PDA for SPL Token legacy or Token-2022 mint accounts, and print only the
decoded metadata PDA content. If metadata is missing or cannot be decoded, the command returns an error.

### Convert

Convert between data formats:

```bash
# Auto-detect: hex (0x...) to decimal array
sonar convert 0x48656c6c6f -t dec-array

# Auto-detect: decimal integer to hex (little-endian by default)
sonar convert 255 -t hex

# Auto-detect: decimal float to lamports (treated as SOL)
sonar convert 1.5 -t lamports

# Auto-detect: hex-array ([0x..]) to UTF-8
sonar convert [0x48,0x65,0x6c,0x6c,0x6f] -t utf8

# Auto-detect: plain text to hex (non-base58 chars => utf8)
sonar convert "Hello World" -t hex

# Explicitly specify input format when needed
sonar convert SGVsbG8= -f base64 -t base58

# Lamports to SOL (explicit format)
sonar convert 1500000000 -f lamports -t sol

# Hex to UTF-8 (explicit format also supported)
sonar convert 0x48656c6c6f -f hex -t utf8
```

Auto-detection priority for `convert` input:

1. `0x...` / `0X...` -> `hex`
2. `[...]` -> `dec-array` or `hex-array` (if any element has `0x` prefix)
3. Contains `+`, `/`, or trailing `=` -> `base64`
4. All digits -> `number`
5. Decimal float (for example `1.5`) -> `sol`
6. Contains non-base58 characters (spaces, punctuation, Unicode, or `0/O/I/l`) -> `utf8`
7. Otherwise -> `base58`

If auto-detection fails to parse, Sonar will try safe fallback formats before returning an error.

### PDA

Derive a Program Derived Address from seeds:

```bash
sonar pda <PROGRAM_ID> string:hello pubkey:<PUBKEY>

# Numeric seeds (u64 little-endian, and single-byte u8)
sonar pda <PROGRAM_ID> string:position u64:42 u8:7
```

### Program Data

Get raw program data (ELF bytecode) from an upgradeable program or buffer:

```bash
sonar program-data <PROGRAM_ID> --rpc-url <RPC_URL>

# Save to file
sonar program-data <PROGRAM_ID> --rpc-url <RPC_URL> -o program.so

# Verify SHA256 hash
sonar program-data <PROGRAM_ID> --rpc-url <RPC_URL> --verify-sha256 <HEX_HASH>

# Fetch from a buffer account
sonar program-data <BUFFER_ADDRESS> --rpc-url <RPC_URL> --buffer
```

### Fetch IDL

Fetch Anchor IDL from on-chain program accounts:

```bash
sonar fetch-idl <PROGRAM_ID> --rpc-url <RPC_URL>

# Fetch multiple IDLs
sonar fetch-idl <PROGRAM_ID_1> <PROGRAM_ID_2> --rpc-url <RPC_URL> --output-dir ./idls/

# Sync existing IDL directory
sonar fetch-idl --sync-dir ./idls/ --rpc-url <RPC_URL>
```

### Send

Send a signed transaction to the network:

```bash
sonar send <SIGNED_TX> --rpc-url <RPC_URL>

# Skip preflight checks
sonar send <SIGNED_TX> --rpc-url <RPC_URL> --skip-preflight

# Wait for confirmation (default: confirmed, 30s timeout)
sonar send <SIGNED_TX> --rpc-url <RPC_URL> --wait

# Wait with custom commitment/timeout
sonar send <SIGNED_TX> --rpc-url <RPC_URL> --wait --wait-commitment finalized --wait-timeout-secs 60
```

### Completions

Generate shell completion scripts:

```bash
sonar completions bash > ~/.local/share/bash-completion/completions/sonar
sonar completions zsh > ~/.zsh/completions/_sonar
sonar completions fish > ~/.config/fish/completions/sonar.fish
```

## Configuration

Sonar reads configuration from `~/.config/sonar/config.toml`:

```toml
rpc_url = "https://api.mainnet-beta.solana.com"
idl_dir = "~/.sonar/idls"
color = "auto"  # auto, always, never

# Default for `simulate --show-balance-change`
show_balance_change = false
# Default for `simulate --show-ix-detail`
show_ix_detail = false
# Default for `simulate --raw-log`
raw_log = false
# Default for `simulate/decode --raw-ix-data`
raw_ix_data = false
# Default for `simulate --check-sig`
verify_signatures = false
# Default for `send --skip-preflight`
skip_preflight = false
```

Priority: CLI arguments > environment variables > config file > defaults.

## Technology Stack

- **Language**: Rust (Edition 2021)
- **Core Dependencies**:
  - `litesvm` - Local Solana Virtual Machine for transaction simulation
  - Fine-grained Solana crates (`solana-pubkey`, `solana-transaction`, `solana-account`, etc.) for better compile times
  - `solana-rpc-client` / `solana-rpc-client-api` - RPC interaction
  - `clap` - Command-line interface parsing
  - `serde` / `serde_json` - Serialization support
  - `anyhow` - Error handling
  - `base64` / `bs58` - Encoding/decoding
  - `colored` - Terminal color output

## Testing

```bash
# Run unit tests
cargo test

# Run integration tests (requires mainnet RPC access)
cargo test --test e2e_simulation -- --ignored --nocapture
```

**Note**: Some integration tests require mainnet RPC access and are marked with `#[ignore]`.

## License

MIT
