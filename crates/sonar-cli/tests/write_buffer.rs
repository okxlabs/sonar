//! End-to-end tests for `sonar write-buffer` against a mock JSON-RPC node.
//!
//! The node is backed by a real LiteSVM instance, so every transaction the
//! command sends is strictly parsed as v1, signature-checked against the v1
//! payload, and executed by the actual BPF Loader Upgradeable. The resulting
//! buffer account is then compared byte-for-byte with the program file.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use assert_cmd::cargo::cargo_bin_cmd;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use litesvm::LiteSVM;
use serde_json::{Value, json};
use solana_account::Account;
use solana_keypair::{Keypair, write_keypair_file};
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use tempfile::TempDir;

const BUFFER_METADATA_SIZE: usize = 37;
const V1_MAX_TRANSACTION_SIZE: usize = 4096;
const LEGACY_MAX_TRANSACTION_SIZE: usize = 1232;
/// Program bytes per `Write`: the loader's 1232-byte instruction data limit
/// minus the 16-byte `Write` header.
const CHUNK_SIZE: usize = 1216;

struct Cluster {
    svm: LiteSVM,
    statuses: HashMap<String, Value>,
    /// Wire bytes of every transaction the command submitted.
    sent: Vec<Vec<u8>>,
    /// The `sendTransaction` options object of every submission, in order.
    send_options: Vec<Value>,
}

impl Cluster {
    fn new() -> Arc<Mutex<Self>> {
        // Signatures are verified by the handler below against the v1 payload.
        let svm = LiteSVM::new().with_sigverify(false);
        Arc::new(Mutex::new(Self {
            svm,
            statuses: HashMap::new(),
            sent: Vec::new(),
            send_options: Vec::new(),
        }))
    }

    /// Answer one JSON-RPC call: `Ok` is the `result`, `Err` the `error` object.
    fn handle(&mut self, method: &str, params: &Value) -> Result<Value, Value> {
        let context = json!({"slot": 1});
        Ok(match method {
            "getAccountInfo" => {
                let value = self.svm.get_account(&pubkey(&params[0])).map(|a| account_json(&a));
                json!({"context": context, "value": value})
            }
            "getBalance" => {
                let value = self.svm.get_balance(&pubkey(&params[0])).unwrap_or(0);
                json!({"context": context, "value": value})
            }
            "getMinimumBalanceForRentExemption" => {
                json!(
                    self.svm
                        .minimum_balance_for_rent_exemption(params[0].as_u64().unwrap() as usize)
                )
            }
            "getLatestBlockhash" => json!({"context": context, "value": {
                "blockhash": self.svm.latest_blockhash().to_string(),
                "lastValidBlockHeight": 1_000,
            }}),
            "getBlockHeight" => json!(0),
            "sendTransaction" => {
                let options = params[1].clone();
                self.send_options.push(options.clone());
                // Blockhashes are handed out at `confirmed`. Like a real node, a
                // preflight against the (default) finalized bank does not know
                // such a blockhash yet.
                let preflight = options["skipPreflight"] != json!(true);
                let preflight_commitment =
                    options["preflightCommitment"].as_str().unwrap_or("finalized");
                if preflight && preflight_commitment == "finalized" {
                    return Err(json!({"code": -32002,
                        "message": "Transaction simulation failed: Blockhash not found"}));
                }
                let wire = STANDARD.decode(params[0].as_str().unwrap()).unwrap();
                self.sent.push(wire.clone());
                let parsed = sonar_sim::V1Transaction::parse(&wire).expect("sent a valid v1 tx");
                parsed.verify_signatures().expect("v1 signatures verify");
                let tx = parsed.to_versioned_transaction();
                let signature = tx.signatures[0].to_string();
                let status = match self.svm.send_transaction(tx) {
                    Ok(_) => json!({"slot": 1, "confirmations": null, "status": {"Ok": null},
                        "err": null, "confirmationStatus": "finalized"}),
                    Err(failed) => {
                        eprintln!("transaction failed: {:?}\n{:#?}", failed.err, failed.meta.logs);
                        json!({"slot": 1, "confirmations": null,
                            "status": {"Err": "AccountInUse"}, "err": "AccountInUse",
                            "confirmationStatus": "finalized"})
                    }
                };
                // A resend of an already processed transaction keeps its first status.
                self.statuses.entry(signature.clone()).or_insert(status);
                json!(signature)
            }
            "getSignatureStatuses" => {
                let value: Vec<Value> = params[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|sig| {
                        self.statuses.get(sig.as_str().unwrap()).cloned().unwrap_or(Value::Null)
                    })
                    .collect();
                json!({"context": context, "value": value})
            }
            other => panic!("unexpected RPC method {other}"),
        })
    }
}

