use std::process::Command;

/// Embed the current git commit hash so the binary can report it via
/// `sonar --version` (clap only knows `CARGO_PKG_VERSION` by itself).
fn main() {
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=SONAR_GIT_HASH={git_hash}");

    // Re-run when HEAD moves so the embedded hash stays current. Paths are
    // relative to this crate (two levels under the repo root). Best-effort:
    // a clean rebuild always re-reads the hash regardless.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");
}
