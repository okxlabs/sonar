# Simulate, Decode & Replay

## Simulate

Simulate a Solana transaction locally using LiteSVM (`simulate`, alias: `sim`).
`sonar simulate --help` groups options into `Input & RPC`, `State Preparation`,
`Simulation Controls`, and `Output & Debug`.

```bash
# Simulate a transaction with raw Base58/Base64 data
sonar simulate <BASE58_OR_BASE64_STRING> --rpc-url https://api.mainnet-beta.solana.com

# Simulate using transaction signature (auto-detected)
sonar simulate 2gTzNX3zLNhhmJaY44LycEgF8UMadrKeDLHz8rgcQVbXWVU4bs8fLBzWKhvAqKBeo2ttqyXsCeqUW47dfW6775Wu \
  --rpc-url https://api.mainnet-beta.solana.com

# Bundle simulation (multiple transactions)
sonar simulate <TX1> <TX2> <TX3> --rpc-url https://api.mainnet-beta.solana.com

# Instruction input mode (one synthesized transaction)
sonar simulate --payer <PAYER_PUBKEY> \
  --ix 'program=MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr data=0x68656c6c6f'

# Read transaction from stdin (omit TX to read from pipe)
cat ./transaction.txt | sonar simulate --rpc-url <RPC_URL>

# Show balance changes and instruction details
sonar simulate <TX> --rpc-url <RPC_URL> -b -d

# JSON output
sonar simulate <TX> --rpc-url <RPC_URL> --json
```

### Instruction Input

Use `--ix` to simulate one or more raw instructions without first building a
signed transaction. Sonar creates one unsigned legacy transaction, using
`--payer` as the fee payer, and then runs the normal simulation pipeline.
Use one instruction input format per command. Mixing `--ix` and `--ix-json`
is rejected because cross-format ordering is ambiguous after CLI parsing.

```bash
# Single instruction. `data` is hex and may start with 0x.
sonar simulate --payer <PAYER_PUBKEY> \
  --ix 'program=<PROGRAM_ID> accounts=<ACCOUNT>:sw data=0x01020304'

# Multiple instructions in one atomic transaction.
sonar simulate --payer <PAYER_PUBKEY> \
  --ix 'program=<PROGRAM_A> data=0x01' \
  --ix 'program=<PROGRAM_B> accounts=<ACCOUNT>:w data=0x02'

# Inline JSON when structured input is easier to generate.
sonar simulate --payer <PAYER_PUBKEY> \
  --ix-json '{"program":"<PROGRAM_ID>","accounts":[{"pubkey":"<ACCOUNT>","is_signer":true,"is_writable":true}],"data":"0x01020304"}'

# Read JSON from a file (curl-style `@` prefix). `@/dev/stdin` works for piping.
sonar simulate --payer <PAYER_PUBKEY> --ix-json @instructions.json
```

Inline `--ix` fields:

- `program` (or `program_id`): program pubkey. Required.
- `accounts`: optional comma-separated account metas.
  Account flags are `s` (signer) and `w` (writable).
  Omit the `:flags` suffix for a read-only non-signer.
- `data`: optional instruction data. Hex only, with optional `0x`/`0X`
  prefix. Empty data via omitting the field, `""`, or `"0x"`.

Instruction JSON fields for `--ix-json` (inline or `@<path>`):

- `program` (or `program_id`): program pubkey.
- `accounts`: optional ordered account metas. Each account requires
  `pubkey`, `is_signer`, and `is_writable` — same shape as Solana's
  `AccountMeta` so an existing meta object copies over verbatim.
- `data`: optional instruction data. A `0x`/`0X` prefix selects hex;
  anything else is decoded as base64. Empty data via omitting the field,
  `""`, or `"0x"`.

### Program & Account Override

Override on-chain programs or accounts with local files for testing:

```bash
# Override a program with a local .so file
sonar simulate <TX> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --override TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA=./custom_token.so

# Override an account with a local .json file
sonar simulate <TX> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --override <PUBKEY>=./account.json
```

