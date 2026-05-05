# Sonar — Solana transaction simulator & utilities

A CLI for local Solana transaction simulation (LiteSVM) plus developer utilities.

- Simulate transactions locally without deploying programs (raw tx, signature, bundle, or raw instructions)
- Override programs/accounts, fund SOL or tokens, patch data
- Store accounts once, replay without hitting RPC
- Serialize/deserialize Borsh with a type descriptor DSL
- Decode, convert, derive PDAs, inspect accounts, pull program ELF, send transactions

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

# Or fetch by signature (auto-detected)
sonar simulate <SIGNATURE> --rpc-url https://api.mainnet-beta.solana.com

# Or synthesize a transaction from one or more instruction specs
sonar simulate --payer <PUBKEY> \
  --ix 'program=MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr data=0x68656c6c6f'

# Decode an account
sonar account <PUBKEY> --rpc-url https://api.mainnet-beta.solana.com
```

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
| `borsh` | Serialize JSON to Borsh bytes or deserialize Borsh bytes to JSON |
| `convert` | Format conversion |
| `pda` | Derive a PDA from seeds |
| `config` | Inspect or update `~/.config/sonar/config.toml` |
| `cache` | List, clean, or inspect cached account data |
| `completions` | Generate shell completion scripts |

## Documentation

- [Simulate & Decode](docs/simulate.md): full usage, overrides, funding, patching, cache
- [Account, PDA, Program ELF, IDL, Send](docs/commands.md): per-command reference
- [Borsh](docs/borsh.md): serialization/deserialization with type descriptor DSL
- [Convert](docs/convert.md): format conversion reference
- [Configuration](docs/configuration.md): config file, cache, completions
- [Output Conventions](docs/output-conventions.md): stdout/stderr contract, permission markers, diagnostics

## License

MIT
