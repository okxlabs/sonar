use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;
use std::thread::{self, JoinHandle};

use assert_cmd::cargo::cargo_bin_cmd;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use flate2::{Compression, write::ZlibEncoder};
use serde_json::{Value, json};
use solana_epoch_schedule::EpochSchedule;
use solana_hash::Hash;
use solana_message::{Message, VersionedMessage};
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_system_interface::instruction as system_instruction;
use solana_transaction::versioned::VersionedTransaction;

const SLOT: &str = "100";
const SYSTEM: &str = "11111111111111111111111111111111";
const LOADER: &str = "BPFLoaderUpgradeab1e11111111111111111111111";

fn mock_rpc(responses: Vec<Value>) -> (String, JoinHandle<Vec<Value>>) {
    let count = responses.len();
    let responses = Mutex::new(VecDeque::from(responses));
    mock_rpc_with_handler(count, move |_| responses.lock().unwrap().pop_front().unwrap())
}

fn mock_rpc_with_handler(
    count: usize,
    respond: impl Fn(&Value) -> Value + Send + Sync + 'static,
) -> (String, JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let requests = Mutex::new(Vec::new());
        thread::scope(|scope| {
            for _ in 0..count {
                let (mut stream, _) = listener.accept().unwrap();
                let requests = &requests;
                let respond = &respond;
                scope.spawn(move || {
                    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                    let mut reader = BufReader::new(&stream);
                    let mut length = 0;
                    loop {
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        if line == "\r\n" { break; }
                        if let Some((name, value)) = line.split_once(':') {
                            if name.eq_ignore_ascii_case("content-length") {
                                length = value.trim().parse().unwrap();
                            }
                        }
                    }
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).unwrap();
                    let request: Value = serde_json::from_slice(&body).unwrap();
                    requests.lock().unwrap().push(request.clone());
                    let body = respond(&request).to_string();
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
                });
            }
        });
        requests.into_inner().unwrap()
    });
    (url, handle)
}

fn result(value: Value) -> Value {
    json!({"jsonrpc":"2.0", "id":1, "result":{"value":value}})
}

fn account(owner: &str, data: &[u8], executable: bool) -> Value {
    json!({"lamports":10000000, "data":[STANDARD.encode(data),"base64"],
        "owner":owner,"executable":executable,"rentEpoch":0})
}

fn assert_historical(request: &Value, address: &str) {
    assert_eq!(request["method"], "getHistoricalAccountInfo");
    assert_eq!(request["params"], json!([address,{"slot":100},{"encoding":"base64"}]));
}

#[test]
fn account_reads_historical_data_and_keeps_missing_as_missing() {
    for value in [account(SYSTEM, &[], false), Value::Null] {
        let missing = value.is_null();
        let (url, server) = mock_rpc(vec![result(value)]);
        let mut cmd = cargo_bin_cmd!("sonar");
        cmd.args(["account", SYSTEM, "--at-slot", SLOT, "--rpc-url", &url, "--raw"]);
        let output = cmd.output().unwrap();
        assert_eq!(
            output.status.success(),
            !missing,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if !missing {
            let data: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(data["lamports"], 10000000);
        }
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert_historical(&requests[0], SYSTEM);
    }
}

#[test]
fn account_metadata_and_on_chain_idl_use_the_historical_slot() {
    let mint = Pubkey::new_unique().to_string();
    let token_program = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
    let mut mint_data = vec![0; 82];
    mint_data[36..44].copy_from_slice(&1000u64.to_le_bytes());
    mint_data[44] = 6;
    mint_data[45] = 1;
    // A missing historical metadata account must not be replaced with current metadata.
    let (url, server) =
        mock_rpc(vec![result(account(token_program, &mint_data, false)), result(Value::Null)]);
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.args(["account", &mint, "--at-slot", SLOT, "--rpc-url", &url, "--json"]);
    cmd.assert().success();
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert_historical(&requests[0], &mint);
    assert_eq!(requests[1]["method"], "getHistoricalAccountInfo");
    assert_eq!(requests[1]["params"][1], json!({"slot":100}));

    let owner = Pubkey::new_unique().to_string();
    let (url, server) =
        mock_rpc(vec![result(account(&owner, &[0; 8], false)), result(Value::Null)]);
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.args(["account", &mint, "--at-slot", SLOT, "--rpc-url", &url, "--json", "--idl-dir"])
        .arg(dir.path());
    cmd.assert().success();
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1]["method"], "getHistoricalAccountInfo");
    assert_eq!(requests[1]["params"][1], json!({"slot":100}));
}