fn pubkey(value: &Value) -> Pubkey {
    value.as_str().unwrap().parse().unwrap()
}

fn account_json(account: &Account) -> Value {
    json!({"lamports": account.lamports, "data": [STANDARD.encode(&account.data), "base64"],
        "owner": account.owner.to_string(), "executable": account.executable,
        "rentEpoch": 0, "space": account.data.len()})
}

/// Serve JSON-RPC requests from `cluster` until the test process exits.
fn serve(cluster: Arc<Mutex<Cluster>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let cluster = Arc::clone(&cluster);
            thread::spawn(move || respond(stream, &cluster));
        }
    });
    url
}

fn respond(mut stream: TcpStream, cluster: &Mutex<Cluster>) {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let mut reader = BufReader::new(&stream);
    let mut length = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap();
            }
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let request: Value = serde_json::from_slice(&body).unwrap();
    let method = request["method"].as_str().unwrap().to_string();
    let body = match cluster.lock().unwrap().handle(&method, &request["params"]) {
        Ok(result) => json!({"jsonrpc": "2.0", "id": request["id"], "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": request["id"], "error": error}),
    }
    .to_string();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .unwrap();
}

struct Fixture {
    dir: TempDir,
    cluster: Arc<Mutex<Cluster>>,
    url: String,
    payer: Keypair,
    program: Vec<u8>,
}

impl Fixture {
    fn new(program_len: usize) -> Self {
        let cluster = Cluster::new();
        let payer = Keypair::new();
        cluster.lock().unwrap().svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
        let url = serve(Arc::clone(&cluster));
        let dir = tempfile::tempdir().unwrap();
        write_keypair_file(&payer, dir.path().join("payer.json")).unwrap();

        let mut program = b"\x7fELF".to_vec();
        program.extend((0..program_len - 4).map(|i| (i * 31 % 251) as u8));
        std::fs::write(dir.path().join("program.so"), &program).unwrap();
        Self { dir, cluster, url, payer, program }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn keypair_file(&self, name: &str, keypair: &Keypair) -> PathBuf {
        let path = self.path(name);
        write_keypair_file(keypair, &path).unwrap();
        path
    }

    fn run(&self, extra: &[&str]) -> std::process::Output {
        let mut cmd = cargo_bin_cmd!("sonar");
        cmd.env("HOME", self.dir.path())
            .env_remove("RPC_URL")
            .env_remove("SONAR_KEYPAIR")
            .arg("write-buffer")
            .arg(self.path("program.so"))
            .args(["--rpc-url", &self.url, "--keypair"])
            .arg(self.path("payer.json"))
            .args(extra);
        cmd.output().unwrap()
    }

    fn buffer(&self, address: &Pubkey) -> Account {
        self.cluster.lock().unwrap().svm.get_account(address).expect("buffer exists")
    }

    fn sent(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.cluster.lock().unwrap().sent)
    }

    fn send_options(&self) -> Vec<Value> {
        std::mem::take(&mut self.cluster.lock().unwrap().send_options)
    }
}

fn json_output(output: &std::process::Output) -> Value {
    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).expect("stdout is JSON")
}

fn buffer_authority(account: &Account) -> Option<Pubkey> {
    match wincode::deserialize::<UpgradeableLoaderState>(&account.data).unwrap() {
        UpgradeableLoaderState::Buffer { authority_address } => {
            authority_address.map(|key| Pubkey::new_from_array(key.to_bytes()))
        }
        other => panic!("not a buffer: {other:?}"),
    }
}

fn assert_all_v1(sent: &[Vec<u8>]) {
    for wire in sent {
        assert_eq!(wire[0], 0x81, "every transaction uses the v1 format");
        assert!(wire.len() <= V1_MAX_TRANSACTION_SIZE);
    }
}

