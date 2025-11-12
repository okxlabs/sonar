# Solsim - Solana Transaction Simulator

## Project Overview

`solsim` is a command-line tool for simulating Solana transactions locally using LiteSVM. It allows developers to test and debug Solana transactions without deploying to a live network by providing raw transaction data (Base58 or Base64 encoded) and optionally replacing on-chain programs with local .so files for testing.

**Key Features:**
- Parse and analyze Solana transactions from raw encoding (Base58/Base64)
- **Fetch and parse transactions directly from Solana RPC using transaction signatures**
- Simulate transactions in a local SVM environment
- Support for address lookup tables (ALT)
- Program replacement for testing custom program behavior
- **Fund system accounts with arbitrary SOL amounts for testing**
- Multiple output formats (text and JSON)
- Account loading from Solana RPC nodes
- Parse-only mode for transaction analysis without simulation

## Technology Stack

- **Language**: Rust (Edition 2021)
- **Core Dependencies**:
  - `litesvm` - Local Solana Virtual Machine for transaction simulation
  - `solana-sdk` / `solana-client` - Solana blockchain interaction
  - `solana-rpc-client-types` / `solana-transaction-status-client-types` - RPC configuration and transaction encoding types
  - `clap` - Command-line interface parsing
  - `serde` / `serde_json` - Serialization support
  - `anyhow` - Error handling
  - `base64` / `bs58` - Transaction encoding/decoding

## Project Structure

```
src/
├── main.rs           # Entry point and command routing (145 lines)
├── cli.rs            # CLI argument parsing and validation (125 lines)
├── transaction.rs    # Transaction parsing, analysis, and signature detection (1,066 lines)
├── account_loader.rs # RPC account fetching, caching, and transaction fetching (386 lines)
├── executor.rs       # Transaction simulation execution with account funding (180 lines)
└── output.rs         # Result formatting and rendering (762 lines)

tests/
├── e2e_simulation.rs # Integration tests using assert_cmd
└── fixtures/         # Compiled Solana programs (.so files)
    ├── dex_solana_v3.so
    └── spl_token.so
```

## Build and Development Commands

### Building
```bash
# Build the project
cargo build

# Build release version
cargo build --release

# Check for compilation errors
cargo check
```

### formatting
```bash
# Format the codebase, run it before committing
cargo fmt
# Check code formatting without making changes
cargo fmt --check
```

### Testing
```bash
# Run unit tests
cargo test

# Run integration tests (requires mainnet RPC access)
cargo test --test e2e_simulation -- --ignored --nocapture

# Run specific test
cargo test <test_name>
```

### Running
```bash
# Simulate a transaction
cargo run -- simulate --tx <BASE58_OR_BASE64_STRING> --rpc-url <RPC_URL>

# Parse transaction only (skip simulation)
cargo run -- simulate --tx <BASE58_OR_BASE64_STRING> --rpc-url <RPC_URL> --parse-only

# With program replacement
cargo run -- simulate \
  --tx <TRANSACTION> \
  --rpc-url <RPC_URL> \
  --replace <PROGRAM_ID>=<PATH_TO_SO_FILE> \
  --output json
```

## Code Style Guidelines

### Naming Conventions
- **Variables/Functions**: snake_case (Rust standard)
- **Types/Structs**: PascalCase
- **Constants**: UPPER_SNAKE_CASE
- **Error messages**: Written in English

### Error Handling
- Use `anyhow::Result<T>` for error propagation
- Provide context with `.context()` for better error traces
- Error messages should be descriptive and in English

### Module Organization
- Each module has a clear single responsibility
- Keep module sizes manageable (largest is transaction.rs at ~1k lines)
- Use `pub(crate)` for internal visibility when appropriate

### Key Patterns
1. **Transaction Flow**: Raw input → Parse → Load accounts → Simulate → Render output
2. **Signature Detection**: Auto-detects 88-character base58 signatures and fetches from RPC
3. **Account Loading**: RPC fetching with caching, handles upgradeable programs
4. **Program Replacement**: Replace on-chain programs with local .so files for testing
5. **Account Funding**: Fund system accounts with custom SOL amounts before simulation

