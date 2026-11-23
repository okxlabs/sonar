use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};
use solana_account::ReadableAccount;
use solana_clock::Clock;
use solana_epoch_schedule::EpochSchedule;
use solana_pubkey::Pubkey;
use solana_slot_hashes::SlotHashes;
use solana_sysvar_id::SysvarId;
use sonar_sim::{RpcAccountProvider, SolanaRpcProvider};

fn mock_rpc(responses: Vec<Value>) -> (String, JoinHandle<Vec<Value>>) {
    let count = responses.len();
    let responses = Mutex::new(VecDeque::from(responses));
    mock_parallel_rpc(count, move |_| responses.lock().unwrap().pop_front().unwrap())
}

fn mock_parallel_rpc(
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
                    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
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
                    write!(stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(), body).unwrap();
                });
            }
        });
        requests.into_inner().unwrap()
    });
    (url, handle)
}

fn account(lamports: u64) -> Value {
    json!({
        "lamports": lamports, "data": ["AQID", "base64"],
        "owner": "11111111111111111111111111111111",
        "executable": false, "rentEpoch": 0,
        "lastUpdatedAt": {"slot": 99, "transactionIndex": 2}
    })
}

fn result(value: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "result": {"context": {"slot": 200}, "value": value}})
}

#[test]
fn historical_reads_preserve_order_missing_accounts_and_slot() {
    let keys = [Pubkey::new_unique(), Pubkey::new_unique(), Pubkey::new_unique()];
    let strings = keys.map(|key| key.to_string());
    let (url, server) = mock_parallel_rpc(3, move |request| {
        let index = strings.iter().position(|key| request["params"][0] == *key).unwrap();
        // Force the first result to finish last.
        if index == 0 {
            thread::sleep(Duration::from_millis(80));
        }
        result(if index == 1 { Value::Null } else { account((index as u64 + 1) * 10) })
    });
    let accounts =
        SolanaRpcProvider::new(url).with_historical_slot(100).get_multiple_accounts(&keys).unwrap();
    assert_eq!(accounts.len(), 3);
    assert_eq!(accounts[0].as_ref().unwrap().lamports(), 10);
    assert_eq!(accounts[0].as_ref().unwrap().data(), &[1, 2, 3]);
    assert!(accounts[1].is_none());
    assert_eq!(accounts[2].as_ref().unwrap().lamports(), 30);
    let requests = server.join().unwrap();
    for key in keys {
        let request = requests.iter().find(|r| r["params"][0] == key.to_string()).unwrap();
        assert_eq!(request["method"], "getHistoricalAccountInfo");
        assert_eq!(
            request["params"],
            json!([key.to_string(), {"slot": 100}, {"encoding": "base64"}])
        );
    }
}

#[test]
fn historical_progress_arrives_while_another_account_is_still_pending() {
    let keys = [Pubkey::new_unique(), Pubkey::new_unique()];
    let slow_key = keys[0].to_string();
    let completed = Mutex::new(Vec::new());
    let signal = Arc::new((Mutex::new(false), Condvar::new()));
    let observed = signal.clone();
    let (url, server) = mock_parallel_rpc(2, move |request| {
        if request["params"][0] == slow_key {
            let (ready, wake) = &*observed;
            let (ready, _) = wake
                .wait_timeout_while(ready.lock().unwrap(), Duration::from_secs(5), |ready| !*ready)
                .unwrap();
            assert!(*ready, "fast account progress must arrive before the slow response");
            result(Value::Null)
        } else {
            result(account(10))
        }
    });
    let accounts = SolanaRpcProvider::new(url)
        .with_historical_slot(100)
        .get_multiple_accounts_with_progress(&keys, &|key| {
            completed.lock().unwrap().push(*key);
            if *key == keys[1] {
                *signal.0.lock().unwrap() = true;
                signal.1.notify_one();
            }
        })
        .unwrap();
    assert!(accounts[0].is_none());
    assert_eq!(*completed.lock().unwrap(), vec![keys[1], keys[0]]);
    assert_eq!(server.join().unwrap().len(), 2);
}

#[test]
fn historical_progress_does_not_count_failed_accounts() {
    let keys = [Pubkey::new_unique(), Pubkey::new_unique()];
    let failed_key = keys[1].to_string();
    let completed = Mutex::new(Vec::new());
    let (url, server) = mock_parallel_rpc(2, move |request| {
        if request["params"][0] == failed_key {
            json!({"jsonrpc":"2.0", "id":1, "error":{"code":-32000, "message":"Access denied"}})
        } else {
            result(account(10))
        }
    });
    assert!(
        SolanaRpcProvider::new(url)
            .with_historical_slot(100)
            .get_multiple_accounts_with_progress(&keys, &|key| completed.lock().unwrap().push(*key))
            .is_err()
    );
    assert_eq!(*completed.lock().unwrap(), vec![keys[0]]);
    server.join().unwrap();
}