### Account Funding

Fund system accounts with SOL or token accounts for testing:

```bash
# Fund a single account with SOL
sonar simulate <TX> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --fund-sol DPLezAkFZ5sFaBXMWt3J2StQwYtcqecUipWSP7YfrLth=10.5sol

# Fund multiple accounts
sonar simulate <TX> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --fund-sol <PUBKEY1>=100.0sol \
  --fund-sol <PUBKEY2>=2.75sol

# Fund a token account (raw amount)
sonar simulate <TX> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --fund-token <TOKEN_ACCOUNT>=1000000

# Fund a token account with explicit mint (mint auto-detected if account exists on-chain)
sonar simulate <TX> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --fund-token <ACCOUNT>:<MINT>=1000000

# Fund a new token account with explicit mint and owner
# Owner is required when the token account does not already exist on-chain
sonar simulate <TX> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --fund-token <ACCOUNT>:<MINT>:<OWNER>=1000000

# Fund using decimal amount (uses mint decimals, e.g. 1.5 USDC = 1500000 raw units)
sonar simulate <TX> \
  --rpc-url https://api.mainnet-beta.solana.com \
  --fund-token <TOKEN_ACCOUNT>=1.5
```

### Patching & State Manipulation

```bash
# Patch an account inside instruction 2, account 3
# Format: <IX>.<ACCOUNT>=<NEW_PUBKEY>[:<w|r>] with 1-based indices
# :w is the default; use :r to force read-only
sonar simulate <TX> --rpc-url <RPC_URL> \
  --patch-ix-account 2.3=<NEW_PUBKEY>:r

# Insert an account at a specific position within instruction 1's account list
# Format: <IX>.<POSITION>=<PUBKEY>[:<w|r>] with 1-based indices
# Existing accounts at and after POSITION shift right by one.
# POSITION may equal current_count + 1 to insert at the end (push semantics).
sonar simulate <TX> --rpc-url <RPC_URL> \
  --insert-ix-account 1.3=<PUBKEY>:r

# Remove an account at a specific position from instruction 1's account list
# Format: <IX>.<POSITION> with 1-based indices
# Subsequent accounts shift left by one. The static account_keys table is left
# intact (unreferenced keys remain, mirroring --patch-ix-account behavior).
sonar simulate <TX> --rpc-url <RPC_URL> \
  --remove-ix-account 1.4 \
  --remove-ix-account 1.2

# Ordering note for instruction-account ops:
# Ops apply in flag order (all patches → all inserts → all removes); within each
# flag, CLI argument order is preserved. Positions are interpreted at apply time
# (not against the original list). To express positions relative to the
# pre-mutation list, list ops in descending position order — e.g. above we
# remove position 4 before position 2 so both refer to the original numbering.

# Patch instruction data before simulation
# Format: <IX>=<OFFSET>:<HEX_DATA> with a 1-based instruction index
# HEX_DATA may optionally start with 0x
sonar simulate <TX> --rpc-url <RPC_URL> \
  --patch-ix-data 1=8:0x01020304

# Patch account data before simulation
sonar simulate <TX> --rpc-url <RPC_URL> \
  --patch-account-data <PUBKEY>=<OFFSET>:<HEX_DATA>

# Close an account so it does not exist during simulation
sonar simulate <TX> --rpc-url <RPC_URL> \
  --close-account <PUBKEY>
```

### Cache & Offline Replay

```bash
# Cache accounts for offline replay
sonar simulate <TX> --rpc-url <RPC_URL> --cache

# Replay from cache (no network; uses ~/.sonar/cache/ when cache is complete)
sonar simulate <TX> --cache

# Force refresh cache
sonar simulate <TX> --rpc-url <RPC_URL> --cache --refresh-cache
```

### Simulation Controls

