//! Simulate command arguments and related types.

use std::{path::PathBuf, str::FromStr};

use chrono::DateTime;
use clap::Args;
use serde::Deserialize;
use solana_account::Account;
use solana_pubkey::Pubkey;

use super::RpcArgs;

#[derive(Args, Debug)]
pub struct SimulateArgs {
    #[command(flatten)]
    pub transaction: TransactionInputArgs,
    #[command(flatten)]
    pub rpc: RpcArgs,
    /// Replace an on-chain program or account.
    /// Format: <PUBKEY>=<PATH> (.so/.elf for programs, .json for accounts)
    #[arg(
        long = "replace",
        value_name = "MAPPING",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub replacements: Vec<String>,
    /// Fund a system account with SOL. Format: <PUBKEY>=<LAMPORTS> or <PUBKEY>=<AMOUNT>sol
    #[arg(
        long = "fund-sol",
        value_name = "FUNDING",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub fundings: Vec<String>,
    /// Fund a token account.
    /// Format: <ACCOUNT>=<AMOUNT> or <ACCOUNT>:<MINT>=<AMOUNT> (mint auto-detected if account exists on-chain).
    /// Integer amounts are treated as raw token units; decimal amounts (e.g. 1.5) are
    /// converted using the mint's decimals (e.g. 1.5 with 6 decimals → 1500000).
    #[arg(
        long = "fund-token",
        value_name = "FUNDING",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub token_fundings: Vec<String>,
    /// Always print raw instruction data, even when parser succeeds
    #[arg(long = "raw-ix-data", env = "SONAR_RAW_IX_DATA")]
    pub ix_data: bool,
    /// Verify transaction signatures during simulation
    #[arg(long = "check-sig", env = "SONAR_VERIFY_SIGNATURES")]
    pub verify_signatures: bool,
    /// Directory containing Anchor IDL JSON files
    #[arg(long = "idl-dir", value_name = "DIR", env = "SONAR_IDL_DIR")]
    pub idl_dir: Option<PathBuf>,
    /// Show SOL and token balance changes after simulation
    #[arg(short = 'b', long = "show-balance-change", env = "SONAR_SHOW_BALANCE_CHANGE")]
    pub show_balance_change: bool,
    /// Print raw program logs instead of structured execution trace
    #[arg(long = "raw-log", env = "SONAR_RAW_LOG")]
    pub raw_log: bool,
    /// Show detailed instruction information (accounts, parsed fields, inner instructions)
    #[arg(short = 'd', long = "show-ix-detail", env = "SONAR_SHOW_IX_DETAIL")]
    pub show_ix_detail: bool,
    /// Override the Clock sysvar's unix_timestamp for simulation.
    /// Supports Unix timestamp (e.g. 1700000000) or RFC3339 (e.g. 2024-01-01T00:00:00Z).
    #[arg(long = "timestamp", value_name = "TIMESTAMP", value_parser = parse_timestamp)]
    pub timestamp: Option<i64>,
    /// Override the simulation slot
    #[arg(long = "slot", value_name = "SLOT")]
    pub slot: Option<u64>,
    /// Patch bytes in an account data field before simulation.
    /// Format: <PUBKEY>=<OFFSET>:<HEX_DATA>
    /// HEX_DATA may optionally start with 0x.
    #[arg(
        short = 'p',
        long = "patch-account-data",
        value_name = "PATCH",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub data_patches: Vec<String>,
    /// Save fetched account data to a directory as <PUBKEY>.json before applying patches
    #[arg(long = "dump-accounts", value_name = "DIR")]
    pub dump_accounts: Option<PathBuf>,
    /// Load account data from a local directory (<PUBKEY>.json).
    /// Missing accounts fall back to RPC unless --offline is set
    #[arg(long = "load-accounts", value_name = "DIR")]
    pub load_accounts: Option<PathBuf>,
    /// Disable RPC fallback; error if any account is missing from --load-accounts directory
    #[arg(long = "offline", requires = "load_accounts")]
    pub offline: bool,
}

