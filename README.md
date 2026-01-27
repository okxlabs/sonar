# Solsim - Solana Transaction Simulator

A command-line tool for simulating Solana transactions locally using LiteSVM. Test and debug Solana transactions without deploying to a live network.

## Features

- Parse and analyze Solana transactions from raw encoding (Base58/Base64)
- Fetch and parse transactions directly from Solana RPC using transaction signatures
- Simulate transactions in a local SVM environment
- Support for address lookup tables (ALT)
- Program replacement for testing custom program behavior
- Fund system accounts with arbitrary SOL amounts for testing
- Multiple output formats (text and JSON)
- Account loading from Solana RPC nodes
- Load IDL files from custom directories for Anchor program instruction parsing
- Parse-only mode for transaction analysis without simulation

## Installation

```bash
# Clone the repository
git clone https://github.com/user/solsim.git
cd solsim

# Build release version
cargo build --release

# The binary will be at target/release/solsim
```

## Usage

### Basic Simulation

```bash
# Simulate a transaction with raw Base58/Base64 data
solsim simulate --tx <BASE58_STRING> --rpc-url https://api.mainnet-beta.solana.com

# Simulate using transaction signature (auto-detected)
solsim simulate --tx 2gTzNX3zLNhhmJaY44LycEgF8UMadrKeDLHz8rgcQVbXWVU4bs8fLBzWKhvAqKBeo2ttqyXsCeqUW47dfW6775Wu \
  --rpc-url https://api.mainnet-beta.solana.com

# Read transaction from file
solsim simulate --tx-file ./transaction.txt --rpc-url <RPC_URL>
```

### Parse-Only Mode

```bash
# Parse transaction without simulation
solsim simulate --tx 2gTzNX3zLNhhmJaY44LycEgF8UMadrKeDLHz8rgcQVbXWVU4bs8fLBzWKhvAqKBeo2ttqyXsCeqUW47dfW6775Wu \
  --rpc-url https://api.mainnet-beta.solana.com \
  --parse-only
```

### Program Replacement

Replace on-chain programs with local .so files for testing:

```bash
solsim simulate \
  --tx <BASE58_STRING> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --replace TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA=./custom_token.so \
  --output json
```

### Account Funding

Fund system accounts with SOL for testing:

```bash
# Fund a single account
solsim simulate \
  --tx <TRANSACTION_SIGNATURE> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --fund-sol 11111111111111111111111111111111=10.5

# Fund multiple accounts
solsim simulate \
  --tx <TRANSACTION_SIGNATURE> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --fund-sol DPLezAkFZ5sFaBXMWt3J2StQwYtcqecUipWSP7YfrLth=100.0 \
  --fund-sol 7xP9jZBWvmpqE2J8V3jfgjuktJ2cFJJ3pU7Q5iQj1kJQ=2.75 \
  --output json
```

### IDL Loading

Load IDL files from a custom directory for Anchor program instruction parsing:

```bash
solsim simulate --tx <TRANSACTION_SIGNATURE> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --idl-path /path/to/idl/files/
```

### Combined Options

```bash
solsim simulate \
  --tx <TRANSACTION> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --replace TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA=./custom_token.so \
  --fund-sol SomeAccountPubkey=50.0 \
  --output json
```

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