## Testing Strategy

### Unit Tests
- Embedded within source files where appropriate
- Focus on individual function behavior

### Integration Tests
- Located in `tests/e2e_simulation.rs`
- Use `assert_cmd` for CLI testing
- Test `simulate` command with and without `--parse-only` flag
- Include tests for program replacement functionality
- **Important**: Some tests require mainnet RPC access and are marked with `#[ignore]`

### Test Fixtures
- Pre-compiled Solana programs in `tests/fixtures/`
- Used for testing program replacement feature
- Currently includes SPL Token and a DEX program

## Security Considerations

1. **RPC URL Handling**: Default uses mainnet-beta, but can be configured
2. **Program Replacement**: Allows loading arbitrary .so files - use with caution
3. **Account Caching**: Thread-safe caching with Mutex for concurrent access
4. **Input Validation**: Comprehensive validation for transaction encoding and CLI arguments

## Performance Notes

- Account loading is batched (MAX_ACCOUNTS_PER_REQUEST = 100)
- Uses caching to avoid redundant RPC calls
- LiteSVM provides efficient local simulation
- Handles address lookup tables efficiently

## Dependencies and Versions

- Rust 1.91.0 or later
- Solana SDK 2.2.x / 3.0.x series (mixed versions for different components)
- LiteSVM 0.8.1
- All dependencies managed through Cargo

**Key Dependency Versions:**
- `solana-sdk` / `solana-client` / `solana-message` / `solana-address-lookup-table-interface`: 2.2.x series
- `solana-transaction` / `solana-account` / `solana-pubkey`: 3.0.x / 4.0.x series
- `solana-rpc-client-types` / `solana-transaction-status-client-types`: 2.2.1 (for RPC configuration)
- `litesvm`: 0.8.1
- `clap`: 4.5.x with derive feature
- `serde` / `serde_json`: 1.0.x

## CLI Usage Examples

```bash
# Basic simulation with raw transaction
solsim simulate --tx <BASE58_STRING> --rpc-url https://api.mainnet-beta.solana.com

# Simulation using transaction signature (auto-detected)
solsim simulate --tx 2gTzNX3zLNhhmJaY44LycEgF8UMadrKeDLHz8rgcQVbXWVU4bs8fLBzWKhvAqKBeo2ttqyXsCeqUW47dfW6775Wu \
  --rpc-url https://api.mainnet-beta.solana.com

# With program replacement
solsim simulate \
  --tx <BASE58_STRING> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --replace TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA=./custom_token.so \
  --output json

# Parse transaction only (from signature) - skip simulation
solsim simulate --tx 2gTzNX3zLNhhmJaY44LycEgF8UMadrKeDLHz8rgcQVbXWVU4bs8fLBzWKhvAqKBeo2ttqyXsCeqUW47dfW6775Wu \
  --rpc-url https://api.mainnet-beta.solana.com \
  --parse-only

# Read transaction from file
solsim simulate --tx-file ./transaction.txt --rpc-url <RPC_URL>

# Fund system accounts with SOL for testing
solsim simulate \
  --tx <TRANSACTION_SIGNATURE> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --fund-sol 11111111111111111111111111111111=10.5

# Fund multiple accounts with different amounts
solsim simulate \
  --tx <TRANSACTION_SIGNATURE> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --fund-sol DPLezAkFZ5sFaBXMWt3J2StQwYtcqecUipWSP7YfrLth=100.0 \
  --fund-sol 7xP9jZBWvmpqE2J8V3jfgjuktJ2cFJJ3pU7Q5iQj1kJQ=2.75 \
  --output json

# Combine program replacement and account funding
solsim simulate \
  --tx <TRANSACTION> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --replace TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA=./custom_token.so \
  --fund-sol SomeAccountPubkey=50.0 \
  --output json
```

## Development Workflow

1. Make changes to relevant module(s)
2. Run `cargo check` to ensure compilation
3. Run `cargo test` for unit tests
4. For integration tests, ensure RPC access or use mocks
5. Test CLI manually with sample transactions
6. Follow existing code style and English error message convention