#[test]
fn account_rejects_historical_file_and_stdin_inputs() {
    for input in ["-", "not-a-pubkey.json"] {
        let mut cmd = cargo_bin_cmd!("sonar");
        cmd.args(["account", input, "--at-slot", SLOT]);
        let output = cmd.output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("requires an account pubkey"));
    }
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.args(["account", "--at-slot", SLOT]);
    cmd.assert().failure();
}

#[test]
fn program_elf_reads_program_and_programdata_at_the_same_slot() {
    let program = Pubkey::new_unique();
    let programdata = Pubkey::new_unique();
    let mut program_bytes = 2u32.to_le_bytes().to_vec();
    program_bytes.extend_from_slice(programdata.as_ref());
    let elf = b"\x7fELFhistorical-test";
    let mut data = vec![0; 45];
    data[..4].copy_from_slice(&3u32.to_le_bytes());
    data.extend_from_slice(elf);
    let (url, server) = mock_rpc(vec![
        result(account(LOADER, &program_bytes, true)),
        result(account(LOADER, &data, false)),
    ]);
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.args([
        "program-elf",
        &program.to_string(),
        "--at-slot",
        SLOT,
        "--rpc-url",
        &url,
        "-o",
        "-",
    ]);
    let output = cmd.output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout, elf);
    let requests = server.join().unwrap();
    assert_historical(&requests[0], &program.to_string());
    assert_historical(&requests[1], &programdata.to_string());
}

#[test]
fn idl_fetch_reads_historical_idl_and_keeps_not_found() {
    let program = Pubkey::new_unique().to_string();
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    let idl = json!({"name":"historical_idl","version":"0.1.0","instructions":[]});
    encoder.write_all(idl.to_string().as_bytes()).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut data = vec![0; 40];
    data.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
    data.extend_from_slice(&compressed);
    for value in [account(&program, &data, false), Value::Null] {
        let missing = value.is_null();
        let dir = tempfile::tempdir().unwrap();
        let (url, server) = mock_rpc(vec![result(value)]);
        let mut cmd = cargo_bin_cmd!("sonar");
        cmd.args(["idl", "fetch", &program, "--at-slot", SLOT, "--rpc-url", &url, "--json", "-o"])
            .arg(dir.path());
        let output = cmd.output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let output: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output[0]["status"], if missing { "not_found" } else { "ok" });
        let path = dir.path().join(format!("{program}.json"));
        assert_eq!(path.exists(), !missing);
        if !missing {
            let stored: Value =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            assert_eq!(stored, idl);
        }
        let requests = server.join().unwrap();
        let program_key: Pubkey = program.parse().unwrap();
        let (base, _) = Pubkey::find_program_address(&[], &program_key);
        let idl_address = Pubkey::create_with_seed(&base, "anchor:idl", &program_key).unwrap();
        assert_historical(&requests[0], &idl_address.to_string());
    }
}

