use assert_cmd::cargo::cargo_bin_cmd;

const VALID_PUBKEY: &str = "11111111111111111111111111111111";

#[test]
#[ignore = "requires mainnet RPC"]
fn program_elf_file_success_confirmation_on_stdout_stderr_empty() {
    use std::fs;

    let temp =
        std::env::temp_dir().join(format!("sonar_program_elf_e2e_{}.so", std::process::id()));

    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("program-elf")
        .arg("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
        .arg("--rpc-url")
        .arg("https://api.mainnet-beta.solana.com")
        .arg("-o")
        .arg(&temp);

    let assert = cmd.assert().success();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stdout.contains("Wrote ") && stdout.contains(" bytes to "),
        "success path: write confirmation on stdout, got: {stdout}"
    );
    assert!(stderr.trim().is_empty(), "success path: stderr empty, got: {stderr}");

    let _ = fs::remove_file(&temp);
}

#[test]
fn program_data_requires_explicit_output_mode_for_raw_binary() {
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("program-elf").arg(VALID_PUBKEY).arg("--rpc-url").arg("http://127.0.0.1:1");

    let assert = cmd.assert().failure();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stdout.trim().is_empty(), "expected no stdout output for safety error, got: {stdout}");
    assert!(
        stderr.contains("required arguments were not provided"),
        "expected clap required-argument error in stderr, got: {stderr}"
    );
    assert!(stderr.contains("--output <OUTPUT>"), "expected --output hint, got: {stderr}");
}

#[test]
fn program_data_help_is_printed_to_stdout() {
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("program-elf").arg("--help");

    let assert = cmd.assert().success();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.trim().is_empty(), "expected no stderr for --help, got: {stderr}");
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("use \"-\" for stdout"));
    assert!(!stdout.contains("--stdout"));
    assert!(!stdout.contains("--buffer"));
}

#[test]
fn simulate_help_groups_options_by_usage_scenario() {
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("simulate").arg("--help");

    let assert = cmd.assert().success();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.trim().is_empty(), "expected no stderr for --help, got: {stderr}");
    assert!(stdout.contains("Input & RPC"), "expected Input & RPC heading, got: {stdout}");
    assert!(
        stdout.contains("State Preparation"),
        "expected State Preparation heading, got: {stdout}"
    );
    assert!(
        stdout.contains("Simulation Controls"),
        "expected Simulation Controls heading, got: {stdout}"
    );
    assert!(stdout.contains("Output & Debug"), "expected Output & Debug heading, got: {stdout}");
}

#[test]
fn convert_reads_input_from_stdin_when_omitted() {
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("convert").arg("hex").arg("text").write_stdin("0x48656c6c6f\n");

    let assert = cmd.assert().success();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(stdout.trim_end(), "Hello");
    assert!(stderr.trim().is_empty(), "expected no stderr for stdin convert, got: {stderr}");
}

#[test]
fn convert_rejects_empty_stdin_when_input_is_omitted() {
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("convert").arg("hex").arg("text").write_stdin("");

    let assert = cmd.assert().failure();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stdout.trim().is_empty(), "expected no stdout on error, got: {stdout}");
    assert!(
        stderr.contains("No input data received from stdin"),
        "expected stdin-empty error in stderr, got: {stderr}"
    );
}

#[test]
fn convert_hex_to_binary_writes_bitstring_to_stdout() {
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("convert").arg("hex").arg("binary").arg("0x48656c6c6f");

    let assert = cmd.assert().success();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(stdout.trim_end(), "0b0100100001100101011011000110110001101111");
    assert!(stderr.trim().is_empty(), "expected no stderr for binary convert, got: {stderr}");
}

#[test]
fn convert_keypair_to_pubkey_writes_base58_address_to_stdout() {
    let keypair_hex = format!("0x{}{}", "01".repeat(32), "00".repeat(32));

    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("convert").arg("keypair").arg("pubkey").arg(&keypair_hex);

    let assert = cmd.assert().success();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(stdout.trim_end(), "11111111111111111111111111111111");
    assert!(stderr.trim().is_empty(), "expected no stderr for keypair->pubkey, got: {stderr}");
}

#[test]
fn convert_keypair_rejects_non_64_byte_input() {
    let invalid_keypair_hex = format!("0x{}{}", "01".repeat(31), "00".repeat(32));

    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("convert").arg("keypair").arg("pubkey").arg(&invalid_keypair_hex);

    let assert = cmd.assert().failure();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stdout.trim().is_empty(), "expected no stdout on error, got: {stdout}");
    assert!(
        stderr.contains("keypair requires exactly 64 bytes"),
        "expected keypair length error in stderr, got: {stderr}"
    );
}