```bash
# Override clock timestamp and slot (Unix or RFC3339)
sonar simulate <TX> --rpc-url <RPC_URL> \
  --timestamp 1700000000 --slot 250000000
sonar simulate <TX> --rpc-url <RPC_URL> \
  --timestamp 2024-01-01T00:00:00Z --slot 250000000

# Verify transaction signatures during simulation
sonar simulate <TX> --rpc-url <RPC_URL> --check-sig

# Load Anchor IDL files from a custom directory
sonar simulate <TX> --rpc-url <RPC_URL> --idl-dir /path/to/idl/files/
```

### Output & Debug

```bash
# Always print raw instruction data, even when parser succeeds
sonar simulate <TX> --rpc-url <RPC_URL> --raw-ix-data

# Print raw logs and full instruction details
sonar simulate <TX> --rpc-url <RPC_URL> --raw-log --show-ix-detail
```

## Decode

Decode and display a raw transaction without simulation (`decode`, alias: `dec`):

```bash
sonar decode <TX> --rpc-url https://api.mainnet-beta.solana.com
sonar decode <TX> --rpc-url <RPC_URL> --json

# Bundle decode (multiple TXs): --json outputs a single JSON array [{...}, {...}]
# Parseable by jq: sonar decode <TX1> <TX2> --json --rpc-url <RPC_URL> | jq .
sonar decode <TX1> <TX2> --rpc-url <RPC_URL> --json

# Read transaction from stdin (omit TX to read from pipe)
cat ./transaction.txt | sonar decode --rpc-url <RPC_URL>

# Always print raw instruction data, even when parser succeeds
sonar decode <TX> --rpc-url <RPC_URL> --raw-ix-data
```

### Decode Cache Controls

```bash
# decode also uses cache by default (signature/raw-tx resolution + account loading)
sonar decode <TX_OR_SIGNATURE> --rpc-url <RPC_URL>

# --no-cache disables account cache but still allows cached raw-tx reuse for signatures
sonar decode <TX_OR_SIGNATURE> --rpc-url <RPC_URL> --no-cache

# --refresh-cache bypasses both raw-tx/account cache and forces RPC
sonar decode <TX_OR_SIGNATURE> --rpc-url <RPC_URL> --refresh-cache

sonar decode <TX_OR_SIGNATURE> --rpc-url <RPC_URL> --cache-dir /path/to/cache
```

## Replay

Replay a confirmed transaction. Fetches it by signature from RPC and reconstructs the execution trace from on-chain metadata (no local simulation).

```bash
# Replay a confirmed transaction
sonar replay <SIGNATURE> --rpc-url https://api.mainnet-beta.solana.com

# Show balance changes and instruction details
sonar replay <SIGNATURE> --rpc-url <RPC_URL> -b -d

# JSON output
sonar replay <SIGNATURE> --rpc-url <RPC_URL> --json

# Print raw logs instead of structured output
sonar replay <SIGNATURE> --rpc-url <RPC_URL> --raw-log

# Always print raw instruction data, even when parser succeeds
sonar replay <SIGNATURE> --rpc-url <RPC_URL> --raw-ix-data

# Load Anchor IDL files from a custom directory
sonar replay <SIGNATURE> --rpc-url <RPC_URL> --idl-dir /path/to/idls
```

## Cache Management

Manage cached account data for offline simulation:

```bash
# Cache accounts for offline replay (writes to ~/.sonar/cache/ by default)
sonar simulate <TX> --rpc-url <RPC_URL> --cache

# Replay from cache (no network access when cache is complete)
sonar simulate <TX> --cache

# Force refresh cache
sonar simulate <TX> --rpc-url <RPC_URL> --refresh-cache

# Custom cache directory (implies --cache)
sonar simulate <TX> --rpc-url <RPC_URL> --cache-dir /path/to/cache

# Manage cache
sonar cache list
sonar cache clean --older-than 7d
sonar cache info <KEY>
```

Cache metadata schema (`_meta.json`) uses a `transactions` array:

```json
{
  "type": "single",
  "transactions": [
    { "input": "<original input>", "raw_tx": "<base64 tx>", "resolved_from": "raw_input|cache|rpc" }
  ]
}
```
