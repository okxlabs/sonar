# Sonar IDL Layout Refactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Split the large `sonar-idl` source files into smaller focused modules without changing the public API or parser behavior.

**Architecture:** Keep the crate surface stable by preserving the top-level `models` and `parser` modules, but convert them into directory modules with focused submodules and re-exports. Treat the existing test suite as the safety net: move tests alongside the new layout, then verify the refactor with targeted crate checks.

**Tech Stack:** Rust, `serde`, `serde_json`, `anyhow`, Cargo test/clippy

---

### Task 1: Split `models.rs` into focused submodules

**Files:**
- Create: `crates/sonar-idl/src/models/mod.rs`
- Create: `crates/sonar-idl/src/models/idl.rs`
- Create: `crates/sonar-idl/src/models/raw.rs`
- Create: `crates/sonar-idl/src/models/serde.rs`
- Create: `crates/sonar-idl/src/models/types.rs`
- Create: `crates/sonar-idl/src/models/tests.rs`
- Delete: `crates/sonar-idl/src/models.rs`

**Step 1:** Move the core IDL structs and normalization logic into `idl.rs`.

**Step 2:** Move legacy/current IDL conversion into `raw.rs`.

**Step 3:** Move type definitions into `types.rs` and serde helpers into `serde.rs`.

**Step 4:** Re-export the same public items from `models/mod.rs` and move the old tests into `models/tests.rs`.

**Step 5:** Run `cargo test -p sonar-idl models --lib` if needed, otherwise continue to the parser split and rely on the full crate test run at the end.

### Task 2: Rename the resolved parser module and keep the parser API stable

**Files:**
- Create: `crates/sonar-idl/src/parser/indexed.rs`
- Modify: `crates/sonar-idl/src/parser/mod.rs`
- Delete: `crates/sonar-idl/src/parser/lookup.rs`

**Step 1:** Move `IndexedIdl`, the lookup trait, and discriminator scan helpers into `indexed.rs`.

**Step 2:** Update `parser/mod.rs` to import from `indexed.rs` and keep the same public exports.

**Step 3:** Keep function signatures and visibility unchanged so downstream code does not need any updates.

### Task 3: Split parser tests by concern

**Files:**
- Create: `crates/sonar-idl/src/parser/tests/mod.rs`
- Create: `crates/sonar-idl/src/parser/tests/instruction.rs`
- Create: `crates/sonar-idl/src/parser/tests/account.rs`
- Create: `crates/sonar-idl/src/parser/tests/event.rs`
- Create: `crates/sonar-idl/src/parser/tests/decode.rs`
- Delete: `crates/sonar-idl/src/parser/tests.rs`

**Step 1:** Move the shared `hello_anchor_idl()` fixture into `parser/tests/mod.rs`.

**Step 2:** Move instruction matching and parsed-instruction tests into `instruction.rs`.

**Step 3:** Move account decoding tests into `account.rs` and CPI event tests into `event.rs`.

**Step 4:** Move low-level decode coverage into `decode.rs`.

### Task 4: Verify the refactor end-to-end

**Files:**
- Verify: `crates/sonar-idl/src/**`

**Step 1:** Run `cargo fmt --check`.

**Step 2:** Run `cargo test -p sonar-idl`.

**Step 3:** Run `cargo clippy -p sonar-idl -- -D warnings`.

**Step 4:** Review the final diff and make sure no API or behavior changes slipped into the refactor.
