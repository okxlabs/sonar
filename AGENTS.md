# Sonar Development Guide

CLI for local Solana transaction simulation (LiteSVM) plus utility subcommands.

## Project Map

- `src/main.rs`: command entry and dispatch
- `src/cli/`: argument definitions for each subcommand
- `src/handlers/`: subcommand execution logic
- Core simulation flow: `transaction.rs`, `account_loader.rs`, `executor.rs`, `funding.rs`
- Output and parsing: `output/`, `instruction_parsers/`, `log_parser.rs`
- Utilities: `token_account_decoder.rs`, `native_ids.rs`, `config.rs`, `progress.rs`
- Tests: `tests/e2e_simulation.rs`, `tests/fixtures/`

## Common Commands

```bash
# Build
cargo check
cargo build

# Format and lint
cargo fmt --check
cargo clippy -- -D warnings

# Test
cargo test
cargo test --test e2e_simulation -- --ignored --nocapture

# Run
cargo run -- simulate <TX> --rpc-url <RPC_URL>
cargo run -- decode <TX> --rpc-url <RPC_URL>
cargo run -- account <PUBKEY> --rpc-url <RPC_URL>
cargo run -- convert hex text 0x48656c6c6f
cargo run -- pda <PROGRAM_ID> string:hello pubkey:<PUBKEY>
cargo run -- program-elf <PROGRAM_ID> --rpc-url <RPC_URL>
cargo run -- send <SIGNED_TX> --rpc-url <RPC_URL>
cargo run -- fetch-idl <PROGRAM_ID> --rpc-url <RPC_URL>
cargo run -- completions zsh
```

## Coding Rules

- Naming: `snake_case` (fn/var), `PascalCase` (types), `UPPER_SNAKE_CASE` (const)
- Errors: use `anyhow::Result<T>` and add `.context(...)` on fallible boundaries
- Keep modules single-purpose; prefer `pub(crate)` for internal APIs
- User-facing error messages should be English

## Commit and Quality Gate

- Use Conventional Commits
- Before commit, run:
  - `cargo fmt --check`
  - `cargo clippy -- -D warnings`
  - `cargo test`

## Development Checklist

1. Edit only relevant modules
2. Run `cargo check` early
3. Run tests for affected areas
4. Validate CLI behavior manually when command UX changes
5. Update `README.md` for user-facing changes
