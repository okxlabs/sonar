# Sonar - Solana Transaction Simulator & Utilities

A CLI tool for local Solana transaction simulation (LiteSVM) plus common developer utilities.

## Features

### Transaction Simulation (core)

- Local simulation without deploying programs
- Parse raw tx (`base58`/`base64`) or fetch by signature
- Simulate bundles (multiple transactions in one run)
- Replace program/account data with local files
- Fund SOL or token accounts before simulation
- Patch account data; override timestamp and slot
- Cache accounts for offline replay (`--cache` stores to `~/.sonar/cache/`; replay without RPC when cache is complete)
- Decode-only mode via `decode`
- Supports ALT and text/JSON output

### Utilities

- **account**: decode on-chain accounts (BPF upgradeable, Address Lookup Table, SPL Token/Token-2022, Anchor IDL; optional Metaplex metadata enrichment)
- **convert**: explicit format conversions
- **pda**: derive program addresses from seeds
- **program-elf**: extract ELF from Program/ProgramData/Buffer accounts
- **send**: submit signed transactions
- **idl**: fetch/sync Anchor IDLs and derive IDL account address
- **completions**: generate shell completions
- **config**: list/get/set local sonar config values
- **cache**: list, clean, or inspect cached account data

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
| `program-elf` | You need raw ELF bytes from upgradeable program/buffer accounts |
| `idl` | You want to fetch/sync Anchor IDLs or derive an IDL account address |
| `send` | You want to submit a signed transaction to the network |
| `convert` | You want explicit and deterministic format conversion |
| `pda` | You want to derive a PDA from seeds |
| `config` | You want to inspect or update `~/.config/sonar/config.toml` |
| `cache` | You want to list, clean, or inspect cached account data |

### Output Stream Convention

- **stdout**: Machine-consumable primary results (stable fields, success output, confirmation messages).
  - `send`: signature, cluster-aware explorer URL (inferred from `--rpc-url`), and `--wait` confirmation (when used).
  - `program-elf -o <file>`: success message (bytes written, path).
  - Other commands: main structured output.
- **stderr**: Warnings, diagnostics, and errors only.

### Simulate

Simulate a Solana transaction locally using LiteSVM.
`sonar simulate --help` groups options by `Input & RPC`, `State Preparation`,
`Simulation Controls`, and `Output & Debug` for faster scanning.

```bash
# Simulate a transaction with raw Base58/Base64 data
sonar simulate <BASE58_OR_BASE64_STRING> --rpc-url https://api.mainnet-beta.solana.com

# Simulate using transaction signature (auto-detected)
sonar simulate 2gTzNX3zLNhhmJaY44LycEgF8UMadrKeDLHz8rgcQVbXWVU4bs8fLBzWKhvAqKBeo2ttqyXsCeqUW47dfW6775Wu \
  --rpc-url https://api.mainnet-beta.solana.com

# Bundle simulation (multiple transactions)
sonar simulate <TX1> <TX2> <TX3> --rpc-url https://api.mainnet-beta.solana.com

# Read transaction from stdin (omit TX to read from pipe)
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
# State Preparation: patch account data before simulation
sonar simulate <TX> --rpc-url <RPC_URL> \
  --patch-account-data <PUBKEY>=<OFFSET>:<HEX_DATA>

# Cache: store accounts for offline replay
sonar simulate <TX> --rpc-url <RPC_URL> --cache
# Replay from cache (no network; uses ~/.sonar/cache/ when cache is complete)
sonar simulate <TX> --cache
# Force refresh cache
sonar simulate <TX> --rpc-url <RPC_URL> --cache --refresh-cache

# Simulation Controls: override clock timestamp and slot (Unix or RFC3339)
sonar simulate <TX> --rpc-url <RPC_URL> \
  --timestamp 1700000000 --slot 250000000
sonar simulate <TX> --rpc-url <RPC_URL> \
  --timestamp 2024-01-01T00:00:00Z --slot 250000000

# Simulation Controls: verify transaction signatures during simulation
sonar simulate <TX> --rpc-url <RPC_URL> --check-sig

# Simulation Controls: load Anchor IDL files from a custom directory
sonar simulate <TX> --rpc-url <RPC_URL> --idl-dir /path/to/idl/files/

# Output & Debug: always print raw instruction data, even when parser succeeds
sonar simulate <TX> --rpc-url <RPC_URL> --raw-ix-data

# Output & Debug: print raw logs and full instruction details
sonar simulate <TX> --rpc-url <RPC_URL> --raw-log --show-ix-detail
```

### Decode

Decode and display a raw transaction without simulation:

```bash
sonar decode <TX> --rpc-url https://api.mainnet-beta.solana.com
sonar decode <TX> --rpc-url <RPC_URL> --json

# Bundle decode (multiple TXs): --json outputs a single JSON array [{...}, {...}]
# Parseable by jq: sonar decode <TX1> <TX2> --json --rpc-url <RPC_URL> | jq .
sonar decode <TX1> <TX2> --rpc-url <RPC_URL> --json

# Read transaction from stdin (omit TX to read from pipe)
cat ./transaction.txt | sonar decode --rpc-url <RPC_URL>

# Always print raw instruction data, even when parser succeeds
sonar decode <TX> --rpc-url <RPC_URL> --raw-ix-data
```