#[derive(Args, Debug, Clone)]
pub struct TransactionInputArgs {
    /// Raw transaction (Base58/Base64) or transaction signature.
    /// Pass multiple values for bundle mode
    #[arg(value_name = "TX")]
    pub tx: Vec<String>,
    /// Output as JSON instead of human-readable text
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Clone, Debug)]
pub enum Replacement {
    Program { program_id: Pubkey, so_path: PathBuf },
    Account { pubkey: Pubkey, account: Account, source_path: PathBuf },
}

impl Replacement {
    /// Returns the pubkey being replaced, regardless of replacement type.
    pub fn pubkey(&self) -> Pubkey {
        match self {
            Replacement::Program { program_id, .. } => *program_id,
            Replacement::Account { pubkey, .. } => *pubkey,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Funding {
    pub pubkey: Pubkey,
    pub amount_lamports: u64,
}

/// How the user specified the token amount on the CLI.
#[derive(Clone, Debug)]
pub enum TokenAmount {
    /// Raw u64 value — used when the input has no decimal point (e.g. `1500000`).
    Raw(u64),
    /// Human-readable decimal — will be converted using the mint's `decimals` (e.g. `1.5`).
    Decimal(f64),
}

#[derive(Clone, Debug)]
pub struct TokenFunding {
    pub account: Pubkey,
    pub mint: Option<Pubkey>,
    pub amount: TokenAmount,
}

#[derive(Clone, Debug)]
pub struct AccountDataPatch {
    pub pubkey: Pubkey,
    pub offset: usize,
    pub data: Vec<u8>,
}

pub fn parse_replacement(raw: &str) -> Result<Replacement, String> {
    let (pubkey_str, path_str) = raw
        .split_once('=')
        .ok_or_else(|| "Replacement must be in <PUBKEY>=<PATH> format".to_string())?;
    let pubkey = Pubkey::from_str(pubkey_str)
        .map_err(|err| format!("Failed to parse address `{pubkey_str}`: {err}"))?;
    let path = PathBuf::from(crate::utils::config::expand_tilde(path_str.trim()));
    if !path.exists() {
        return Err(format!("Specified file `{}` does not exist", path.display()));
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "so" | "elf" => Ok(Replacement::Program { program_id: pubkey, so_path: path }),
        "json" => {
            let account = parse_account_json(&path)?;
            Ok(Replacement::Account { pubkey, account, source_path: path })
        }
        _ => Err(format!(
            "Unsupported file extension `.{ext}` for replacement file `{}`. \
             Use .so/.elf for program replacement or .json for account replacement.",
            path.display()
        )),
    }
}

/// JSON structure for deserializing an account file (flat format).
/// Supports the simple `{ "lamports": ..., "data": ... }` format.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountJsonFlat {
    lamports: u64,
    data: AccountDataJson,
    owner: String,
    #[serde(default)]
    executable: bool,
    #[serde(default)]
    rent_epoch: u64,
}

/// JSON structure for deserializing a Solana CLI style account file (nested format).
/// Supports `{ "pubkey": "...", "account": { "lamports": ..., "data": ... } }`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountJsonNested {
    #[allow(dead_code)]
    pubkey: String,
    account: AccountJsonFlat,
}

/// Account data can be either a plain base64 string or a tuple `["base64data", "base64"]`.
#[derive(Deserialize)]
#[serde(untagged)]
enum AccountDataJson {
    Plain(String),
    Tuple(String, String),
}

pub(crate) fn parse_account_json(path: &PathBuf) -> Result<Account, String> {
    use base64::Engine;

    let contents = std::fs::read_to_string(path)
        .map_err(|err| format!("Failed to read account file `{}`: {err}", path.display()))?;

    // Try nested format first (Solana CLI style: { "pubkey": ..., "account": { ... } })
    // then fall back to flat format ({ "lamports": ..., "data": ... })
    let json: AccountJsonFlat = if let Ok(nested) =
        serde_json::from_str::<AccountJsonNested>(&contents)
    {
        nested.account
    } else {
        serde_json::from_str(&contents)
            .map_err(|err| format!("Failed to parse account JSON `{}`: {err}", path.display()))?
    };

    let data_b64 = match &json.data {
        AccountDataJson::Plain(s) => s.clone(),
        AccountDataJson::Tuple(data, _encoding) => data.clone(),
    };

    let data = base64::engine::general_purpose::STANDARD
        .decode(&data_b64)
        .map_err(|err| format!("Failed to decode base64 data in `{}`: {err}", path.display()))?;

    let owner = Pubkey::from_str(&json.owner)
        .map_err(|err| format!("Failed to parse owner `{}`: {err}", json.owner))?;

    Ok(Account {
        lamports: json.lamports,
        data,
        owner,
        executable: json.executable,
        rent_epoch: json.rent_epoch,
    })
}

const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

pub fn parse_funding(raw: &str) -> Result<Funding, String> {
    let (pubkey_str, amount_str) = raw
        .split_once('=')
        .ok_or_else(|| "Funding must be in <PUBKEY>=<AMOUNT> format".to_string())?;
    let pubkey = Pubkey::from_str(pubkey_str)
        .map_err(|err| format!("Failed to parse pubkey `{pubkey_str}`: {err}"))?;
    let trimmed = amount_str.trim();
    let amount_lamports = if trimmed.to_ascii_lowercase().ends_with("sol") {
        let sol_str = &trimmed[..trimmed.len() - 3];
        let sol: f64 = sol_str
            .parse()
            .map_err(|err| format!("Failed to parse SOL amount `{sol_str}`: {err}"))?;
        if sol < 0.0 {
            return Err("Funding amount must be non-negative".to_string());
        }
        (sol * LAMPORTS_PER_SOL as f64).round() as u64
    } else {
        trimmed
            .parse::<u64>()
            .map_err(|err| format!("Failed to parse lamports amount `{trimmed}`: {err}"))?
    };

    Ok(Funding { pubkey, amount_lamports })
}

pub fn parse_token_funding(raw: &str) -> Result<TokenFunding, String> {
    let (target, amount_str) = raw.split_once('=').ok_or_else(|| {
        "Token funding must be in <ACCOUNT>=<AMOUNT> or <ACCOUNT>:<MINT>=<AMOUNT> format"
            .to_string()
    })?;

    let (token_str, mint) = if let Some((token_part, mint_str)) = target.split_once(':') {
        if mint_str.contains(':') {
            return Err(
                "Token funding must be in <ACCOUNT>=<AMOUNT> or <ACCOUNT>:<MINT>=<AMOUNT> format"
                    .to_string(),
            );
        }
        let mint = Pubkey::from_str(mint_str)
            .map_err(|err| format!("Failed to parse mint account `{mint_str}`: {err}"))?;
        (token_part, Some(mint))
    } else {
        (target, None)
    };

    let account = Pubkey::from_str(token_str)
        .map_err(|err| format!("Failed to parse token account `{token_str}`: {err}"))?;

    let trimmed = amount_str.trim();
    let amount = if trimmed.contains('.') {
        // Decimal → human-readable UI amount (will be converted using mint decimals later)
        let decimal: f64 = trimmed
            .parse()
            .map_err(|err| format!("Failed to parse token amount `{trimmed}`: {err}"))?;
        if decimal < 0.0 {
            return Err("Token funding amount must be non-negative".to_string());
        }
        TokenAmount::Decimal(decimal)
    } else {
        // Integer → raw token units
        let raw = trimmed
            .parse::<u64>()
            .map_err(|err| format!("Failed to parse token amount `{trimmed}`: {err}"))?;
        TokenAmount::Raw(raw)
    };

    Ok(TokenFunding { account, mint, amount })
}

pub fn parse_data_patch(raw: &str) -> Result<AccountDataPatch, String> {
    let (pubkey_str, rest) = raw
        .split_once('=')
        .ok_or_else(|| "Data patch must be in <PUBKEY>=<OFFSET>:<HEX_DATA> format".to_string())?;
    let pubkey = Pubkey::from_str(pubkey_str)
        .map_err(|err| format!("Failed to parse address `{pubkey_str}`: {err}"))?;

    let (offset_str, hex_str) = rest.split_once(':').ok_or_else(|| {
        "Data patch value must be in <OFFSET>:<HEX_DATA> format (missing `:`)".to_string()
    })?;

    let offset: usize = offset_str
        .trim()
        .parse()
        .map_err(|err| format!("Failed to parse offset `{offset_str}`: {err}"))?;

    let hex_str = hex_str.trim();
    let hex_str =
        hex_str.strip_prefix("0x").or_else(|| hex_str.strip_prefix("0X")).unwrap_or(hex_str);

    if hex_str.is_empty() {
        return Err("HEX_DATA must not be empty".to_string());
    }
    if hex_str.len() % 2 != 0 {
        return Err(format!(
            "HEX_DATA has odd length {}; expected an even number of hex characters",
            hex_str.len()
        ));
    }

    let data = (0..hex_str.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex_str[i..i + 2], 16)
                .map_err(|err| format!("Invalid hex at position {i}: {err}"))
        })
        .collect::<Result<Vec<u8>, _>>()?;

    Ok(AccountDataPatch { pubkey, offset, data })
}

