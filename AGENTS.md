# Sonar Development Guide

CLI for local Solana transaction simulation (LiteSVM) plus utility subcommands.

## Project Map

- `src/main.rs`: command entry and dispatch
- `src/cli/`: argument definitions for each subcommand
- `src/handlers/`: subcommand execution logic
- Conversion logic (`src/converters/`): `bytes.rs`, `integers.rs`, `sol.rs`, `text.rs`, `types.rs`
- Core simulation flow (`src/core/`): `transaction.rs`, `account_loader.rs`, `executor.rs`, `balance_changes.rs`, `cache.rs`, `account_file.rs`, `idl_fetcher.rs`, `rpc_provider.rs`, `types.rs`, `funding/` (`sol.rs`, `token_legacy.rs`, `token2022.rs`)
- Output presentation (`src/output/`): `report.rs`, `text.rs`, `json.rs`, `account_text.rs`, `terminal.rs`
- Parsing and Decoders (`src/parsers/`): `instruction/` (`anchor_idl.rs`, `system_program.rs`, `compute_budget_program.rs`, `memo_program.rs`, `associated_token_program.rs`, `token2022_program.rs`, `template.rs`), `log_parser.rs`, `metaplex_metadata_decoder.rs`, `token_account_decoder.rs`
- Utilities (`src/utils/`): `native_ids.rs`, `config.rs`, `progress.rs`
- Tests: `tests/e2e_simulation.rs`, `tests/e2e_cli_output_streams.rs`, `tests/fixtures/`

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
cargo run -- simulate <TX> --rpc-url <RPC_URL> --cache
cargo run -- simulate <TX> --cache
cargo run -- decode <TX> --rpc-url <RPC_URL>
cargo run -- account <PUBKEY> --rpc-url <RPC_URL>
cargo run -- convert hex text 0x48656c6c6f
cargo run -- pda <PROGRAM_ID> string:hello pubkey:<PUBKEY>
cargo run -- program-elf <PROGRAM_ID> --rpc-url <RPC_URL> -o program.so
cargo run -- send <SIGNED_TX> --rpc-url <RPC_URL>
cargo run -- idl fetch <PROGRAM_ID> --rpc-url <RPC_URL>
cargo run -- cache list
cargo run -- cache clean --older-than 7d
cargo run -- cache info <KEY>
cargo run -- config list
cargo run -- config get <KEY>
cargo run -- config set <KEY> <VALUE>
cargo run -- completions zsh
```

## Coding Rules

- Naming: `snake_case` (fn/var), `PascalCase` (types), `UPPER_SNAKE_CASE` (const)
- Imports: keep `use` declarations at module scope (or test module scope), not inside function bodies
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