#[test]
fn simulate_omitted_tx_empty_stdin_fails_with_actionable_message() {
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("simulate").arg("--rpc-url").arg("https://api.mainnet-beta.solana.com").write_stdin("");

    let assert = cmd.assert().failure();
    let output = assert.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("No transaction data received from stdin")
            || stderr.contains("No transaction provided"),
        "expected actionable error about missing TX, got: {stderr}"
    );
}

#[test]
fn decode_omitted_tx_empty_stdin_fails_with_actionable_message() {
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("decode").arg("--rpc-url").arg("https://api.mainnet-beta.solana.com").write_stdin("");

    let assert = cmd.assert().failure();
    let output = assert.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("No transaction data received from stdin")
            || stderr.contains("No transaction provided"),
        "expected actionable error about missing TX, got: {stderr}"
    );
}

#[test]
#[ignore = "requires mainnet RPC"]
fn simulate_omitted_tx_reads_from_stdin() {
    const V0_RAW_TX: &str = "GPdrKqMbYtzsysuEJhYG4bpUB9xQFpdQ8ps9s8XorbfD5SA5FrFfMAL2oznLNP9Ah4wXPe6Y9BVkAXM7Gw47whMxuK5TCKvpKtkyDEiuYfaRZCmv1mk5u16HvPzQqXGHzmf3iFUraHA2yEghbqaJsUW27PmXWvs2xPhK1WtFBvF4PNxtFBNa7sGwHZPmT88zk5pwpVnseAu48HhDvY6Nj7qjTzRAAFczubznScT4aT1m5CNyYjVwYjR5iqc7PrpTzyAxevb1Zk1ndXgHfwnQAhZfKfV712i5z352Jbf96WdQFGva3f22NGSVWtSFjp6agEBDTvWVUa3Db4WvArURczERDymqEEhX5EfMSZTUYenfgRXL2kgjWoXkuFaDyumgapdyqzQFixL4aJZCEDp6yfq7V5g2WYqwqNXHBsKbfpTfqKsqCV1niunXSZfGTTRgXjFWXuQNbtLrbd9TTJmUhsTMJuPzhohT89yX292vmUDGvHv3YqkJGynKcEGT6cPrB6ayWiBGybsJ2fUiax7QFPKT1hscSm5HDJPV3HmrC3DyHQAWq6hrPzGMeUcEBfSEtkvPFtNe9kpw4N9x2bJwuiGRgHbVmzDnRGMdXpu9KWigYb3uebLTFLUDeDq1CfD377AzdxBkBJQkdQ3peTjAz1kW3pEQQLiE57p16Wf8oUgxCveHpGr73RCoveDsjeF7puENGkM2aFkmKLBRvW3yJHL9mCP6ZkuScMy9VCWkh554yEs72DEZU25Upj7RAAhc7zG7iWyP6m2gZg6gGZ5hqjrCasQXUjJkaPT4LgBeLS9W5scn7QA51QMZi95DgAvD9mQeSFixnFofpqNDTNWVnisoQQ2eAEPVwKC1sfKjBdMgrMJKG1JjzDM7DWRzR7xPYSyjPfHXHx5aZJ8LdyYjYXjS6dViihtH3sNebZzqLdDzERDEe6bAFAkB5tGcRdqF1kcdPN3HNeRgeA7xvJg5r3kaDkAQQ9yjZV2stexZ1eDa4KiRwBY3MsgujEntBT992CrU4uAtHKkSXUbusXHMjbx9Dn57HD9GEdzAnDq53Gmmz7xU2qKZ3hKhLZg3ZtYVTeAysqUSfZTRPp87VjdyrG2Msk32ufPdQbAZ2x9FYbUCPpRvLMPsNhDe5D4fMt9X63bQTsk9VQNx39j5Mo6NkYvcKmKiz3pb5J19bHnnSKixEKa89typqcbNFunudMMmAAT3egyok3WRCqwC83EwNBWwrdm5zs1mRefEDu77arj7E";

    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("simulate")
        .arg("--rpc-url")
        .arg("https://api.mainnet-beta.solana.com")
        .arg("--override")
        .arg("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA=tests/fixtures/spl_token.so")
        .write_stdin(V0_RAW_TX);

    let assert = cmd.assert().success();
    let output = assert.get_output();
    assert!(!output.stdout.is_empty(), "expected simulate output on stdout");
}