pub fn parse_timestamp(raw: &str) -> Result<i64, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(
            "Timestamp must not be empty; use Unix seconds or RFC3339 (e.g. 2024-01-01T00:00:00Z)"
                .to_string(),
        );
    }

    if let Ok(unix_seconds) = trimmed.parse::<i64>() {
        return Ok(unix_seconds);
    }

    DateTime::parse_from_rfc3339(trimmed).map(|datetime| datetime.timestamp()).map_err(|_| {
        format!(
            "Invalid timestamp `{trimmed}`. Supported formats: Unix seconds (e.g. 1700000000) \
                 or RFC3339 (e.g. 2024-01-01T00:00:00Z)"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Commands};
    use clap::Parser;

    fn unique_test_file_path(base_dir: &std::path::Path, ext: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after UNIX_EPOCH")
            .as_nanos();
        base_dir.join(format!(".sonar_replace_test_{}_{}.{}", std::process::id(), nanos, ext))
    }

    #[test]
    fn parse_replacement_expands_tilde_path() {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .expect("HOME or USERPROFILE should be set");
        let absolute_path = unique_test_file_path(std::path::Path::new(&home), "so");
        std::fs::write(&absolute_path, b"fake-elf").expect("create replacement file");

        let file_name = absolute_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("temporary filename should be valid UTF-8");
        let program_id = Pubkey::new_unique();
        let input = format!("{program_id}=~/{file_name}");
        let parsed = parse_replacement(&input).expect("parse replacement with ~ path");

        std::fs::remove_file(&absolute_path).ok();

        match parsed {
            Replacement::Program { program_id: parsed_id, so_path } => {
                assert_eq!(parsed_id, program_id);
                assert_eq!(so_path, absolute_path);
            }
            _ => panic!("expected program replacement"),
        }
    }

    #[test]
    fn parse_replacement_accepts_absolute_path() {
        let absolute_path = unique_test_file_path(&std::env::temp_dir(), "so");
        std::fs::write(&absolute_path, b"fake-elf").expect("create replacement file");

        let program_id = Pubkey::new_unique();
        let input = format!("{program_id}={}", absolute_path.display());
        let parsed = parse_replacement(&input).expect("parse replacement with absolute path");

        std::fs::remove_file(&absolute_path).ok();

        match parsed {
            Replacement::Program { program_id: parsed_id, so_path } => {
                assert_eq!(parsed_id, program_id);
                assert_eq!(so_path, absolute_path);
            }
            _ => panic!("expected program replacement"),
        }
    }

    #[test]
    fn parse_replacement_reports_missing_file() {
        let missing_path = unique_test_file_path(&std::env::temp_dir(), "so");
        let program_id = Pubkey::new_unique();
        let input = format!("{program_id}={}", missing_path.display());
        let err = parse_replacement(&input).unwrap_err();
        assert!(err.contains("does not exist"));
    }

    #[test]
    fn parse_token_funding_accepts_valid_input_with_mint() {
        let token = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let input = format!("{token}:{mint}=12345");
        let parsed = parse_token_funding(&input).expect("parses");
        assert_eq!(parsed.account, token);
        assert_eq!(parsed.mint, Some(mint));
        assert!(matches!(parsed.amount, TokenAmount::Raw(12_345)));
    }

    #[test]
    fn parse_token_funding_accepts_valid_input_without_mint() {
        let token = Pubkey::new_unique();
        let input = format!("{token}=99999");
        let parsed = parse_token_funding(&input).expect("parses");
        assert_eq!(parsed.account, token);
        assert_eq!(parsed.mint, None);
        assert!(matches!(parsed.amount, TokenAmount::Raw(99_999)));
    }

    #[test]
    fn parse_token_funding_rejects_missing_equals() {
        let err = parse_token_funding("invalid").unwrap_err();
        assert!(err.contains("<ACCOUNT>"));
    }

    #[test]
    fn parse_token_funding_rejects_extra_colons() {
        let key = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let err = parse_token_funding(&format!("{key}:{mint}:extra=100")).unwrap_err();
        assert!(err.contains("<ACCOUNT>"));
    }

    #[test]
    fn parse_token_funding_rejects_negative_amount() {
        let key = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let err = parse_token_funding(&format!("{key}:{mint}=-1")).unwrap_err();
        assert!(err.contains("Failed to parse token amount"));
    }

    #[test]
    fn parse_token_funding_rejects_negative_amount_without_mint() {
        let key = Pubkey::new_unique();
        let err = parse_token_funding(&format!("{key}=-1")).unwrap_err();
        assert!(err.contains("Failed to parse token amount"));
    }

    #[test]
    fn parse_funding_lamports_default() {
        let key = Pubkey::new_unique();
        let input = format!("{key}=1000000000");
        let parsed = parse_funding(&input).expect("parses");
        assert_eq!(parsed.pubkey, key);
        assert_eq!(parsed.amount_lamports, 1_000_000_000);
    }

    #[test]
    fn parse_funding_sol_suffix_lowercase() {
        let key = Pubkey::new_unique();
        let input = format!("{key}=1.5sol");
        let parsed = parse_funding(&input).expect("parses");
        assert_eq!(parsed.pubkey, key);
        assert_eq!(parsed.amount_lamports, 1_500_000_000);
    }

    #[test]
    fn parse_funding_sol_suffix_uppercase() {
        let key = Pubkey::new_unique();
        let input = format!("{key}=0.1SOL");
        let parsed = parse_funding(&input).expect("parses");
        assert_eq!(parsed.pubkey, key);
        assert_eq!(parsed.amount_lamports, 100_000_000);
    }

    #[test]
    fn parse_funding_sol_suffix_mixed_case() {
        let key = Pubkey::new_unique();
        let input = format!("{key}=2Sol");
        let parsed = parse_funding(&input).expect("parses");
        assert_eq!(parsed.pubkey, key);
        assert_eq!(parsed.amount_lamports, 2_000_000_000);
    }

    #[test]
    fn parse_funding_rejects_missing_equals() {
        let err = parse_funding("invalid").unwrap_err();
        assert!(err.contains("<PUBKEY>=<AMOUNT>"));
    }

    #[test]
    fn parse_funding_rejects_negative_sol() {
        let key = Pubkey::new_unique();
        let err = parse_funding(&format!("{key}=-1sol")).unwrap_err();
        assert!(err.contains("non-negative"));
    }

    #[test]
    fn parse_funding_rejects_invalid_lamports() {
        let key = Pubkey::new_unique();
        let err = parse_funding(&format!("{key}=abc")).unwrap_err();
        assert!(err.contains("Failed to parse lamports amount"));
    }

    #[test]
    fn parse_funding_zero_lamports() {
        let key = Pubkey::new_unique();
        let input = format!("{key}=0");
        let parsed = parse_funding(&input).expect("parses");
        assert_eq!(parsed.amount_lamports, 0);
    }

    #[test]
    fn parse_funding_zero_sol() {
        let key = Pubkey::new_unique();
        let input = format!("{key}=0sol");
        let parsed = parse_funding(&input).expect("parses");
        assert_eq!(parsed.amount_lamports, 0);
    }

    #[test]
    fn parse_data_patch_basic() {
        let key = Pubkey::new_unique();
        let input = format!("{key}=16:deadbeef");
        let parsed = parse_data_patch(&input).expect("parses");
        assert_eq!(parsed.pubkey, key);
        assert_eq!(parsed.offset, 16);
        assert_eq!(parsed.data, vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn parse_data_patch_with_0x_prefix() {
        let key = Pubkey::new_unique();
        let input = format!("{key}=0:0xaabb");
        let parsed = parse_data_patch(&input).expect("parses");
        assert_eq!(parsed.offset, 0);
        assert_eq!(parsed.data, vec![0xaa, 0xbb]);
    }

    #[test]
    fn parse_data_patch_with_0x_uppercase_prefix() {
        let key = Pubkey::new_unique();
        let input = format!("{key}=8:0Xff00");
        let parsed = parse_data_patch(&input).expect("parses");
        assert_eq!(parsed.offset, 8);
        assert_eq!(parsed.data, vec![0xff, 0x00]);
    }

    #[test]
    fn parse_data_patch_rejects_missing_equals() {
        let err = parse_data_patch("invalid").unwrap_err();
        assert!(err.contains("<PUBKEY>=<OFFSET>:<HEX_DATA>"));
    }

    #[test]
    fn parse_data_patch_rejects_missing_colon() {
        let key = Pubkey::new_unique();
        let err = parse_data_patch(&format!("{key}=16deadbeef")).unwrap_err();
        assert!(err.contains("missing `:`"));
    }

    #[test]
    fn parse_data_patch_rejects_empty_hex() {
        let key = Pubkey::new_unique();
        let err = parse_data_patch(&format!("{key}=0:")).unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn parse_data_patch_rejects_odd_hex() {
        let key = Pubkey::new_unique();
        let err = parse_data_patch(&format!("{key}=0:abc")).unwrap_err();
        assert!(err.contains("odd length"));
    }

    #[test]
    fn parse_data_patch_rejects_invalid_hex() {
        let key = Pubkey::new_unique();
        let err = parse_data_patch(&format!("{key}=0:zzzz")).unwrap_err();
        assert!(err.contains("Invalid hex"));
    }

    #[test]
    fn parse_timestamp_accepts_unix_seconds() {
        let parsed = parse_timestamp("1700000000").expect("parses unix timestamp");
        assert_eq!(parsed, 1_700_000_000);
    }

    #[test]
    fn parse_timestamp_accepts_rfc3339() {
        let parsed = parse_timestamp("2024-01-01T00:00:00Z").expect("parses rfc3339 timestamp");
        assert_eq!(parsed, 1_704_067_200);
    }

    #[test]
    fn parse_timestamp_rejects_invalid_format() {
        let err = parse_timestamp("not-a-timestamp").unwrap_err();
        assert!(err.contains("Supported formats"));
    }

    // --- TokenAmount::Decimal tests ---

    #[test]
    fn parse_token_funding_decimal_amount_without_mint() {
        let token = Pubkey::new_unique();
        let input = format!("{token}=1.5");
        let parsed = parse_token_funding(&input).expect("parses");
        assert_eq!(parsed.account, token);
        assert_eq!(parsed.mint, None);
        match parsed.amount {
            TokenAmount::Decimal(v) => assert!((v - 1.5).abs() < f64::EPSILON),
            _ => panic!("expected Decimal variant"),
        }
    }

    #[test]
    fn parse_token_funding_decimal_amount_with_mint() {
        let token = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let input = format!("{token}:{mint}=0.001");
        let parsed = parse_token_funding(&input).expect("parses");
        assert_eq!(parsed.account, token);
        assert_eq!(parsed.mint, Some(mint));
        match parsed.amount {
            TokenAmount::Decimal(v) => assert!((v - 0.001).abs() < f64::EPSILON),
            _ => panic!("expected Decimal variant"),
        }
    }

    #[test]
    fn parse_token_funding_decimal_zero() {
        let token = Pubkey::new_unique();
        let input = format!("{token}=0.0");
        let parsed = parse_token_funding(&input).expect("parses");
        match parsed.amount {
            TokenAmount::Decimal(v) => assert!((v - 0.0).abs() < f64::EPSILON),
            _ => panic!("expected Decimal variant"),
        }
    }

    #[test]
    fn parse_token_funding_integer_stays_raw() {
        let token = Pubkey::new_unique();
        let input = format!("{token}=1000000");
        let parsed = parse_token_funding(&input).expect("parses");
        assert!(matches!(parsed.amount, TokenAmount::Raw(1_000_000)));
    }

    #[test]
    fn parse_token_funding_rejects_negative_decimal() {
        let key = Pubkey::new_unique();
        let err = parse_token_funding(&format!("{key}=-1.5")).unwrap_err();
        assert!(err.contains("non-negative"));
    }

    #[test]
    fn simulate_accepts_patch_account_data_flag() {
        let patch = "key1=0:deadbeef";
        let cli = Cli::try_parse_from([
            "sonar",
            "simulate",
            "1111111111111111111111111111111111111111111111111111111111111111111111111111111111111",
            "--patch-account-data",
            patch,
        ])
        .expect("should parse --patch-account-data");

        let Commands::Simulate(args) = cli.command else {
            panic!("expected simulate subcommand");
        };
        assert_eq!(args.data_patches, vec![patch.to_string()]);
    }

    #[test]
    fn simulate_accepts_patch_account_data_short_flag_multiple_times() {
        let patch1 = "key1=0:aabb";
        let patch2 = "key2=4:ccdd";
        let cli = Cli::try_parse_from([
            "sonar",
            "simulate",
            "1111111111111111111111111111111111111111111111111111111111111111111111111111111111111",
            "-p",
            patch1,
            "-p",
            patch2,
        ])
        .expect("should parse -p multiple times");

        let Commands::Simulate(args) = cli.command else {
            panic!("expected simulate subcommand");
        };
        assert_eq!(args.data_patches, vec![patch1.to_string(), patch2.to_string()]);
    }

    #[test]
    fn simulate_rejects_removed_patch_data_flag() {
        let result = Cli::try_parse_from([
            "sonar",
            "simulate",
            "1111111111111111111111111111111111111111111111111111111111111111111111111111111111111",
            "--patch-data",
            "key1=0:deadbeef",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn transaction_input_parses_without_input_kind_flag() {
        let cli = Cli::try_parse_from([
            "sonar",
            "simulate",
            "1111111111111111111111111111111111111111111111111111111111111111111111111111111111111",
        ])
        .expect("should parse without --input-kind");

        let Commands::Simulate(args) = cli.command else {
            panic!("expected simulate subcommand");
        };

        assert_eq!(args.transaction.tx.len(), 1);
    }

    #[test]
    fn transaction_input_rejects_removed_input_kind_flag() {
        let result = Cli::try_parse_from([
            "sonar",
            "decode",
            "3PtGYH77LhhQqTXP4SmDVJ85hmDieWsgXCUbn14v7gYyVYPjZzygUQhTk3bSTYnfA48vCM1rmWY7zWL3j1EVKmEy",
            "--input-kind",
            "signature",
        ]);
        assert!(result.is_err());
    }
}