#[test]
fn writes_the_whole_program_into_a_new_buffer_with_v1_transactions() {
    let fixture = Fixture::new(10_000);
    let buffer = Keypair::new();
    let buffer_file = fixture.keypair_file("buffer.json", &buffer);

    let output = fixture.run(&["--buffer", buffer_file.to_str().unwrap(), "--json"]);
    let report = json_output(&output);

    assert_eq!(report["buffer"], buffer.pubkey().to_string());
    assert_eq!(report["buffer_authority"], fixture.payer.pubkey().to_string());
    assert_eq!(report["created"], true);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.lines().next().unwrap(), format!("Buffer: {}", buffer.pubkey()));
    assert!(stderr.contains("Fetching buffer account"), "{stderr}");
    assert!(stderr.contains("Creating buffer: sending 1 transaction(s)"), "{stderr}");
    assert!(stderr.contains("Writing buffer: sending 3 transaction(s)"), "{stderr}");
    for sent in 1..=3 {
        assert!(
            stderr.contains(&format!("Writing buffer: submitted {sent}/3 transaction(s)")),
            "{stderr}"
        );
    }
    assert!(stderr.contains("Writing buffer: 3/3 transactions confirmed (100%)"), "{stderr}");
    assert!(!stderr.contains('\u{1b}'), "non-terminal progress must not use ANSI escapes");

    assert!(report["elapsed_secs"].as_f64().unwrap() > 0.0);
    assert_eq!(report["program_len"], 10_000);
    assert_eq!(report["chunk_size"], CHUNK_SIZE);
    assert_eq!(report["write_instructions"], 10_000usize.div_ceil(CHUNK_SIZE));
    // Nine writes travel in three v1 transactions instead of nine legacy ones.
    assert_eq!(report["write_transactions"], 3);

    let account = fixture.buffer(&buffer.pubkey());
    assert_eq!(account.owner, solana_sdk_ids::bpf_loader_upgradeable::id());
    assert_eq!(buffer_authority(&account), Some(fixture.payer.pubkey()));
    assert_eq!(&account.data[BUFFER_METADATA_SIZE..], fixture.program.as_slice());

    let sent = fixture.sent();
    assert_eq!(sent.len(), 1 + report["write_transactions"].as_u64().unwrap() as usize);
    assert_all_v1(&sent);
    // The create transaction is confirmed before any write is sent.
    assert_eq!(sonar_sim::V1Transaction::parse(&sent[0]).unwrap().instructions.len(), 2);
    // Writes are sent concurrently, so their order is not fixed.
    let writes_per_tx: Vec<usize> = sent[1..]
        .iter()
        .map(|wire| sonar_sim::V1Transaction::parse(wire).unwrap().instructions.len())
        .collect();
    assert_eq!(writes_per_tx, vec![3, 3, 3]);
    // Two transactions carry three full chunks; the third carries the 272-byte tail.
    let full = sent[1..].iter().filter(|wire| wire.len() > 3 * CHUNK_SIZE).count();
    assert_eq!(full, 2);
    let largest = sent.iter().map(Vec::len).max().unwrap();
    assert!(largest > 2 * LEGACY_MAX_TRANSACTION_SIZE, "far beyond the legacy limit: {largest}");
}

#[test]
fn preflight_runs_at_the_commitment_the_blockhash_was_fetched_at() {
    let fixture = Fixture::new(5_000);
    json_output(&fixture.run(&["--json"]));

    let options = fixture.send_options();
    assert!(options.len() >= 2, "a create and at least one write: {options:?}");
    for sent in &options {
        assert_eq!(sent["encoding"], "base64");
        assert_eq!(sent["preflightCommitment"], "confirmed", "{sent}");
    }
    // The create transaction is preflighted; writes skip it.
    assert_eq!(options[0]["skipPreflight"], false);
    assert!(options[1..].iter().all(|sent| sent["skipPreflight"] == true), "{options:?}");
}

