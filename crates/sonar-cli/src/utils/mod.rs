pub mod config;
pub mod progress;

use std::io::{IsTerminal, Read};

/// Read CLI input from a positional argument or stdin.
///
/// Returns the trimmed, non-empty input string, or an error describing what
/// was expected.  The `kind` label (e.g. `"bytes"`, `"JSON"`, `"transaction"`)
/// is embedded in error messages.
pub fn read_cli_input(input: Option<&str>, kind: &str) -> Result<String, String> {
    if let Some(input) = input {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(format!("{kind} input cannot be empty"));
        }
        return Ok(trimmed.to_owned());
    }

    if !std::io::stdin().is_terminal() {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("Failed to read {kind} from stdin: {e}"))?;
        let trimmed = buf.trim();
        if trimmed.is_empty() {
            return Err(format!("No {kind} data received from stdin"));
        }
        return Ok(trimmed.to_owned());
    }

    Err(format!("No input provided. Pass {kind} as a positional argument or pipe via stdin"))
}

/// Strip an optional `0x` or `0X` prefix from a hex string.
pub fn strip_hex_prefix(s: &str) -> &str {
    s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s)
}

/// Parse a hex string (with optional `0x`/`0X` prefix) into bytes.
///
/// Returns an error for empty data, odd-length strings, or invalid hex characters.
pub fn parse_hex_data(raw: &str) -> Result<Vec<u8>, String> {
    let hex_str = strip_hex_prefix(raw.trim());

    if hex_str.is_empty() {
        return Err("HEX_DATA must not be empty".to_string());
    }
    if hex_str.len() % 2 != 0 {
        return Err(format!(
            "HEX_DATA has odd length {}; expected an even number of hex characters",
            hex_str.len()
        ));
    }

    (0..hex_str.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex_str[i..i + 2], 16)
                .map_err(|err| format!("Invalid hex at position {i}: {err}"))
        })
        .collect()
}