#[test]
fn decode_loads_historical_accounts_without_explicit_no_cache() {
    let payer = Pubkey::new_unique();
    let recipient = Pubkey::new_unique();
    let ix = system_instruction::transfer(&payer, &recipient, 100);
    let message = Message::new_with_blockhash(&[ix], Some(&payer), &Hash::new_unique());
    let transaction = VersionedTransaction {
        signatures: vec![Signature::default()],
        message: VersionedMessage::Legacy(message),
    };
    let raw = STANDARD.encode(wincode::serialize(&transaction).unwrap());
    let (url, server) =
        mock_rpc_with_handler(5, |request| match request["method"].as_str().unwrap() {
            "getHistoricalAccountInfo" => result(account(SYSTEM, &[], false)),
            "getEpochSchedule" => json!({"jsonrpc":"2.0","id":1,"result":EpochSchedule::default()}),
            "getBlockTime" => json!({"jsonrpc":"2.0","id":1,"result":1700000000}),
            method => panic!("unexpected method {method}"),
        });
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.env("HOME", dir.path()).env_remove("SONAR_IDL_DIR").args([
        "decode",
        &raw,
        "--at-slot",
        SLOT,
        "--rpc-url",
        &url,
        "--no-idl-fetch",
        "--json",
    ]);
    let output = cmd.output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let decoded: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(decoded["instructions"][0]["parsed"]["name"], "Transfer");
    let requests = server.join().unwrap();
    for key in [payer, recipient] {
        let request = requests.iter().find(|r| r["params"][0] == key.to_string()).unwrap();
        assert_historical(request, &key.to_string());
    }
    assert!(requests.iter().any(|r| r["method"] == "getEpochSchedule"));
}

#[test]
fn unsupported_historical_rpc_is_clear_without_nested_loading_errors() {
    let expected = "RPC endpoint does not support --at-slot. Use a Helius RPC endpoint with Historical Accounts support.";
    for command in [
        vec!["account", SYSTEM, "--raw"],
        vec!["program-elf", SYSTEM, "-o", "-"],
        vec!["idl", "fetch", SYSTEM, "--json"],
    ] {
        let is_idl = command[0] == "idl";
        let dir = tempfile::tempdir().unwrap();
        let (url, server) = mock_rpc(vec![json!({
            "jsonrpc": "2.0", "id": 1,
            "error": {"code": -32601, "message": "Method not found"}
        })]);
        let mut cmd = cargo_bin_cmd!("sonar");
        cmd.current_dir(dir.path()).args(command).args(["--at-slot", SLOT, "--rpc-url", &url]);
        let output = cmd.output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected), "{stderr}");
        assert!(!stderr.contains("RPC error:"), "{stderr}");
        assert!(!stderr.contains("Method not found"), "{stderr}");
        assert!(!stderr.contains("Failed to fetch"), "{stderr}");
        if is_idl {
            // Keep idl fetch's existing per-program error and exit-code contract.
            assert!(output.status.success());
            let results: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(results[0]["status"], "error");
        } else {
            assert!(!output.status.success());
            assert_eq!(stderr.trim(), format!("Error: {expected}"));
            assert!(output.stdout.is_empty());
        }
        assert_eq!(server.join().unwrap()[0]["method"], "getHistoricalAccountInfo");
    }
}

#[test]
fn at_slot_is_listed_in_help_and_old_option_is_rejected() {
    for (command, positionals) in [
        (vec!["simulate"], vec!["TX"]),
        (vec!["decode"], vec!["TX"]),
        (vec!["account"], vec![SYSTEM]),
        (vec!["program-elf"], vec![SYSTEM, "-o", "-"]),
        (vec!["idl", "fetch"], vec![SYSTEM]),
    ] {
        let mut help = cargo_bin_cmd!("sonar");
        let output = help.args(&command).arg("--help").output().unwrap();
        assert!(output.status.success());
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("--at-slot <SLOT>"));
        assert!(!text.contains("--historical-slot"));

        let mut old = cargo_bin_cmd!("sonar");
        let output =
            old.args(command).args(positionals).args(["--historical-slot", SLOT]).output().unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("unexpected argument '--historical-slot'")
        );
    }
}

#[test]
fn decode_rejects_account_cache_flags_in_historical_mode() {
    for extra in [vec!["--cache-dir", "/tmp/cache"], vec!["--refresh-cache"]] {
        let mut cmd = cargo_bin_cmd!("sonar");
        cmd.args(["decode", "TX", "--at-slot", SLOT]).args(extra);
        let output = cmd.output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));
    }
}