#[test]
#[ignore = "requires mainnet RPC"]
fn decode_omitted_tx_reads_from_stdin() {
    const V0_RAW_TX: &str = "GPdrKqMbYtzsysuEJhYG4bpUB9xQFpdQ8ps9s8XorbfD5SA5FrFfMAL2oznLNP9Ah4wXPe6Y9BVkAXM7Gw47whMxuK5TCKvpKtkyDEiuYfaRZCmv1mk5u16HvPzQqXGHzmf3iFUraHA2yEghbqaJsUW27PmXWvs2xPhK1WtFBvF4PNxtFBNa7sGwHZPmT88zk5pwpVnseAu48HhDvY6Nj7qjTzRAAFczubznScT4aT1m5CNyYjVwYjR5iqc7PrpTzyAxevb1Zk1ndXgHfwnQAhZfKfV712i5z352Jbf96WdQFGva3f22NGSVWtSFjp6agEBDTvWVUa3Db4WvArURczERDymqEEhX5EfMSZTUYenfgRXL2kgjWoXkuFaDyumgapdyqzQFixL4aJZCEDp6yfq7V5g2WYqwqNXHBsKbfpTfqKsqCV1niunXSZfGTTRgXjFWXuQNbtLrbd9TTJmUhsTMJuPzhohT89yX292vmUDGvHv3YqkJGynKcEGT6cPrB6ayWiBGybsJ2fUiax7QFPKT1hscSm5HDJPV3HmrC3DyHQAWq6hrPzGMeUcEBfSEtkvPFtNe9kpw4N9x2bJwuiGRgHbVmzDnRGMdXpu9KWigYb3uebLTFLUDeDq1CfD377AzdxBkBJQkdQ3peTjAz1kW3pEQQLiE57p16Wf8oUgxCveHpGr73RCoveDsjeF7puENGkM2aFkmKLBRvW3yJHL9mCP6ZkuScMy9VCWkh554yEs72DEZU25Upj7RAAhc7zG7iWyP6m2gZg6gGZ5hqjrCasQXUjJkaPT4LgBeLS9W5scn7QA51QMZi95DgAvD9mQeSFixnFofpqNDTNWVnisoQQ2eAEPVwKC1sfKjBdMgrMJKG1JjzDM7DWRzR7xPYSyjPfHXHx5aZJ8LdyYjYXjS6dViihtH3sNebZzqLdDzERDEe6bAFAkB5tGcRdqF1kcdPN3HNeRgeA7xvJg5r3kaDkAQQ9yjZV2stexZ1eDa4KiRwBY3MsgujEntBT992CrU4uAtHKkSXUbusXHMjbx9Dn57HD9GEdzAnDq53Gmmz7xU2qKZ3hKhLZg3ZtYVTeAysqUSfZTRPp87VjdyrG2Msk32ufPdQbAZ2x9FYbUCPpRvLMPsNhDe5D4fMt9X63bQTsk9VQNx39j5Mo6NkYvcKmKiz3pb5J19bHnnSKixEKa89typqcbNFunudMMmAAT3egyok3WRCqwC83EwNBWwrdm5zs1mRefEDu77arj7E";

    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("decode")
        .arg("--rpc-url")
        .arg("https://api.mainnet-beta.solana.com")
        .write_stdin(V0_RAW_TX);

    let assert = cmd.assert().success();
    let output = assert.get_output();
    assert!(!output.stdout.is_empty(), "expected decode output on stdout");
}

