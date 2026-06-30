//! Offline, network-free end-to-end tests for the `simulate` and `decode`
//! pipelines.
//!
//! Unlike `e2e_simulation` / `e2e_cli_output_streams` (which are `#[ignore]` and
//! require mainnet RPC), these run in CI with no network: a local account-cache
//! directory (`_meta.json` + per-account JSON) puts the CLI into offline mode,
//! so the whole parse → load → (mutate/prepare) → execute → render path is
//! exercised against fixed local state. They guard behavior parity of that
//! pipeline regardless of how the handlers are wired internally.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

/// A deterministic legacy SOL-transfer transaction (1 signature, 3 accounts:
/// fee-payer, recipient, system program), transferring 10_000_000 lamports.
const TRANSFER_TX: &str = "AYXl4tu2q/qsjwA+woUaYKC+uPuAozXJHsgxsZLux/8uXuN2z8P1tLt0wHkQImIfxXBjg3dT8ryk8D5BA6g+/QABAAEDiojj3XQJ8ZX9UtstPLpdcspnCb8dlBIb83SIAbQPb1wCAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAgIAAQwCAAAAgJaYAAAAAAA=";

const PAYER: &str = "AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9";
const RECIPIENT: &str = "8qbHbw2BbbTHBW1sbeqakYXVKRQM8Ne7pLK7m6CVfeR";
const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";

/// Build a cache directory that puts the CLI into offline mode. The presence of
/// `_meta.json` is what flips the loader offline; the per-account JSON files are
/// read by the local-dir source so no RPC is needed.
fn offline_cache_dir() -> TempDir {
    let dir = TempDir::new().expect("create temp cache dir");
    let system_account = |lamports: u64| {
        format!(
            r#"{{"lamports":{lamports},"data":["","base64"],"owner":"{SYSTEM_PROGRAM}","executable":false,"rentEpoch":0}}"#
        )
    };
    std::fs::write(dir.path().join(format!("{PAYER}.json")), system_account(1_000_000_000))
        .unwrap();
    std::fs::write(dir.path().join(format!("{RECIPIENT}.json")), system_account(1)).unwrap();
    // Existence alone enables offline mode; contents are irrelevant for a raw-tx
    // input (the signature-cache path is never taken).
    std::fs::write(dir.path().join("_meta.json"), "{}").unwrap();
    dir
}

fn sonar() -> Command {
    cargo_bin_cmd!("sonar")
}

#[test]
fn simulate_offline_executes_and_reports_success() {
    let dir = offline_cache_dir();
    let assert = sonar()
        .args(["simulate", TRANSFER_TX, "--cache-dir"])
        .arg(dir.path())
        .args(["--rpc-url", "http://localhost:1"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("SUCCESS"), "expected a success banner, got:\n{stdout}");
    assert!(stdout.contains(SYSTEM_PROGRAM), "expected the executed program in output:\n{stdout}");
}

#[test]
fn simulate_offline_json_is_structured_and_successful() {
    let dir = offline_cache_dir();
    let assert = sonar()
        .args(["simulate", TRANSFER_TX, "--cache-dir"])
        .arg(dir.path())
        .args(["--rpc-url", "http://localhost:1", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("simulate --json emits JSON");
    assert!(json.get("transaction").is_some(), "missing transaction section: {json}");
    let simulation = json.get("simulation").expect("missing simulation section");
    assert!(simulation.get("status").is_some(), "missing simulation.status: {simulation}");
    assert!(
        simulation.get("compute_units_consumed").is_some(),
        "missing simulation.compute_units_consumed: {simulation}"
    );
}

#[test]
fn simulate_offline_bundle_executes_all() {
    let dir = offline_cache_dir();
    let assert = sonar()
        .args(["simulate", TRANSFER_TX, TRANSFER_TX, "--cache-dir"])
        .arg(dir.path())
        .args(["--rpc-url", "http://localhost:1"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("Bundle"), "expected a bundle banner, got:\n{stdout}");
    assert!(stdout.contains("2/2"), "expected both bundle txs to run, got:\n{stdout}");
}

#[test]
fn decode_offline_renders_decoded_transfer() {
    let dir = offline_cache_dir();
    let assert = sonar()
        .args(["decode", TRANSFER_TX, "--cache-dir"])
        .arg(dir.path())
        .args(["--rpc-url", "http://localhost:1"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("Decoded Instructions"), "expected decode header:\n{stdout}");
    assert!(stdout.contains("Transfer"), "system transfer should be decoded:\n{stdout}");
    assert!(stdout.contains(PAYER), "payer should appear in account list:\n{stdout}");
    assert!(stdout.contains(RECIPIENT), "recipient should appear in account list:\n{stdout}");
}
