use assert_cmd::cargo::cargo_bin_cmd;

const VALID_PUBKEY: &str = "11111111111111111111111111111111";

#[test]
fn program_data_requires_explicit_output_mode_for_raw_binary() {
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("program-elf")
        .arg(VALID_PUBKEY)
        .arg("--rpc-url")
        .arg("http://127.0.0.1:1");

    let assert = cmd.assert().failure();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stdout.trim().is_empty(),
        "expected no stdout output for safety error, got: {stdout}"
    );
    assert!(
        stderr.contains("Refusing to write raw program bytes to stdout by default"),
        "expected explicit stdout safety error in stderr, got: {stderr}"
    );
    assert!(
        stderr.contains("-o <PATH>"),
        "expected stderr to suggest -o output mode, got: {stderr}"
    );
    assert!(
        stderr.contains("-o - for stdout"),
        "expected stderr to mention -o - stdout mode, got: {stderr}"
    );
}

#[test]
fn program_data_help_is_printed_to_stdout() {
    let mut cmd = cargo_bin_cmd!("sonar");
    cmd.arg("program-elf").arg("--help");

    let assert = cmd.assert().success();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.trim().is_empty(),
        "expected no stderr for --help, got: {stderr}"
    );
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("use \"-\" for stdout"));
    assert!(!stdout.contains("--stdout"));
}
