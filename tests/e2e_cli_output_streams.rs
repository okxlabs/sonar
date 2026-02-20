use assert_cmd::cargo::cargo_bin_cmd;

const VALID_PUBKEY: &str = "11111111111111111111111111111111";

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