#[test]
fn historical_concurrency_is_shared_across_providers_and_batches() {
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let server_active = active.clone();
    let server_peak = peak.clone();
    let (url, server) = mock_parallel_rpc(32, move |_| {
        let count = server_active.fetch_add(1, Ordering::SeqCst) + 1;
        server_peak.fetch_max(count, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(350));
        server_active.fetch_sub(1, Ordering::SeqCst);
        result(Value::Null)
    });
    let first = SolanaRpcProvider::new(url.clone()).with_historical_slot(100);
    let second = SolanaRpcProvider::new(url).with_historical_slot(200);
    let keys: Vec<_> = (0..16).map(|_| Pubkey::new_unique()).collect();
    thread::scope(|scope| {
        let a = scope.spawn(|| first.get_multiple_accounts(&keys).unwrap());
        let b = scope.spawn(|| second.get_multiple_accounts(&keys).unwrap());
        assert_eq!(a.join().unwrap().len(), keys.len());
        assert_eq!(b.join().unwrap().len(), keys.len());
    });
    let cores = thread::available_parallelism().map_or(1, usize::from);
    let limit = cores.saturating_mul(2).clamp(4, 16);
    assert!(peak.load(Ordering::SeqCst) > 1, "historical requests should overlap");
    assert!(peak.load(Ordering::SeqCst) <= limit, "providers must share the concurrency cap");
    assert_eq!(server.join().unwrap().len(), 32);
}

#[test]
fn unsupported_historical_method_has_a_concise_actionable_error() {
    let key = Pubkey::new_unique();
    let (url, server) = mock_rpc(vec![json!({
        "jsonrpc": "2.0", "id": 1,
        "error": {"code": -32601, "message": "Method not found"}
    })]);
    let error = SolanaRpcProvider::new(url)
        .with_historical_slot(100)
        .get_multiple_accounts(&[key])
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "RPC endpoint does not support --at-slot. Use a Helius RPC endpoint with Historical Accounts support."
    );
    assert_eq!(server.join().unwrap().len(), 1);
}

fn untracked_history() -> Value {
    json!({
        "jsonrpc": "2.0", "id": 1,
        "error": {"code": -32602, "message":
            "Invalid params: server returned error: account history is not tracked for accounts owned by Vote111111111111111111111111111111111111111"}
    })
}

#[test]
fn untracked_accounts_use_current_state_and_preserve_order() {
    let keys = [Pubkey::new_unique(), Pubkey::new_unique(), Pubkey::new_unique()];
    let strings = keys.map(|key| key.to_string());
    let (url, server) = mock_parallel_rpc(4, move |request| {
        let index = strings.iter().position(|key| request["params"][0] == *key).unwrap();
        if index == 1 && request["method"] == "getHistoricalAccountInfo" {
            untracked_history()
        } else {
            result(account((index as u64 + 1) * 10))
        }
    });
    let accounts =
        SolanaRpcProvider::new(url).with_historical_slot(100).get_multiple_accounts(&keys).unwrap();
    assert_eq!(
        accounts.iter().map(|a| a.as_ref().unwrap().lamports()).collect::<Vec<_>>(),
        vec![10, 20, 30]
    );
    let requests = server.join().unwrap();
    let fallback = requests.iter().find(|r| r["method"] == "getAccountInfo").unwrap();
    assert_eq!(fallback["params"], json!([keys[1].to_string(), {"encoding": "base64"}]));
    assert_eq!(requests.iter().filter(|r| r["method"] == "getHistoricalAccountInfo").count(), 3);
}

#[test]
fn untracked_account_missing_in_current_state_returns_none() {
    let (url, server) = mock_rpc(vec![untracked_history(), result(Value::Null)]);
    let accounts = SolanaRpcProvider::new(url)
        .with_historical_slot(100)
        .get_multiple_accounts(&[Pubkey::new_unique()])
        .unwrap();
    assert!(accounts[0].is_none());
    assert_eq!(server.join().unwrap()[1]["method"], "getAccountInfo");
}

#[test]
fn current_fallback_failure_is_reported() {
    let (url, server) = mock_rpc(vec![
        untracked_history(),
        json!({
            "jsonrpc": "2.0", "id": 1, "error": {"code": -32000, "message": "Access denied"}
        }),
    ]);
    let key = Pubkey::new_unique();
    let error = SolanaRpcProvider::new(url)
        .with_historical_slot(100)
        .get_multiple_accounts(&[key])
        .unwrap_err()
        .to_string();
    assert!(error.contains("current-state fallback"));
    assert!(error.contains("Access denied"));
    assert!(error.contains(&key.to_string()));
    assert_eq!(server.join().unwrap().len(), 2);
}

