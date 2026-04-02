# Output Conventions

## stdout / stderr Contract

| Stream   | Content                                                                                  |
|----------|------------------------------------------------------------------------------------------|
| **stdout** | Primary command output: simulation report (text or JSON), decoded instructions, account data, PDA results, etc. |
| **stderr** | Diagnostics (warnings, errors), progress indicators, and utility-command informational output (e.g. `cache list`). |

- **stdout**: Machine-consumable primary results (stable fields, success output, confirmation messages).
  - `send` (text mode): signature only. Explorer URL and `--wait` confirmation go to stderr.
  - `send` (JSON mode): `{"signature", "explorer_url"}` object. `--wait` confirmation still goes to stderr.
  - `program-elf -o <file>`: success message (bytes written, path).
  - Other commands: main structured output.
- **stderr**: Warnings, diagnostics, and errors only.

When `--json` is used, **stdout always contains a single valid JSON document** (object or array). Diagnostics remain on stderr so that `jq` pipelines work without filtering. Note: `program-elf` and `completions` do not support `--json`.

## Diagnostic Levels

Sonar routes all diagnostics through the `log` crate (backend: `env_logger`).

| Level    | Default visibility | Examples |
|----------|--------------------|----------|
| `error`  | shown | Fatal errors, IDL fetch failures |
| `warn`   | shown | Unused `--override`/`--fund-*` addresses, offline-mode missing accounts, config-file parse errors |
| `info`   | hidden | IDL sync progress, summary counts |
| `debug`  | hidden | IDL parser loading, cache key derivation |
| `trace`  | hidden | RPC request/response details |

Control via `RUST_LOG`:

```bash
RUST_LOG=info  sonar simulate ...   # show info + warn + error
RUST_LOG=error sonar simulate ...   # suppress warnings
RUST_LOG=debug sonar simulate ...   # verbose developer output
```

## Account Permission Markers

In the text output of `simulate` and `decode`, each account is annotated with a compact
permission marker in the form `[<sig><perm><exe>]`:

| Position | Values | Meaning |
|----------|--------|---------|
| **sig**  | `s` / `-` | **s**igner or not |
| **perm** | `w` / `r` | **w**ritable or **r**ead-only |
| **exe**  | `x` / `-` | e**x**ecutable (program) or not |

Examples: `[sw-]` = signer, writable; `[-r-]` = non-signer, read-only.

The `[n]` label next to a marker refers to the account's index in the transaction's
account list. Static accounts occupy the lower indices and lookup-table accounts follow;
the legend at the bottom of the Instruction Details section shows the exact ranges.

## Color and Emoji

- Color output is **automatically disabled** when stdout is not a TTY or `NO_COLOR` is set (see [no-color.org](https://no-color.org)).
- No emoji is used in machine-readable output paths.
