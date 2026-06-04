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

/// An account reference with its `AccountMeta` flags, parsed from the shared
/// `<PUBKEY>[:<flags>]` mini-syntax used across the `simulate` instruction flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountMetaFlags {
    pub pubkey: solana_pubkey::Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
}

/// Parse `<PUBKEY>[:<flags>]` where `<flags>` is any combination of `s`
/// (signer) and `w` (writable). An absent suffix means a read-only non-signer,
/// matching Solana's `AccountMeta` defaults. Flags are order-independent and
/// may repeat. The same grammar is shared by `--ix accounts=`,
/// `--patch-ix-account`, and `--insert-ix-account` so a suffix means the same
/// thing everywhere.
pub fn parse_account_meta_flags(raw: &str) -> Result<AccountMetaFlags, String> {
    use std::str::FromStr;

    let trimmed = raw.trim();
    let (pubkey_str, flags, had_colon) = match trimmed.split_once(':') {
        Some((pubkey_str, flags)) => (pubkey_str, flags, true),
        None => (trimmed, "", false),
    };
    if pubkey_str.is_empty() {
        return Err("Account entry is missing a pubkey".to_string());
    }
    if had_colon && flags.is_empty() {
        return Err(format!("Account `{pubkey_str}` has an empty `:` flag suffix"));
    }

    let pubkey = solana_pubkey::Pubkey::from_str(pubkey_str)
        .map_err(|err| format!("Failed to parse account pubkey `{pubkey_str}`: {err}"))?;

    let mut is_signer = false;
    let mut is_writable = false;
    for flag in flags.chars() {
        match flag {
            's' => is_signer = true,
            'w' => is_writable = true,
            _ => {
                return Err(format!(
                    "Unknown account flag `{flag}` for `{pubkey_str}`; \
                     valid flags are `s` (signer) and `w` (writable)"
                ));
            }
        }
    }

    Ok(AccountMetaFlags { pubkey, is_signer, is_writable })
}