#[test]
fn resumes_an_existing_buffer_rewriting_only_changed_chunks() {
    let fixture = Fixture::new(9_000);
    let authority = Keypair::new();
    let authority_file = fixture.keypair_file("authority.json", &authority);

    // The buffer address is printed once on stderr before upload; stdout has elapsed time.
    let output = fixture.run(&["--buffer-authority", authority_file.to_str().unwrap()]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stderr = String::from_utf8(output.stderr).unwrap();
    let address: Pubkey = stderr
        .lines()
        .next()
        .unwrap()
        .strip_prefix("Buffer: ")
        .expect("Buffer: line")
        .parse()
        .unwrap();
    assert_eq!(stderr.lines().filter(|line| line.starts_with("Buffer: ")).count(), 1);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("Buffer:"));
    let mut lines = stdout.lines();
    let elapsed = lines.next().unwrap().strip_prefix("Elapsed: ").expect("Elapsed: line");
    assert!(elapsed.strip_suffix('s').unwrap().parse::<f64>().unwrap() > 0.0);
    assert!(lines.next().is_none());
    assert_eq!(buffer_authority(&fixture.buffer(&address)), Some(authority.pubkey()));
    fixture.sent();

    // Corrupt one byte of the second chunk and resume by address.
    let mut account = fixture.buffer(&address);
    let corrupt_at = BUFFER_METADATA_SIZE + 5_000;
    account.data[corrupt_at] ^= 0xff;
    fixture.cluster.lock().unwrap().svm.set_account(address, account).unwrap();

    let report = json_output(&fixture.run(&[
        "--buffer",
        &address.to_string(),
        "--buffer-authority",
        authority_file.to_str().unwrap(),
        "--json",
    ]));
    assert_eq!(report["created"], false);
    assert_eq!(report["write_instructions"], 1);
    assert_eq!(report["write_transactions"], 1);
    assert_eq!(report["skipped_chunks"], 9_000usize.div_ceil(CHUNK_SIZE) - 1);
    assert_eq!(&fixture.buffer(&address).data[BUFFER_METADATA_SIZE..], fixture.program.as_slice());
    let sent = fixture.sent();
    assert_eq!(sent.len(), 1, "only the changed chunk is rewritten");
    assert_all_v1(&sent);

    // Nothing left to write: no transactions at all.
    let report = json_output(&fixture.run(&[
        "--buffer",
        &address.to_string(),
        "--buffer-authority",
        authority_file.to_str().unwrap(),
        "--json",
    ]));
    assert_eq!(report["write_transactions"], 0);
    assert!(fixture.sent().is_empty());

    // The wrong authority is refused before anything is sent.
    let output = fixture.run(&["--buffer", &address.to_string()]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Buffer authority mismatch"), "{stderr}");
    assert!(fixture.sent().is_empty());
}

#[test]
fn refuses_to_create_a_buffer_from_an_address_or_without_funds() {
    let fixture = Fixture::new(5_000);

    let missing = Pubkey::new_unique().to_string();
    let output = fixture.run(&["--buffer", &missing]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not exist"), "{stderr}");

    let poor = Keypair::new();
    fixture.cluster.lock().unwrap().svm.airdrop(&poor.pubkey(), 1_000_000).unwrap();
    let poor_file = fixture.keypair_file("poor.json", &poor);
    let output = fixture.run(&["--fee-payer", poor_file.to_str().unwrap()]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("insufficient funds"), "{stderr}");
    let address = stderr.lines().next().unwrap().strip_prefix("Buffer: ").unwrap();
    assert!(address.parse::<Pubkey>().is_ok(), "{stderr}");
    assert!(fixture.sent().is_empty());
}

#[test]
fn max_len_reserves_room_and_the_priority_fee_is_charged() {
    let fixture = Fixture::new(4_000);
    let before = fixture.cluster.lock().unwrap().svm.get_balance(&fixture.payer.pubkey()).unwrap();

    let report =
        json_output(&fixture.run(&["--max-len", "20000", "--priority-fee", "1234", "--json"]));
    let address: Pubkey = report["buffer"].as_str().unwrap().parse().unwrap();
    assert_eq!(report["max_len"], 20_000);

    let account = fixture.buffer(&address);
    assert_eq!(account.data.len(), BUFFER_METADATA_SIZE + 20_000);
    assert_eq!(&account.data[BUFFER_METADATA_SIZE..][..4_000], fixture.program.as_slice());

    let sent = fixture.sent();
    assert_all_v1(&sent);
    for wire in &sent {
        let parsed = sonar_sim::V1Transaction::parse(wire).unwrap();
        assert_eq!(parsed.config.priority_fee, Some(1_234));
        assert!(parsed.config.compute_unit_limit.is_some());
        assert!(parsed.config.loaded_accounts_data_size_limit.is_some());
    }

    let after = fixture.cluster.lock().unwrap().svm.get_balance(&fixture.payer.pubkey()).unwrap();
    let rent = account.lamports;
    // Create (payer + buffer signatures) and writes (payer only) all pay the priority fee.
    let fees = sent.iter().map(|wire| {
        let signatures = sonar_sim::V1Transaction::parse(wire).unwrap().signatures.len() as u64;
        signatures * 5_000 + 1_234
    });
    assert_eq!(before - after, rent + fees.sum::<u64>());
}

#[test]
fn rejects_files_that_are_not_elf() {
    let fixture = Fixture::new(100);
    std::fs::write(fixture.path("program.so"), b"plain text").unwrap();
    let output = fixture.run(&[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("is not an ELF file"));
}