#[test]
fn unrelated_invalid_params_do_not_use_current_state() {
    let (url, server) = mock_rpc(vec![json!({
        "jsonrpc": "2.0", "id": 1, "error": {"code": -32602, "message": "Slot outside available history"}
    })]);
    assert!(
        SolanaRpcProvider::new(url)
            .with_historical_slot(100)
            .get_multiple_accounts(&[Pubkey::new_unique()])
            .is_err()
    );
    assert_eq!(server.join().unwrap().len(), 1);
}

#[test]
fn missing_current_rpc_method_is_not_reported_as_unsupported_history() {
    let (url, server) = mock_rpc(vec![json!({
        "jsonrpc": "2.0", "id": 1,
        "error": {"code": -32601, "message": "Method not found"}
    })]);
    let error = SolanaRpcProvider::new(url)
        .get_multiple_accounts(&[Pubkey::new_unique()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("Method not found"));
    assert!(!error.contains("--at-slot"));
    server.join().unwrap();
}

#[test]
fn current_reads_still_use_get_multiple_accounts() {
    let key = Pubkey::new_unique();
    let (url, server) = mock_rpc(vec![result(json!([account(42)]))]);
    let accounts = SolanaRpcProvider::new(url).get_multiple_accounts(&[key]).unwrap();
    assert_eq!(accounts[0].as_ref().unwrap().lamports(), 42);
    let requests = server.join().unwrap();
    assert_eq!(requests[0]["method"], "getMultipleAccounts");
    assert_eq!(requests[0]["params"], json!([[key.to_string()], {"encoding": "base64"}]));
}

#[test]
fn historical_clock_uses_block_time_and_epoch_schedule() {
    let slot = 460_000_000;
    let schedule = EpochSchedule::default();
    let (url, server) = mock_rpc(vec![
        json!({"jsonrpc": "2.0", "id": 1, "result": schedule}),
        json!({"jsonrpc": "2.0", "id": 1, "result": 1_790_000_000}),
        json!({"jsonrpc": "2.0", "id": 1, "result": 1_790_100_000}),
    ]);
    let accounts = SolanaRpcProvider::new(url)
        .with_historical_slot(slot)
        .with_simulation_sysvars()
        .get_multiple_accounts(&[Clock::id(), SlotHashes::id()])
        .unwrap();
    let clock: Clock = bincode::deserialize(accounts[0].as_ref().unwrap().data()).unwrap();
    assert_eq!(clock.slot, slot);
    assert_eq!(clock.epoch, schedule.get_epoch(slot));
    assert_eq!(clock.leader_schedule_epoch, schedule.get_leader_schedule_epoch(slot));
    assert_eq!(clock.epoch_start_timestamp, 1_790_000_000);
    assert_eq!(clock.unix_timestamp, 1_790_100_000);
    assert!(accounts[1].is_none());
    let requests = server.join().unwrap();
    assert_eq!(requests[0]["method"], "getEpochSchedule");
    assert_eq!(requests[1]["method"], "getBlockTime");
    assert_eq!(requests[1]["params"], json!([schedule.get_first_slot_in_epoch(clock.epoch)]));
    assert_eq!(requests[2]["params"], json!([slot]));
}

#[test]
fn historical_inspection_sysvars_use_current_fallback_not_vm_reconstruction() {
    let (url, server) = mock_parallel_rpc(4, |request| {
        if request["method"] == "getHistoricalAccountInfo" {
            untracked_history()
        } else {
            result(account(if request["params"][0] == Clock::id().to_string() { 10 } else { 20 }))
        }
    });
    let accounts = SolanaRpcProvider::new(url)
        .with_historical_slot(100)
        .get_multiple_accounts(&[Clock::id(), SlotHashes::id()])
        .unwrap();
    assert_eq!(accounts[0].as_ref().unwrap().lamports(), 10);
    assert_eq!(accounts[1].as_ref().unwrap().lamports(), 20);
    let requests = server.join().unwrap();
    assert_eq!(requests.iter().filter(|r| r["method"] == "getHistoricalAccountInfo").count(), 2);
    assert_eq!(requests.iter().filter(|r| r["method"] == "getAccountInfo").count(), 2);
}

#[test]
fn historical_clock_missing_block_time_is_an_error() {
    let (url, server) = mock_rpc(vec![
        json!({"jsonrpc": "2.0", "id": 1, "result": EpochSchedule::default()}),
        json!({"jsonrpc": "2.0", "id": 1, "result": null}),
    ]);
    let error = SolanaRpcProvider::new(url)
        .with_historical_slot(460_000_000)
        .with_simulation_sysvars()
        .get_multiple_accounts(&[Clock::id()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("Block time unavailable"));
    server.join().unwrap();
}

#[test]
fn historical_invalid_account_data_is_an_error() {
    let mut invalid = account(42);
    invalid["data"] = json!(["not base64!", "base64"]);
    let (url, server) = mock_rpc(vec![result(invalid)]);
    assert!(
        SolanaRpcProvider::new(url)
            .with_historical_slot(100)
            .get_multiple_accounts(&[Pubkey::new_unique()])
            .is_err()
    );
    server.join().unwrap();
}
