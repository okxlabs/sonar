# Sonar Development Guide

CLI tool for simulating Solana transactions locally using LiteSVM.

## Project Structure

```
src/
├── main.rs           # Entry point and command routing
├── cli.rs            # CLI argument parsing and validation
├── transaction.rs    # Transaction parsing, analysis, and signature detection
├── account_loader.rs # RPC account fetching, caching, and transaction fetching
├── executor.rs       # Transaction simulation execution with account funding
└── output.rs         # Result formatting and rendering

tests/
├── e2e_simulation.rs # Integration tests using assert_cmd
└── fixtures/         # Compiled Solana programs (.so files)
```

## Commands

```bash
# Build
cargo build
cargo build --release
cargo check

# Format and lint (run before committing)
cargo fmt
cargo fmt --check
cargo clippy -- -D warnings

# Test
cargo test
cargo test --test e2e_simulation -- --ignored --nocapture
cargo test <test_name>

# Run
cargo run -- simulate <BASE58_OR_BASE64_STRING> --rpc-url <RPC_URL>
cargo run -- simulate <TRANSACTION> --rpc-url <RPC_URL> --parse-only
cargo run -- simulate <TRANSACTION> --rpc-url <RPC_URL> --replace <PROGRAM_ID>=<PATH_TO_SO_FILE>
cargo run -- simulate <TX1> <TX2> <TX3> --rpc-url <RPC_URL>  # bundle simulation
```

## Code Style

### Naming Conventions
- **Variables/Functions**: snake_case
- **Types/Structs**: PascalCase
- **Constants**: UPPER_SNAKE_CASE
- **Error messages**: English

### Error Handling
- Use `anyhow::Result<T>` for error propagation
- Provide context with `.context()` for better error traces

### Module Organization
- Each module has a clear single responsibility
- Use `pub(crate)` for internal visibility when appropriate

## Commit Conventions
- Follow the **Conventional Commits** standard
- Before every commit, ensure **zero `cargo clippy` warnings** — treat all clippy warnings as errors (`cargo clippy -- -D warnings`)

## Key Patterns

1. **Transaction Flow**: Raw input → Parse → Load accounts → Simulate → Render output
2. **Signature Detection**: Auto-detects tx signatures and fetches from RPC
3. **Account Loading**: RPC fetching with caching, handles upgradeable programs
4. **Program Replacement**: Replace on-chain programs with local .so files for testing
5. **Account Funding**: Fund system accounts with custom SOL amounts before simulation
6. **Fine-Grained Imports**: Uses individual Solana crates (solana-pubkey, solana-transaction, etc.) instead of monolithic solana-sdk for better compile times

## Development Workflow

1. Make changes to relevant module(s)
2. Run `cargo check` to ensure compilation
3. Run `cargo test` for unit tests
4. For integration tests, ensure RPC access or use mocks
5. Test CLI manually with sample transactions
6. Follow existing code style and English error message convention
7. Run `cargo clippy -- -D warnings` and fix all warnings before committing
8. Run `cargo fmt` before committing changes
