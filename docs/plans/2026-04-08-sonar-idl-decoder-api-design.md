# Sonar IDL Decoder-First API Design

## Goal

Refactor `crates/sonar-idl` into a smaller, decoder-first crate whose supported public API is centered on `IndexedIdl` and parsed output, not on exposing the full Anchor IDL schema model.

## Motivation

Recent cleanup already moved real usage toward:

1. deserialize Anchor IDL JSON
2. normalize and index it
3. decode instruction, account, and CPI event data through `IndexedIdl`

The remaining public surface still exposes a large schema API that callers no longer need:

- `RawAnchorIdl`
- `LegacyIdl`
- `Idl`
- many `Idl*` schema structs
- `IndexedIdl::idl()`
- parsed output that still leaks schema internals through `IdlParsedInstruction.accounts`

That surface makes the crate look like a general-purpose IDL model library when it now behaves more like a decoding engine.

## Desired Public API

Keep these public:

- `IndexedIdl`
- `IdlParsedInstruction`
- `IdlParsedField`
- `sighash`
- `is_cpi_event_data`

Shrink or remove support for these public entry points and model exports:

- `RawAnchorIdl`
- `LegacyIdl`
- `Idl`
- most `Idl*` schema structs
- `IndexedIdl::new(Idl)`
- `IndexedIdl::idl()`

## Construction Model

`IndexedIdl` becomes the only supported top-level decoder type.

Callers should be able to construct it directly from Anchor IDL JSON:

```rust
let indexed: IndexedIdl = serde_json::from_str(idl_json)?;
```

That deserialization path must support both:

- current Anchor IDL JSON
- legacy pre-0.30 Anchor IDL JSON

Legacy conversion, discriminator normalization, and address fallback handling become internal implementation details of `IndexedIdl` construction.

## Parsed Output Model

`IdlParsedInstruction` should stop exposing raw schema account types.

Today it contains:

- `name`
- `fields`
- `accounts: Vec<IdlAccountItem>`

That ties the public parsed output to internal schema definitions. The replacement should use a small decoder-oriented representation instead. Two acceptable shapes:

1. `account_names: Vec<String>`
2. a small public parsed-account enum/struct designed specifically for decode results

The preferred direction is `account_names: Vec<String>` because it matches what the CLI adapter already consumes and keeps the API simple.

## Internal Structure

Keep the current high-level split, but tighten responsibilities:

- `parser/indexed.rs`: public type, deserialization, indexing, decode entry points
- `parser/decode.rs`: low-level binary decoding only
- `models/`: internal schema, current/legacy normalization, serde helpers

The `models` module remains necessary, but it becomes an implementation detail instead of a supported public model layer.

## Refactor Scope

The refactor should include:

- implement `Deserialize` for `IndexedIdl` using internal raw/schema conversion
- remove `IndexedIdl::idl()`
- remove `IndexedIdl::new(Idl)` from the public API
- stop re-exporting schema-heavy model types from `lib.rs`
- replace parsed-instruction account exposure with a smaller decoder-facing type
- update CLI integration to consume only the new surface

The refactor may also include a second internal cleanup pass on `parser/decode.rs` if the API changes leave obvious extraction boundaries.

## Non-Goals

This refactor should not:

- change decoding semantics for supported IDLs
- change ordered JSON preservation behavior
- introduce a new facade or view abstraction just to hide types
- add new dependencies beyond what is required for serde/deserialization support

## Error Handling

The decoder-only API should make failures clearer:

- invalid legacy/current IDL JSON fails during `IndexedIdl` deserialization
- malformed binary data continues to return parse errors or `None` depending on discriminator matching
- callers should no longer need to know about separate normalization or conversion steps

## Testing Strategy

Coverage should move from model-centric construction toward decoder-centric construction:

- `serde_json::from_str::<IndexedIdl>(...)` for current-format IDLs
- `serde_json::from_str::<IndexedIdl>(...)` for legacy IDLs
- regression tests for missing discriminators
- regression tests for legacy account/type merging
- regression tests for legacy alias handling
- regression tests for ordered field preservation
- regression tests for malformed vector and defined-type decode behavior

## Migration Impact

This is an intentional breaking change.

Downstream code will need to stop:

- constructing `IndexedIdl` from public `Idl`
- inspecting normalized `Idl` directly through `idl()`
- depending on public schema structs from `sonar-idl`

Downstream code should instead:

- deserialize `IndexedIdl` directly from JSON
- decode through `IndexedIdl` methods
- consume parsed output types only

## Recommendation

Proceed with the decoder-first cleanup now, while the crate is already in the middle of API-focused refactoring. Waiting will only make downstream expectations harder to change later.