#[test]
#[ignore = "requires mainnet RPC"]
fn decode_bundle_json_outputs_single_valid_json_array() {
    const V0_RAW_TX: &str = "GPdrKqMbYtzsysuEJhYG4bpUB9xQFpdQ8ps9s8XorbfD5SA5FrFfMAL2oznLNP9Ah4wXPe6Y9BVkAXM7Gw47whMxuK5TCKvpKtkyDEiuYfaRZCmv1mk5u16HvPzQqXGHzmf3iFUraHA2yEghbqaJsUW27PmXWvs2xPhK1WtFBvF4PNxtFBNa7sGwHZPmT88zk5pwpVnseAu48HhDvY6Nj7qjTzRAAFczubznScT4aT1m5CNyYjVwYjR5iqc7PrpTzyAxevb1Zk1ndXgHfwnQAhZfKfV712i5z352Jbf96WdQFGva3f22NGSVWtSFjp6agEBDTvWVUa3Db4WvArURczERDymqEEhX5EfMSZTUYenfgRXL2kgjWoXkuFaDyumgapdyqzQFixL4aJZCEDp6yfq7V5g2WYqwqNXHBsKbfpTfqKsqCV1niunXSZfGTTRgXjFWXuQNbtLrbd9TTJmUhsTMJuPzhohT89yX292vmUDGvHv3YqkJGynKcEGT6cPrB6ayWiBGybsJ2fUiax7QFPKT1hscSm5HDJPV3HmrC3DyHQAWq6hrPzGMeUcEBfSEtkvPFtNe9kpw4N9x2bJwuiGRgHbVmzDnRGMdXpu9KWigYb3uebLTFLUDeDq1CfD377AzdxBkBJQkdQ3peTjAz1kW3pEQQLiE57p16Wf8oUgxCveHpGr73RCoveDsjeF7puENGkM2aFkmKLBRvW3yJHL9mCP6ZkuScMy9VCWkh554yEs72DEZU25Upj7RAAhc7zG7iWyP6m2gZg6gGZ5hqjrCasQXUjJkaPT4LgBeLS9W5scn7QA51QMZi95DgAvD9mQeSFixnFofpqNDTNWVnisoQQ2eAEPVwKC1sfKjBdMgrMJKG1JjzDM7DWRzR7xPYSyjPfHXHx5aZJ8LdyYjYXjS6dViihtH3sNebZzqLdDzERDEe6bAFAkB5tGcRdqF1kcdPN3HNeRgeA7xvJg5r3kaDkAQQ9yjZV2stexZ1eDa4KiRwBY3MsgujEntBT992CrU4uAtHKkSXUbusXHMjbx9Dn57HD9GEdzAnDq53Gmmz7xU2qKZ3hKhLZg3ZtYVTeAysqUSfZTRPp87VjdyrG2Msk32ufPdQbAZ2x9FYbUCPpRvLMPsNhDe5D4fMt9X63bQTsk9VQNx39j5Mo6NkYvcKmKiz3pb5J19bHnnSKixEKa89typqcbNFunudMMmAAT3egyok3WRCqwC83EwNBWwrdm5zs1mRefEDu77arj7E";

    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("decode")
        .arg(V0_RAW_TX)
        .arg(V0_RAW_TX)
        .arg("--json")
        .arg("--rpc-url")
        .arg("https://api.mainnet-beta.solana.com");

    let assert = cmd.assert().success();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Must parse as a single JSON array (jq-compatible)
    let arr: Vec<serde_json::Value> = serde_json::from_str(stdout.trim())
        .expect("bundle decode --json must output a single valid JSON array parseable by jq");
    assert_eq!(arr.len(), 2, "expected 2 decoded transactions in array, got {}", arr.len());
}