### Account

Fetch and decode a Solana account (`account` or alias `acc`):

```bash
sonar account <PUBKEY> --rpc-url https://api.mainnet-beta.solana.com
sonar acc <PUBKEY> --rpc-url <RPC_URL>

# Output raw account data as base64 JSON (skip all decoding)
sonar account <PUBKEY> --rpc-url <RPC_URL> --raw

# Load local IDLs first (<OWNER_PROGRAM_ID>.json), then fallback to on-chain fetch
sonar account <PUBKEY> --rpc-url <RPC_URL> --idl-dir /path/to/idls

# For SPL Token / Token-2022 mint accounts, opt-in Metaplex metadata PDA decoding
sonar account <MINT_PUBKEY> --rpc-url <RPC_URL> --mpl-metadata
sonar account <MINT_PUBKEY> --rpc-url <RPC_URL> -m
```

### Convert

Format conversion with explicit syntax:

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
```

Supported formats:

- Generic: `int`, `hex`, `hex-bytes`, `bytes`, `text`, `binary`, `base64`, `base58`, `lamports`, `sol`
- Solana: `pubkey` (32-byte), `signature` (64-byte), `keypair` (64-byte; alias `kp`)
- Fixed-width integers: `u8`, `u16`, `u32`, `u64`, `u128`, `i8`, `i16`, `i32`, `i64`, `i128`

Validation rules:

- `TO=pubkey` requires 32 bytes.
- `TO=signature` requires 64 bytes.
- `FROM=keypair` requires 64 bytes (`secret[32] + pubkey[32]`).
- `TO=u/iN` enforces exact width for byte-oriented input (`hex`, `bytes`, `base64`, `base58`).

### PDA

Derive a Program Derived Address from seeds:

```bash
sonar pda <PROGRAM_ID> string:hello pubkey:<PUBKEY>

# Numeric seeds (u64 little-endian, and single-byte u8)
sonar pda <PROGRAM_ID> string:position u64:42 u8:7
```

### Program ELF

Get raw program data (ELF bytecode) from an upgradeable Program / ProgramData / Buffer account:
you must explicitly choose one output mode: `-o` (use `-o -` for stdout) or `--verify-sha256`.

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

### IDL

Manage Anchor IDLs (fetch/sync/address). By default, `idl fetch` and `idl sync` exit with a non-zero code if any program fails (not found or RPC error). stdout contains only successfully written file paths; failure details and a summary go to stderr. Use `--allow-partial` to exit 0 even when some programs fail.

```bash
# Fetch one IDL
sonar idl fetch <PROGRAM_ID> --rpc-url <RPC_URL>

# Fetch multiple IDLs
sonar idl fetch <PROGRAM_ID_1> <PROGRAM_ID_2> --rpc-url <RPC_URL> -o ./idls/

# Allow partial success (exit 0 when some fail)
sonar idl fetch <PROGRAM_ID_1> <PROGRAM_ID_2> --rpc-url <RPC_URL> --allow-partial

# Sync using an existing IDL directory (scan `<PUBKEY>.json` names)
sonar idl sync ./idls/ --rpc-url <RPC_URL>

# Sync one IDL by filename
sonar idl sync ./idls/<PROGRAM_ID>.json --rpc-url <RPC_URL>

# Derive Anchor IDL account address for a program
sonar idl address <PROGRAM_ID>
```

### Send

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

### Completions

Generate shell completion scripts:

```bash
sonar completions bash > ~/.local/share/bash-completion/completions/sonar
sonar completions zsh > ~/.zsh/completions/_sonar
sonar completions fish > ~/.config/fish/completions/sonar.fish
```

### Config

View or modify `~/.config/sonar/config.toml`:

```bash
# List all supported config items
sonar config list

# Get one config value
sonar config get show_ix_detail

# Set one config value
sonar config set show_ix_detail=true

# Alternative assignment form
sonar config set show_ix_detail true
```

### Cache

Manage cached account data for offline simulation:

```bash
# Cache accounts for offline replay (writes to ~/.sonar/cache/ by default)
sonar simulate <TX> --rpc-url <RPC_URL> --cache

# Replay from cache (no network access when cache is complete)
sonar simulate <TX> --cache

# Force refresh cache
sonar simulate <TX> --rpc-url <RPC_URL> --cache --refresh-cache

# Custom cache directory
sonar simulate <TX> --rpc-url <RPC_URL> --cache --cache-dir /path/to/cache

# Manage cache
sonar cache list
sonar cache clean --older-than 7d
sonar cache info <KEY>
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

## Account Permission Markers

In the text output of `simulate` and `decode`, each account is annotated with a compact
permission marker in the form `[<src>:<sig><perm><exe>]`:

| Position | Values | Meaning |
|----------|--------|---------|
| **src**  | `s` / `l` | Account source: **s**tatic (in transaction) or **l**ookup table |
| **sig**  | `S` / `-` | **S**igner or not |
| **perm** | `w` / `r` | **w**ritable or **r**ead-only |
| **exe**  | `x` / `-` | e**x**ecutable (program) or not |

Examples: `[s:Sw-]` = static, signer, writable; `[l:-r-]` = lookup table, non-signer, read-only.

The `[n]` label next to a marker refers to the account's index in the Account List section.

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
