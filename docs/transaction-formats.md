# Transaction Formats

Sonar parses all three Solana transaction formats and reports which one it found.

| Format | Detected by | Address lookup tables | Compute budget |
|--------|-------------|-----------------------|----------------|
| `legacy` | `short_vec` signature count (`< 0x80`) | no | `ComputeBudget` instructions |
| `v0` | version byte `0x80` | yes | `ComputeBudget` instructions |
| `v1` | version byte `0x81` | no | header config mask ([SIMD-0385](#resources)) |

Detection is byte-based and happens on the decoded input, so Base58 and Base64
inputs behave identically, including when the bytes are fetched from RPC by
signature. `parse` output, the `version` field of the text and
JSON reports (`legacy` / `v0` / `v1`), and `size_bytes` all reflect the format
that arrived on the wire.

## v1 (SIMD-0385)

A v1 transaction replaces compute budget instructions with a header config mask
and writes its signatures as a trailing fixed-length array:

```text
VersionByte (0x81)
LegacyHeader (3 bytes)
TransactionConfigMask (u32 LE)
LifetimeSpecifier (32 bytes)   -- the recent blockhash, renamed
NumInstructions (u8)
NumAddresses (u8)
Addresses ([u8; 32] x NumAddresses)
ConfigValues (one 4-byte LE slot per set bit, in bit order)
InstructionHeaders (program index u8, account count u8, data length u16 LE)
InstructionPayloads (account indices, then data, per instruction)
Signatures ([u8; 64] x num_required_signatures, no length prefix)
```

### What Sonar does with v1

- **Strict parsing.** Every sanitization rule is enforced, including duplicate
  addresses, index ranges (an instruction may not name the fee payer as its
  program), the 4096-byte / 64-address / 64-instruction / 12-signature limits,
  the heap size range and alignment, and the rule that both priority-fee mask
  bits must be set together. A v1 transaction that parses is one the cluster
  would accept.
- **Faithful round-trip.** `sonar send` and the raw-transaction cache emit the
  original v1 bytes, and the executable message serializes back to exactly those
  bytes. The bytes fetched from RPC are forwarded untouched rather than decoded
  and re-encoded, so what Sonar runs and sends is what the cluster accepted.
- **Replay.** `replay` reproduces the on-chain result from `getTransaction`
  metadata (accounts as of the transaction's slot, plus the recorded logs), which
  is the way to see why a historical v1 transaction behaved as it did.
- **Decode and reporting.** `decode` and `simulate` list the v1 message's
  addresses and instructions exactly as for legacy/v0, and additionally show the
  header config (`Transaction Config (v1)` for `decode`, a `v1 config:` line for
  `simulate`, `v1_config` in JSON) including the effective values the SIMD
  defines for absent fields. v1 has no address lookup tables, so that section is
  always empty.
- **Simulation.** The transaction is executed as a native v1 message, so the
  header config is in force: the requested compute-unit limit is the budget, the
  requested heap size is used, the loaded accounts data size limit is enforced,
  and the priority fee is charged to the fee payer. Execution does not go through
  a lowered legacy message, so nothing about the message is approximated.
- **Signature checks.** `--check-sig` verifies v1 signatures against the v1
  signing payload (everything before the signature array) before execution. The
  VM-level check is skipped for v1 because the v1 payload is not the payload the
  VM would verify. The legacy/v0 transactions of a batch that contains v1 are
  verified in the same pass, against the payload the VM uses, so `--check-sig`
  covers every transaction in the batch.
- **Mutations.** Instruction and account patches apply to the v1 message itself.
  Sonar rebuilds the v1 view afterwards, so reported size and config stay
  consistent with what was executed. As for every format, mutating invalidates
  signatures.
- **Building.** `write-buffer` builds, signs, and sends its own v1
  transactions, setting the compute unit limit and loaded accounts data size
  limit explicitly and packing several loader `Write` instructions into each one
  (see [Write Buffer](commands.md#write-buffer)).
- **Bundles.** v1 and legacy/v0 transactions can be mixed in one bundle. When any
  transaction in a batch is v1, the VM cannot verify the v1 signature, so Sonar
  performs every signature check for that batch instead.

### Sanitization rules

A transaction that parses is one the cluster accepts: every rule beyond the wire
layout is either listed in SIMD-0385 or enforced by the reference implementation
(`solana-message`), and none of them is stricter than that implementation.

| Rule | Source |
| --- | --- |
| Version byte is `129` | SIMD §VersionByte |
| `num_readonly_signed_accounts < num_required_signatures` (so at least one signer) | SIMD §LegacyHeader |
| Size ≤ 4096, ≤ 12 signatures, ≤ 64 addresses, ≤ 64 instructions | SIMD §Transaction Constraints |
| `num_addresses ≥ num_required_signatures + num_readonly_unsigned_accounts` | SIMD §NumAddresses |
| No duplicate addresses | SIMD §Addresses |
| Instruction account index < `num_addresses` | SIMD §InstructionPayloads |
| No trailing bytes after the signature array | SIMD §Signatures |
| Both priority-fee bits set, or neither | SIMD §TransactionConfigMask |
| Heap size a 1 KiB multiple in `[32 KiB, 256 KiB]` | SIMD §TransactionConfigMask |
| Unknown config mask bits rejected | SIMD silent; `TransactionConfigMask::has_unknown_bits` |
| Program index in range, and never the fee payer (address 0) | SIMD silent; `v1::Message::validate` |

The two spec-silent rules are the reference implementation's own, so rejecting
them cannot turn a cluster-valid transaction away.

### Configuration defaults

An absent config field means the *minimum* value, which for the loaded accounts
data size limit is zero — deliberately unlike the 64 MiB default a legacy/v0
transaction gets. A v1 transaction that leaves bit 3 unset therefore requests a
zero-byte budget and cannot load any account, so executable v1 transactions
always set it. Sonar reports what the header asks for (`effective_*` in JSON) and
the backend enforces it, so such a transaction fails in simulation exactly as it
would on-chain. `decode` is the way to inspect one.

### Known limitations

- **`ComputeBudget` instructions inside a v1 transaction** are ignored for
  configuration by the format, but they still consume compute units. Sonar's
  simulation applies them the way the VM does, since they are ordinary
  instructions by the time they reach it.
- **Unknown config mask bits are rejected** rather than ignored: each set bit
  consumes a config slot and the mask is part of the signed payload, so a mask
  this build does not understand cannot be interpreted safely. The reference
  implementation rejects them for the same reason.
- **The fee the simulation charges is not part of the report.** It is deducted
  from the fee payer as it would be on-chain (the v1 priority fee included), but
  no report field states it explicitly yet. A v1 transaction's fee can be read
  off the header config: base fee plus the priority-fee field.

## Resources

- [SIMD-0385: Transaction V1 Format](https://github.com/solana-foundation/solana-improvement-documents/blob/main/proposals/0385-transaction-v1.md)