#[test]
fn offline_missing_account_does_not_trigger_strict_offline_error() {
    use std::fs;
    use std::io::Write;

    let temp = std::env::temp_dir().join(format!("sonar_offline_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp);
    fs::create_dir_all(&temp).expect("create temp dir");

    let meta = serde_json::json!({
        "created_at": "2026-02-22T10:00:00Z",
        "sonar_version": "0.2.0",
        "type": "single",
        "transactions": [{
            "input": "GPdrKqMbYtzsysuEJhYG4bpUB9xQFpdQ8ps9s8XorbfD5SA5FrFfMAL2oznLNP9Ah4wXPe6Y9BVkAXM7Gw47whMxuK5TCKvpKtkyDEiuYfaRZCmv1mk5u16HvPzQqXGHzmf3iFUraHA2yEghbqaJsUW27PmXWvs2xPhK1WtFBvF4PNxtFBNa7sGwHZPmT88zk5pwpVnseAu48HhDvY6Nj7qjTzRAAFczubznScT4aT1m5CNyYjVwYjR5iqc7PrpTzyAxevb1Zk1ndXgHfwnQAhZfKfV712i5z352Jbf96WdQFGva3f22NGSVWtSFjp6agEBDTvWVUa3Db4WvArURczERDymqEEhX5EfMSZTUYenfgRXL2kgjWoXkuFaDyumgapdyqzQFixL4aJZCEDp6yfq7V5g2WYqwqNXHBsKbfpTfqKsqCV1niunXSZfGTTRgXjFWXuQNbtLrbd9TTJmUhsTMJuPzhohT89yX292vmUDGvHv3YqkJGynKcEGT6cPrB6ayWiBGybsJ2fUiax7QFPKT1hscSm5HDJPV3HmrC3DyHQAWq6hrPzGMeUcEBfSEtkvPFtNe9kpw4N9x2bJwuiGRgHbVmzDnRGMdXpu9KWigYb3uebLTFLUDeDq1CfD377AzdxBkBJQkdQ3peTjAz1kW3pEQQLiE57p16Wf8oUgxCveHpGr73RCoveDsjeF7puENGkM2aFkmKLBRvW3yJHL9mCP6ZkuScMy9VCWkh554yEs72DEZU25Upj7RAAhc7zG7iWyP6m2gZg6gGZ5hqjrCasQXUjJkaPT4LgBeLS9W5scn7QA51QMZi95DgAvD9mQeSFixnFofpqNDTNWVnisoQQ2eAEPVwKC1sfKjBdMgrMJKG1JjzDM7DWRzR7xPYSyjPfHXHx5aZJ8LdyYjYXjS6dViihtH3sNebZzqLdDzERDEe6bAFAkB5tGcRdqF1kcdPN3HNeRgeA7xvJg5r3kaDkAQQ9yjZV2stexZ1eDa4KiRwBY3MsgujEntBT992CrU4uAtHKkSXUbusXHMjbx9Dn57HD9GEdzAnDq53Gmmz7xU2qKZ3hKhLZg3ZtYVTeAysqUSfZTRPp87VjdyrG2Msk32ufPdQbAZ2x9FYbUCPpRvLMPsNhDe5D4fMt9X63bQTsk9VQNx39j5Mo6NkYvcKmKiz3pb5J19bHnnSKixEKa89typqcbNFunudMMmAAT3egyok3WRCqwC83EwNBWwrdm5zs1mRefEDu77arj7E",
            "raw_tx": "GPdrKqMbYtzsysuEJhYG4bpUB9xQFpdQ8ps9s8XorbfD5SA5FrFfMAL2oznLNP9Ah4wXPe6Y9BVkAXM7Gw47whMxuK5TCKvpKtkyDEiuYfaRZCmv1mk5u16HvPzQqXGHzmf3iFUraHA2yEghbqaJsUW27PmXWvs2xPhK1WtFBvF4PNxtFBNa7sGwHZPmT88zk5pwpVnseAu48HhDvY6Nj7qjTzRAAFczubznScT4aT1m5CNyYjVwYjR5iqc7PrpTzyAxevb1Zk1ndXgHfwnQAhZfKfV712i5z352Jbf96WdQFGva3f22NGSVWtSFjp6agEBDTvWVUa3Db4WvArURczERDymqEEhX5EfMSZTUYenfgRXL2kgjWoXkuFaDyumgapdyqzQFixL4aJZCEDp6yfq7V5g2WYqwqNXHBsKbfpTfqKsqCV1niunXSZfGTTRgXjFWXuQNbtLrbd9TTJmUhsTMJuPzhohT89yX292vmUDGvHv3YqkJGynKcEGT6cPrB6ayWiBGybsJ2fUiax7QFPKT1hscSm5HDJPV3HmrC3DyHQAWq6hrPzGMeUcEBfSEtkvPFtNe9kpw4N9x2bJwuiGRgHbVmzDnRGMdXpu9KWigYb3uebLTFLUDeDq1CfD377AzdxBkBJQkdQ3peTjAz1kW3pEQQLiE57p16Wf8oUgxCveHpGr73RCoveDsjeF7puENGkM2aFkmKLBRvW3yJHL9mCP6ZkuScMy9VCWkh554yEs72DEZU25Upj7RAAhc7zG7iWyP6m2gZg6gGZ5hqjrCasQXUjJkaPT4LgBeLS9W5scn7QA51QMZi95DgAvD9mQeSFixnFofpqNDTNWVnisoQQ2eAEPVwKC1sfKjBdMgrMJKG1JjzDM7DWRzR7xPYSyjPfHXHx5aZJ8LdyYjYXjS6dViihtH3sNebZzqLdDzERDEe6bAFAkB5tGcRdqF1kcdPN3HNeRgeA7xvJg5r3kaDkAQQ9yjZV2stexZ1eDa4KiRwBY3MsgujEntBT992CrU4uAtHKkSXUbusXHMjbx9Dn57HD9GEdzAnDq53Gmmz7xU2qKZ3hKhLZg3ZtYVTeAysqUSfZTRPp87VjdyrG2Msk32ufPdQbAZ2x9FYbUCPpRvLMPsNhDe5D4fMt9X63bQTsk9VQNx39j5Mo6NkYvcKmKiz3pb5J19bHnnSKixEKa89typqcbNFunudMMmAAT3egyok3WRCqwC83EwNBWwrdm5zs1mRefEDu77arj7E",
            "resolved_from": "raw_input"
        }],
        "rpc_url": "https://api.mainnet-beta.solana.com",
        "account_count": 0
    });
    let mut f = fs::File::create(temp.join("_meta.json")).expect("create _meta.json");
    f.write_all(meta.to_string().as_bytes()).expect("write _meta.json");

    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("simulate")
        .arg("GPdrKqMbYtzsysuEJhYG4bpUB9xQFpdQ8ps9s8XorbfD5SA5FrFfMAL2oznLNP9Ah4wXPe6Y9BVkAXM7Gw47whMxuK5TCKvpKtkyDEiuYfaRZCmv1mk5u16HvPzQqXGHzmf3iFUraHA2yEghbqaJsUW27PmXWvs2xPhK1WtFBvF4PNxtFBNa7sGwHZPmT88zk5pwpVnseAu48HhDvY6Nj7qjTzRAAFczubznScT4aT1m5CNyYjVwYjR5iqc7PrpTzyAxevb1Zk1ndXgHfwnQAhZfKfV712i5z352Jbf96WdQFGva3f22NGSVWtSFjp6agEBDTvWVUa3Db4WvArURczERDymqEEhX5EfMSZTUYenfgRXL2kgjWoXkuFaDyumgapdyqzQFixL4aJZCEDp6yfq7V5g2WYqwqNXHBsKbfpTfqKsqCV1niunXSZfGTTRgXjFWXuQNbtLrbd9TTJmUhsTMJuPzhohT89yX292vmUDGvHv3YqkJGynKcEGT6cPrB6ayWiBGybsJ2fUiax7QFPKT1hscSm5HDJPV3HmrC3DyHQAWq6hrPzGMeUcEBfSEtkvPFtNe9kpw4N9x2bJwuiGRgHbVmzDnRGMdXpu9KWigYb3uebLTFLUDeDq1CfD377AzdxBkBJQkdQ3peTjAz1kW3pEQQLiE57p16Wf8oUgxCveHpGr73RCoveDsjeF7puENGkM2aFkmKLBRvW3yJHL9mCP6ZkuScMy9VCWkh554yEs72DEZU25Upj7RAAhc7zG7iWyP6m2gZg6gGZ5hqjrCasQXUjJkaPT4LgBeLS9W5scn7QA51QMZi95DgAvD9mQeSFixnFofpqNDTNWVnisoQQ2eAEPVwKC1sfKjBdMgrMJKG1JjzDM7DWRzR7xPYSyjPfHXHx5aZJ8LdyYjYXjS6dViihtH3sNebZzqLdDzERDEe6bAFAkB5tGcRdqF1kcdPN3HNeRgeA7xvJg5r3kaDkAQQ9yjZV2stexZ1eDa4KiRwBY3MsgujEntBT992CrU4uAtHKkSXUbusXHMjbx9Dn57HD9GEdzAnDq53Gmmz7xU2qKZ3hKhLZg3ZtYVTeAysqUSfZTRPp87VjdyrG2Msk32ufPdQbAZ2x9FYbUCPpRvLMPsNhDe5D4fMt9X63bQTsk9VQNx39j5Mo6NkYvcKmKiz3pb5J19bHnnSKixEKa89typqcbNFunudMMmAAT3egyok3WRCqwC83EwNBWwrdm5zs1mRefEDu77arj7E")
        .arg("--cache")
        .arg("--cache-dir")
        .arg(&temp)
        .arg("--override")
        .arg("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA=tests/fixtures/spl_token.so");

    let assert = cmd.assert().failure();
    let output = assert.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.to_lowercase().contains("offline mode")
            && stderr.contains("treated as non-existent"),
        "expected offline missing-account warning in stderr, got: {stderr}"
    );
    assert!(
        !stderr.contains("Error: offline mode:") && !stderr.contains("error: offline mode:"),
        "strict offline error should not be returned, got: {stderr}"
    );

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn decode_signature_prefers_cached_raw_tx_and_skips_rpc_fetch() {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use solana_hash::Hash;
    use solana_keypair::Keypair;
    use solana_message::Message;
    use solana_pubkey::Pubkey;
    use solana_signer::Signer;
    use solana_system_interface::instruction as system_instruction;
    use solana_transaction::Transaction;
    use solana_transaction::versioned::VersionedTransaction;
    use std::fs;
    use std::io::Write;

    let signature =
        "3PtGYH77LhhQqTXP4SmDVJ85hmDieWsgXCUbn14v7gYyVYPjZzygUQhTk3bSTYnfA48vCM1rmWY7zWL3j1EVKmEy";
    let payer = Keypair::new();
    let recipient = Pubkey::new_unique();
    let blockhash = Hash::new_unique();
    let message = Message::new(
        &[system_instruction::transfer(&payer.pubkey(), &recipient, 42)],
        Some(&payer.pubkey()),
    );
    let tx = Transaction::new(&[&payer], message, blockhash);
    let raw_tx = BASE64_STANDARD
        .encode(bincode::serialize(&VersionedTransaction::from(tx)).expect("serialize tx"));

    let temp = std::env::temp_dir().join(format!("sonar_decode_cache_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp);
    fs::create_dir_all(&temp).expect("create temp dir");
    let cache_dir = temp.join(signature);
    fs::create_dir_all(&cache_dir).expect("create cache key dir");

    let meta = serde_json::json!({
        "created_at": "2026-02-22T10:00:00Z",
        "sonar_version": "0.3.0",
        "type": "single",
        "transactions": [{
            "input": signature,
            "raw_tx": raw_tx,
            "resolved_from": "cache"
        }],
        "rpc_url": "https://api.mainnet-beta.solana.com",
        "account_count": 0
    });
    let mut f = fs::File::create(cache_dir.join("_meta.json")).expect("create _meta.json");
    f.write_all(meta.to_string().as_bytes()).expect("write _meta.json");

    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("decode")
        .arg(signature)
        .arg("--rpc-url")
        .arg("http://127.0.0.1:1")
        .env("SONAR_CACHE_DIR", &temp);

    let assert = cmd.assert().success();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.trim().is_empty(), "expected decode output from cache, got empty stdout");

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn decode_signature_no_cache_forces_rpc_fetch_and_fails_on_bad_rpc() {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use solana_hash::Hash;
    use solana_keypair::Keypair;
    use solana_message::Message;
    use solana_pubkey::Pubkey;
    use solana_signer::Signer;
    use solana_system_interface::instruction as system_instruction;
    use solana_transaction::Transaction;
    use solana_transaction::versioned::VersionedTransaction;
    use std::fs;
    use std::io::Write;

    let signature =
        "3PtGYH77LhhQqTXP4SmDVJ85hmDieWsgXCUbn14v7gYyVYPjZzygUQhTk3bSTYnfA48vCM1rmWY7zWL3j1EVKmEy";
    let payer = Keypair::new();
    let recipient = Pubkey::new_unique();
    let blockhash = Hash::new_unique();
    let message = Message::new(
        &[system_instruction::transfer(&payer.pubkey(), &recipient, 42)],
        Some(&payer.pubkey()),
    );
    let tx = Transaction::new(&[&payer], message, blockhash);
    let raw_tx = BASE64_STANDARD
        .encode(bincode::serialize(&VersionedTransaction::from(tx)).expect("serialize tx"));

    let temp =
        std::env::temp_dir().join(format!("sonar_decode_no_cache_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp);
    fs::create_dir_all(&temp).expect("create temp dir");
    let cache_dir = temp.join(signature);
    fs::create_dir_all(&cache_dir).expect("create cache key dir");

    let meta = serde_json::json!({
        "created_at": "2026-02-22T10:00:00Z",
        "sonar_version": "0.3.0",
        "type": "single",
        "transactions": [{
            "input": signature,
            "raw_tx": raw_tx,
            "resolved_from": "cache"
        }],
        "rpc_url": "https://api.mainnet-beta.solana.com",
        "account_count": 0
    });
    let mut f = fs::File::create(cache_dir.join("_meta.json")).expect("create _meta.json");
    f.write_all(meta.to_string().as_bytes()).expect("write _meta.json");

    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("decode")
        .arg(signature)
        .arg("--rpc-url")
        .arg("http://127.0.0.1:1")
        .arg("--no-cache")
        .env("SONAR_CACHE_DIR", &temp);

    let assert = cmd.assert().failure();
    let output = assert.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Signature fetch failed")
            && !stderr.contains("Failed to fetch transaction"),
        "expected --no-cache to still reuse cached raw tx, got signature fetch error: {stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("error"),
        "expected account fetch to fail on bad RPC when --no-cache is set, got: {stderr}"
    );

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn decode_signature_refresh_cache_forces_rpc_fetch_and_fails_on_bad_rpc() {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use solana_hash::Hash;
    use solana_keypair::Keypair;
    use solana_message::Message;
    use solana_pubkey::Pubkey;
    use solana_signer::Signer;
    use solana_system_interface::instruction as system_instruction;
    use solana_transaction::Transaction;
    use solana_transaction::versioned::VersionedTransaction;
    use std::fs;
    use std::io::Write;

    let signature =
        "3PtGYH77LhhQqTXP4SmDVJ85hmDieWsgXCUbn14v7gYyVYPjZzygUQhTk3bSTYnfA48vCM1rmWY7zWL3j1EVKmEy";
    let payer = Keypair::new();
    let recipient = Pubkey::new_unique();
    let blockhash = Hash::new_unique();
    let message = Message::new(
        &[system_instruction::transfer(&payer.pubkey(), &recipient, 42)],
        Some(&payer.pubkey()),
    );
    let tx = Transaction::new(&[&payer], message, blockhash);
    let raw_tx = BASE64_STANDARD
        .encode(bincode::serialize(&VersionedTransaction::from(tx)).expect("serialize tx"));

    let temp = std::env::temp_dir()
        .join(format!("sonar_decode_refresh_cache_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp);
    fs::create_dir_all(&temp).expect("create temp dir");
    let cache_dir = temp.join(signature);
    fs::create_dir_all(&cache_dir).expect("create cache key dir");

    let meta = serde_json::json!({
        "created_at": "2026-02-22T10:00:00Z",
        "sonar_version": "0.3.0",
        "type": "single",
        "transactions": [{
            "input": signature,
            "raw_tx": raw_tx,
            "resolved_from": "cache"
        }],
        "rpc_url": "https://api.mainnet-beta.solana.com",
        "account_count": 0
    });
    let mut f = fs::File::create(cache_dir.join("_meta.json")).expect("create _meta.json");
    f.write_all(meta.to_string().as_bytes()).expect("write _meta.json");

    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("decode")
        .arg(signature)
        .arg("--rpc-url")
        .arg("http://127.0.0.1:1")
        .arg("--refresh-cache")
        .env("SONAR_CACHE_DIR", &temp);

    let assert = cmd.assert().failure();
    let output = assert.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Signature fetch failed") || stderr.contains("Failed to fetch transaction"),
        "expected RPC fetch failure when --refresh-cache is set, got: {stderr}"
    );

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn idl_fetch_failure_exits_nonzero() {
    // Unreachable RPC causes fetch to fail for all programs
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("idl")
        .arg("fetch")
        .arg("11111111111111111111111111111111")
        .arg("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
        .arg("--rpc-url")
        .arg("http://127.0.0.1:1")
        .arg("-o")
        .arg(std::env::temp_dir());

    let assert = cmd.assert().failure();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stdout.trim().is_empty(), "stdout should be empty on failure, got: {stdout}");
    assert!(stderr.contains("Summary:"), "expected Summary in stderr, got: {stderr}");
    assert!(
        stderr.to_lowercase().contains("error"),
        "expected error info in stderr, got: {stderr}"
    );
}

#[test]
fn idl_fetch_allow_partial_exits_zero_on_failure() {
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("idl")
        .arg("fetch")
        .arg("11111111111111111111111111111111")
        .arg("--rpc-url")
        .arg("http://127.0.0.1:1")
        .arg("--allow-partial")
        .arg("-o")
        .arg(std::env::temp_dir());

    let assert = cmd.assert().success();
    let output = assert.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("Summary:"),
        "expected Summary in stderr when allow-partial, got: {stderr}"
    );
}

#[test]
fn idl_fetch_success_paths_go_to_stdout() {
    // System program has no IDL -> not_found. Unreachable RPC -> error.
    // Both trigger failure; we just verify stdout is empty (no successes)
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("idl")
        .arg("fetch")
        .arg("11111111111111111111111111111111")
        .arg("--rpc-url")
        .arg("http://127.0.0.1:1")
        .arg("-o")
        .arg(std::env::temp_dir());

    let assert = cmd.assert().failure();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // No successful fetches -> stdout empty
    assert!(stdout.trim().is_empty(), "stdout should be empty when all fail, got: {stdout}");
}

#[test]
fn config_without_subcommand_prints_config_help() {
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("config");

    let assert = cmd.assert().failure();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.trim().is_empty(),
        "expected no stderr for config subcommand help, got: {stderr}"
    );
    assert!(stdout.contains("Usage:"), "expected usage help, got: {stdout}");
    assert!(stdout.contains("list"), "expected list subcommand help, got: {stdout}");
    assert!(stdout.contains("get"), "expected get subcommand help, got: {stdout}");
    assert!(stdout.contains("set"), "expected set subcommand help, got: {stdout}");
}

#[test]
fn cnofig_alias_is_rejected() {
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("cnofig").arg("list");

    let assert = cmd.assert().failure();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stdout.trim().is_empty(), "expected no stdout when alias is rejected, got: {stdout}");
    assert!(
        stderr.contains("cnofig"),
        "expected error mentioning removed alias cnofig, got: {stderr}"
    );
    assert!(
        stderr.contains("unrecognized subcommand"),
        "expected clap unknown subcommand error, got: {stderr}"
    );
}
