# Sonar Development Guide

CLI for local Solana transaction simulation (LiteSVM) plus utility subcommands.

## Project Map

### CLI (`src/`)

- `main.rs` → `cli/` (arg definitions) → `handlers/` (execution logic)
- `converters/`: bytes, integers, sol, text, types
- `core/`: transaction, account_loader, cache, account_file, idl_fetcher
- `output/`: report, text, json, account_text, terminal
- `parsers/`: `instruction/` (anchor_idl, system_program, compute_budget, memo, ata, token2022), log_parser, metaplex_metadata_decoder, token_account_decoder
- `utils/`: config, progress
- `tests/`: e2e_simulation, e2e_cli_output_streams, fixtures/

### Simulation Engine (`crates/sonar-sim/`)

LiteSVM wrapper. Internals are `pub(crate)`, stable facade in `lib.rs`.

- **Pipeline**: `transaction.rs` (parse) → `account_loader.rs` (orchestrate fetch) → `executor.rs` (prepare + run)
- **Fetch layer**: `account_fetcher.rs` (dedup, cache, policy, batched RPC), `account_dependencies.rs` (auto-resolve ProgramData + mints), `rpc_provider.rs` (trait + impls)
- **Execution**: `executor.rs` (`PreparedSimulation` → `SimulationRunner`), `svm_backend.rs` (backend trait)
- **Funding**: `funding/` — sol, token_legacy, token2022, common
- **Support**: `token_decode.rs` (SPL/Token-2022 decoding), `known_programs.rs` (builtin IDs), `balance_changes.rs` (pre/post diffs)
- **Types**: `types/` — accounts, funding, simulation, traits
- **Error**: `error.rs` (`SonarSimError`)

### IDL Library (`crates/sonar-idl/`)

Anchor IDL parsing and instruction decoding: models, parser, discriminator, registry, value

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
