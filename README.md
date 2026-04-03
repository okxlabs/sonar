# Sonar — Solana Transaction Simulator & Utilities

A CLI tool for local Solana transaction simulation (LiteSVM) plus common developer utilities.

- **Local simulation** without deploying programs — raw tx, signature, or bundle
- **State manipulation** — override programs/accounts, fund SOL or tokens, patch data
- **Offline cache & replay** — store accounts once, simulate without RPC
- **Borsh & Anchor IDL** — serialize/deserialize with a type descriptor DSL
- **Developer utilities** — decode, convert, PDA, account, program-elf, send, and more

## Installation

```bash
git clone https://github.com/user/sonar.git
cd sonar
cargo build --release
# Binary at target/release/sonar
```

## Quick Start

```bash
# Simulate a transaction locally
sonar simulate <BASE58_OR_BASE64_TX> --rpc-url https://api.mainnet-beta.solana.com

# Or fetch by signature (auto-detected)
sonar simulate <SIGNATURE> --rpc-url https://api.mainnet-beta.solana.com

# Decode an account
sonar account <PUBKEY> --rpc-url https://api.mainnet-beta.solana.com
```

## Command Reference

| Command | Use when |
|---------|----------|
| **`simulate`** | **You want local execution logs, balance changes, and failure reasons** |
| `decode` | You only need transaction structure (instructions/accounts) without execution |
| `account` | You want decoded account metadata/data for a pubkey |
| `program-elf` | You need raw ELF bytes from upgradeable program/buffer accounts |
| `idl` | You want to fetch/sync Anchor IDLs or derive an IDL account address |
| `send` | You want to submit a signed transaction to the network |
| `borsh` | You want to serialize JSON to Borsh bytes or deserialize Borsh bytes to JSON |
| `convert` | You want explicit and deterministic format conversion |
| `pda` | You want to derive a PDA from seeds |
| `config` | You want to inspect or update `~/.config/sonar/config.toml` |
| `cache` | You want to list, clean, or inspect cached account data |
| `completions` | You want to generate shell completion scripts |

## Documentation

- [Simulate & Decode](docs/simulate.md) — full usage, overrides, funding, patching, cache
- [Account, PDA, Program ELF, IDL, Send](docs/commands.md) — per-command reference
- [Borsh](docs/borsh.md) — serialization/deserialization with type descriptor DSL
- [Convert](docs/convert.md) — format conversion reference
- [Configuration](docs/configuration.md) — config file, cache, completions
- [Output Conventions](docs/output-conventions.md) — stdout/stderr contract, permission markers, diagnostics

## License

MIT
