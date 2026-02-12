# Sonar Development Guide

CLI tool for simulating Solana transactions locally using LiteSVM.

## Project Structure

```
src/
├── main.rs                    # Entry point and command routing
├── cli/                       # CLI argument parsing and validation
│   ├── mod.rs                 # Top-level CLI struct, subcommand enum, shared args
│   ├── simulate.rs            # Simulate args, replacement/funding/patch parsing
│   ├── decode.rs              # Decode args
│   ├── account.rs             # Account args
│   ├── convert.rs             # Convert args and conversion logic
│   ├── pda.rs                 # PDA args and seed parsing
│   ├── program_data.rs        # Program data args
│   ├── send.rs                # Send args
│   └── fetch_idl.rs           # Fetch IDL args
├── handlers/                  # Command handler implementations
│   ├── mod.rs                 # Handler module declarations
│   ├── simulate.rs            # Simulate command handler
│   ├── decode.rs              # Decode command handler
│   ├── account.rs             # Account command handler
│   ├── convert.rs             # Convert command handler
│   ├── pda.rs                 # PDA command handler
│   ├── program_data.rs        # Program data command handler
│   ├── send.rs                # Send command handler
│   ├── fetch_idl.rs           # Fetch IDL command handler
│   └── completions.rs         # Shell completions handler
├── transaction.rs             # Transaction parsing, analysis, and signature detection
├── account_loader.rs          # RPC account fetching, caching, and transaction fetching
├── executor.rs                # Transaction simulation execution with account funding
├── funding.rs                 # SOL and token account funding logic
├── balance_changes.rs         # Balance change tracking and computation
├── config.rs                  # Config file loading (~/.config/sonar/config.toml)
├── progress.rs                # Progress indicator for long-running operations
├── output/                    # Result formatting and rendering
│   ├── mod.rs                 # Output module and shared types
│   ├── text.rs                # Human-readable text output
│   ├── json.rs                # JSON output format
│   └── report.rs              # Detailed report output
├── instruction_parsers/       # Instruction decoding for known programs
│   ├── mod.rs                 # Parser registry and dispatch
│   ├── system_program.rs      # System Program instruction parser
│   ├── token2022_program.rs   # SPL Token / Token-2022 instruction parser
│   ├── anchor_idl.rs          # Anchor IDL-based instruction parser
│   └── template.rs            # Parser template utilities
├── log_parser.rs              # Transaction log parsing
├── token_account_decoder.rs   # SPL Token account data decoding
└── native_ids.rs              # Well-known Solana program IDs and labels

tests/
├── e2e_simulation.rs          # Integration tests using assert_cmd
└── fixtures/                  # Compiled Solana programs (.so files)
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

# Run — simulate
cargo run -- simulate <BASE58_OR_BASE64_STRING> --rpc-url <RPC_URL>
cargo run -- simulate <TX1> <TX2> <TX3> --rpc-url <RPC_URL>  # bundle simulation
cargo run -- simulate <TX> --rpc-url <RPC_URL> --replace <PUBKEY>=<PATH_TO_FILE>
cargo run -- simulate <TX> --rpc-url <RPC_URL> --fund-sol <PUBKEY>=10sol
cargo run -- simulate <TX> --rpc-url <RPC_URL> -b -d  # balance changes + ix detail

# Run — decode (parse transaction without simulation)
cargo run -- decode <TX> --rpc-url <RPC_URL>

# Run — other subcommands
cargo run -- account <PUBKEY> --rpc-url <RPC_URL>
cargo run -- convert 0x48656c6c6f -t utf8
cargo run -- pda <PROGRAM_ID> "hello:string,<PUBKEY>:pubkey"
cargo run -- program-data <PROGRAM_ID> --rpc-url <RPC_URL>
cargo run -- send <SIGNED_TX> --rpc-url <RPC_URL>
cargo run -- fetch-idl <PROGRAM_ID> --rpc-url <RPC_URL>
cargo run -- completions zsh
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
