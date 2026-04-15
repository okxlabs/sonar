//! Simulate command arguments and related types.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use chrono::DateTime;
use clap::Args;
use solana_account::Account;
use solana_pubkey::Pubkey;

use super::RpcArgs;

const HELP_HEADING_INPUT_RPC: &str = "Input & RPC";
const HELP_HEADING_STATE_PREPARATION: &str = "State Preparation";
const HELP_HEADING_SIMULATION_CONTROLS: &str = "Simulation Controls";
const HELP_HEADING_OUTPUT_DEBUG: &str = "Output & Debug";

#[derive(Args, Debug)]
pub struct SimulateArgs {
    #[command(flatten, next_help_heading = HELP_HEADING_INPUT_RPC)]
    pub transaction: TransactionInputArgs,
    #[command(flatten, next_help_heading = HELP_HEADING_INPUT_RPC)]
    pub rpc: RpcArgs,
    /// Override an on-chain program or account with a local file.
    /// Format: <PUBKEY>=<PATH> (.so/.elf for programs, .json for accounts)
    #[arg(
        short = 'O',
        short_alias = 'R',
        long = "override",
        alias = "replace",
        help_heading = HELP_HEADING_STATE_PREPARATION,
        value_name = "MAPPING",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub overrides: Vec<String>,
    /// Fund a system account with SOL. Format: <PUBKEY>=<LAMPORTS> or <PUBKEY>=<AMOUNT>sol
    #[arg(
        short = 'f',
        long = "fund-sol",
        help_heading = HELP_HEADING_STATE_PREPARATION,
        value_name = "FUNDING",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub fundings: Vec<String>,
    /// Fund a token account.
    /// Format: <ACCOUNT>=<AMOUNT>, <ACCOUNT>:<MINT>=<AMOUNT>,
    /// or <ACCOUNT>:<MINT>:<OWNER>=<AMOUNT> (mint/owner auto-detected if account exists on-chain).
    /// Owner is required when creating a new account that doesn't exist on-chain.
    /// Integer amounts are treated as raw token units; decimal amounts (e.g. 1.5) are
    /// converted using the mint's decimals (e.g. 1.5 with 6 decimals -> 1500000).
    #[arg(
        long = "fund-token",
        help_heading = HELP_HEADING_STATE_PREPARATION,
        value_name = "FUNDING",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub token_fundings: Vec<String>,
    /// Patch an account pubkey within a specific instruction.
    /// Format: <IX>.<ACCOUNT>=<NEW_PUBKEY>[:<w|r>] (1-based indices)
    /// Append :w (default) for writable, :r for read-only.
    #[arg(
        short = 'A',
        long = "patch-ix-account",
        help_heading = HELP_HEADING_STATE_PREPARATION,
        value_name = "PATCH",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub ix_account_patches: Vec<String>,
    /// Append an account to the end of a specific instruction's account list.
    /// Format: <IX>=<PUBKEY>[:<w|r>] (1-based instruction index)
    /// Append :w (default) for writable, :r for read-only.
    #[arg(
        long = "append-ix-account",
        help_heading = HELP_HEADING_STATE_PREPARATION,
        value_name = "APPEND",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub ix_account_appends: Vec<String>,
    /// Patch bytes in an instruction's data field before simulation.
    /// Format: <IX>=<OFFSET>:<HEX_DATA> (1-based instruction index)
    /// HEX_DATA may optionally start with 0x.
    #[arg(
        short = 'P',
        long = "patch-ix-data",
        help_heading = HELP_HEADING_STATE_PREPARATION,
        value_name = "PATCH",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub ix_data_patches: Vec<String>,
    /// Patch bytes in an account data field before simulation.
    /// Format: <PUBKEY>=<OFFSET>:<HEX_DATA>
    /// HEX_DATA may optionally start with 0x.
    #[arg(
        short = 'p',
        long = "patch-account-data",
        help_heading = HELP_HEADING_STATE_PREPARATION,
        value_name = "PATCH",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub data_patches: Vec<String>,
    /// Close an account so it does not exist during simulation.
    /// Takes one or more account pubkeys.
    #[arg(
        long = "close-account",
        help_heading = HELP_HEADING_STATE_PREPARATION,
        value_name = "PUBKEY",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub account_closures: Vec<String>,
    /// Enable account caching: load from cache on repeat runs, save to cache on first run.
    /// Cache is stored per-transaction under ~/.sonar/cache/<KEY>/
    #[arg(short = 'c', long, help_heading = HELP_HEADING_STATE_PREPARATION, env = "SONAR_CACHE")]
    pub cache: bool,
    /// Override the cache root directory (default: ~/.sonar/cache).
    /// Implies --cache.
    #[arg(short = 'D', long, help_heading = HELP_HEADING_STATE_PREPARATION, value_name = "DIR")]
    pub cache_dir: Option<PathBuf>,
    /// Force re-fetch all accounts from RPC, overwriting existing cache.
    /// Implies --cache.
    #[arg(short = 'r', long, help_heading = HELP_HEADING_STATE_PREPARATION)]
    pub refresh_cache: bool,
    /// Override the Clock sysvar's unix_timestamp for simulation.
    /// Supports Unix timestamp (e.g. 1700000000) or RFC3339 (e.g. 2024-01-01T00:00:00Z).
    #[arg(
        short = 't',
        long = "timestamp",
        help_heading = HELP_HEADING_SIMULATION_CONTROLS,
        value_name = "TIMESTAMP",
        value_parser = parse_timestamp
    )]
    pub timestamp: Option<i64>,
    /// Override the simulation slot
    #[arg(short = 's', long = "slot", help_heading = HELP_HEADING_SIMULATION_CONTROLS, value_name = "SLOT")]
    pub slot: Option<u64>,
    /// Verify transaction signatures during simulation
    #[arg(long = "check-sig", help_heading = HELP_HEADING_SIMULATION_CONTROLS, env = "SONAR_VERIFY_SIGNATURES")]
    pub verify_signatures: bool,
    /// Directory containing Anchor IDL JSON files
    #[arg(
        long = "idl-dir",
        help_heading = HELP_HEADING_SIMULATION_CONTROLS,
        value_name = "DIR",
        env = "SONAR_IDL_DIR"
    )]
    pub idl_dir: Option<PathBuf>,
    /// Skip auto-fetching missing IDLs from chain
    #[arg(
        long = "no-idl-fetch",
        help_heading = HELP_HEADING_SIMULATION_CONTROLS,
        env = "SONAR_NO_IDL_FETCH"
    )]
    pub no_idl_fetch: bool,
    /// Always print raw instruction data, even when parser succeeds
    #[arg(long = "raw-ix-data", help_heading = HELP_HEADING_OUTPUT_DEBUG, env = "SONAR_RAW_IX_DATA")]
    pub ix_data: bool,
    /// Print raw program logs instead of structured execution trace
    #[arg(short = 'l', long = "raw-log", help_heading = HELP_HEADING_OUTPUT_DEBUG, env = "SONAR_RAW_LOG")]
    pub raw_log: bool,
    /// Show detailed instruction information (accounts, parsed fields, inner instructions)
    #[arg(short = 'd', long = "show-ix-detail", help_heading = HELP_HEADING_OUTPUT_DEBUG, env = "SONAR_SHOW_IX_DETAIL")]
    pub show_ix_detail: bool,
    /// Show SOL and token balance changes after simulation
    #[arg(
        short = 'b',
        long = "show-balance-change",
        help_heading = HELP_HEADING_OUTPUT_DEBUG,
        env = "SONAR_SHOW_BALANCE_CHANGE"
    )]
    pub show_balance_change: bool,
}

#[derive(Args, Debug, Clone)]
pub struct TransactionInputArgs {
    /// Raw transaction (Base58/Base64) or transaction signature.
    /// Omit to read from stdin (when stdin is not a TTY).
    /// Pass multiple values for bundle mode.
    #[arg(value_name = "TX", required = false)]
    pub tx: Vec<String>,
}

pub use sonar_sim::internals::{
    AccountDataPatch, AccountOverride, InstructionAccountAppend, InstructionAccountPatch,
    InstructionDataPatch, SolFunding, TokenAmount, TokenFunding,
};

/// Parse a pubkey string with an optional `:w` (writable) or `:r` (read-only)
/// suffix.  Defaults to writable when no suffix is present.
fn parse_pubkey_with_writable_flag(value_str: &str) -> Result<(Pubkey, bool), String> {
    let value_str = value_str.trim();
    let (pubkey_str, writable) = if let Some(pk) = value_str.strip_suffix(":w") {
        (pk, true)
    } else if let Some(pk) = value_str.strip_suffix(":r") {
        (pk, false)
    } else {
        (value_str, true)
    };
    let pubkey = Pubkey::from_str(pubkey_str)
        .map_err(|err| format!("Failed to parse pubkey `{pubkey_str}`: {err}"))?;
    Ok((pubkey, writable))
}

pub fn parse_override(raw: &str) -> Result<AccountOverride, String> {
    let (pubkey_str, path_str) = raw
        .split_once('=')
        .ok_or_else(|| "Override must be in <PUBKEY>=<PATH> format".to_string())?;
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
        "so" | "elf" => Ok(AccountOverride::Program { program_id: pubkey, so_path: path }),
        "json" => {
            let account = parse_account_json(&path)?;
            Ok(AccountOverride::Account { pubkey, account, source_path: path })
        }
        _ => Err(format!(
            "Unsupported file extension `.{ext}` for override file `{}`. \
             Use .so/.elf for program override or .json for account override.",
            path.display()
        )),
    }
}

pub(crate) fn parse_account_json(path: &Path) -> Result<Account, String> {
    crate::core::account_file::parse_account_json(path)
}

const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

pub fn parse_funding(raw: &str) -> Result<SolFunding, String> {
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

    Ok(SolFunding { pubkey, amount_lamports })
}

pub fn parse_token_funding(raw: &str) -> Result<TokenFunding, String> {
    let (target, amount_str) = raw.split_once('=').ok_or_else(|| {
        "Token funding must be in <ACCOUNT>=<AMOUNT>, <ACCOUNT>:<MINT>=<AMOUNT>, \
         or <ACCOUNT>:<MINT>:<OWNER>=<AMOUNT> format"
            .to_string()
    })?;

    let parts: Vec<&str> = target.split(':').collect();
    let (token_str, mint, owner) = match parts.len() {
        1 => (parts[0], None, None),
        2 => {
            let mint = Pubkey::from_str(parts[1])
                .map_err(|err| format!("Failed to parse mint account `{}`: {err}", parts[1]))?;
            (parts[0], Some(mint), None)
        }
        3 => {
            let mint = Pubkey::from_str(parts[1])
                .map_err(|err| format!("Failed to parse mint account `{}`: {err}", parts[1]))?;
            let owner = Pubkey::from_str(parts[2])
                .map_err(|err| format!("Failed to parse owner `{}`: {err}", parts[2]))?;
            (parts[0], Some(mint), Some(owner))
        }
        _ => {
            return Err("Token funding must be in <ACCOUNT>=<AMOUNT>, <ACCOUNT>:<MINT>=<AMOUNT>, \
                 or <ACCOUNT>:<MINT>:<OWNER>=<AMOUNT> format"
                .to_string());
        }
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

    Ok(TokenFunding { account, mint, owner, amount })
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

    let data = crate::utils::parse_hex_data(hex_str)?;

    Ok(AccountDataPatch { pubkey, offset, data })
}

pub fn parse_ix_account_patch(raw: &str) -> Result<InstructionAccountPatch, String> {
    let (position_str, value_str) = raw.split_once('=').ok_or_else(|| {
        "Patch must be in <IX>.<ACCOUNT>=<NEW_PUBKEY>[:<w|r>] format (1-based indices)".to_string()
    })?;
    let (ix_str, pos_str) = position_str.split_once('.').ok_or_else(|| {
        "Patch position must be in <IX>.<ACCOUNT> format (missing `.`)".to_string()
    })?;
    let ix_1based: usize = ix_str
        .trim()
        .parse()
        .map_err(|err| format!("Failed to parse instruction index `{ix_str}`: {err}"))?;
    if ix_1based == 0 {
        return Err("Instruction index is 1-based and must be >= 1".to_string());
    }
    let pos_1based: usize = pos_str
        .trim()
        .parse()
        .map_err(|err| format!("Failed to parse account position `{pos_str}`: {err}"))?;
    if pos_1based == 0 {
        return Err("Account position is 1-based and must be >= 1".to_string());
    }

    let (new_pubkey, writable) = parse_pubkey_with_writable_flag(value_str)?;
    Ok(InstructionAccountPatch {
        instruction_index: ix_1based - 1,
        account_position: pos_1based - 1,
        new_pubkey,
        writable,
    })
}

pub fn parse_ix_account_append(raw: &str) -> Result<InstructionAccountAppend, String> {
    let (ix_str, value_str) = raw.split_once('=').ok_or_else(|| {
        "Append must be in <IX>=<PUBKEY>[:<w|r>] format (1-based instruction index)".to_string()
    })?;
    let ix_1based: usize = ix_str
        .trim()
        .parse()
        .map_err(|err| format!("Failed to parse instruction index `{ix_str}`: {err}"))?;
    if ix_1based == 0 {
        return Err("Instruction index is 1-based and must be >= 1".to_string());
    }

    let (new_pubkey, writable) = parse_pubkey_with_writable_flag(value_str)?;
    Ok(InstructionAccountAppend { instruction_index: ix_1based - 1, new_pubkey, writable })
}

pub fn parse_ix_data_patch(raw: &str) -> Result<InstructionDataPatch, String> {
    let (ix_str, rest) = raw.split_once('=').ok_or_else(|| {
        "Instruction data patch must be in <IX>=<OFFSET>:<HEX_DATA> format (1-based index)"
            .to_string()
    })?;
    let ix_1based: usize = ix_str
        .trim()
        .parse()
        .map_err(|err| format!("Failed to parse instruction index `{ix_str}`: {err}"))?;
    if ix_1based == 0 {
        return Err("Instruction index is 1-based and must be >= 1".to_string());
    }

    let (offset_str, hex_str) = rest.split_once(':').ok_or_else(|| {
        "Instruction data patch value must be in <OFFSET>:<HEX_DATA> format (missing `:`)"
            .to_string()
    })?;
    let offset: usize = offset_str
        .trim()
        .parse()
        .map_err(|err| format!("Failed to parse offset `{offset_str}`: {err}"))?;

    let data = crate::utils::parse_hex_data(hex_str)?;

    Ok(InstructionDataPatch { instruction_index: ix_1based - 1, offset, data })
}

pub fn parse_close_account(raw: &str) -> Result<Pubkey, String> {
    let trimmed = raw.trim();
    Pubkey::from_str(trimmed)
        .map_err(|err| format!("Failed to parse --close-account pubkey `{trimmed}`: {err}"))
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
    fn parse_override_expands_tilde_path() {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .expect("HOME or USERPROFILE should be set");
        let absolute_path = unique_test_file_path(std::path::Path::new(&home), "so");
        std::fs::write(&absolute_path, b"fake-elf").expect("create override file");

        let file_name = absolute_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("temporary filename should be valid UTF-8");
        let program_id = Pubkey::new_unique();
        let input = format!("{program_id}=~/{file_name}");
        let parsed = parse_override(&input).expect("parse override with ~ path");

        std::fs::remove_file(&absolute_path).ok();

        match parsed {
            AccountOverride::Program { program_id: parsed_id, so_path } => {
                assert_eq!(parsed_id, program_id);
                assert_eq!(so_path, absolute_path);
            }
            _ => panic!("expected program override"),
        }
    }

    #[test]
    fn parse_override_accepts_absolute_path() {
        let absolute_path = unique_test_file_path(&std::env::temp_dir(), "so");
        std::fs::write(&absolute_path, b"fake-elf").expect("create override file");

        let program_id = Pubkey::new_unique();
        let input = format!("{program_id}={}", absolute_path.display());
        let parsed = parse_override(&input).expect("parse override with absolute path");

        std::fs::remove_file(&absolute_path).ok();

        match parsed {
            AccountOverride::Program { program_id: parsed_id, so_path } => {
                assert_eq!(parsed_id, program_id);
                assert_eq!(so_path, absolute_path);
            }
            _ => panic!("expected program override"),
        }
    }

    #[test]
    fn parse_override_reports_missing_file() {
        let missing_path = unique_test_file_path(&std::env::temp_dir(), "so");
        let program_id = Pubkey::new_unique();
        let input = format!("{program_id}={}", missing_path.display());
        let err = parse_override(&input).unwrap_err();
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
    fn parse_token_funding_accepts_owner() {
        let token = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let input = format!("{token}:{mint}:{owner}=12345");
        let parsed = parse_token_funding(&input).expect("parses");
        assert_eq!(parsed.account, token);
        assert_eq!(parsed.mint, Some(mint));
        assert_eq!(parsed.owner, Some(owner));
        assert!(matches!(parsed.amount, TokenAmount::Raw(12_345)));
    }

    #[test]
    fn parse_token_funding_with_mint_no_owner() {
        let token = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let input = format!("{token}:{mint}=100");
        let parsed = parse_token_funding(&input).expect("parses");
        assert_eq!(parsed.owner, None);
    }

    #[test]
    fn parse_token_funding_without_mint_has_no_owner() {
        let token = Pubkey::new_unique();
        let input = format!("{token}=100");
        let parsed = parse_token_funding(&input).expect("parses");
        assert_eq!(parsed.mint, None);
        assert_eq!(parsed.owner, None);
    }

    #[test]
    fn parse_token_funding_rejects_four_colons() {
        let keys: Vec<_> = (0..4).map(|_| Pubkey::new_unique()).collect();
        let err =
            parse_token_funding(&format!("{}:{}:{}:{}=100", keys[0], keys[1], keys[2], keys[3]))
                .unwrap_err();
        assert!(err.contains("<ACCOUNT>"));
    }

    #[test]
    fn parse_token_funding_rejects_extra_colons() {
        let key = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let err = parse_token_funding(&format!("{key}:{mint}:extra=100")).unwrap_err();
        assert!(err.contains("Failed to parse owner"));
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

        let Some(Commands::Simulate(args)) = cli.command else {
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

        let Some(Commands::Simulate(args)) = cli.command else {
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

        let Some(Commands::Simulate(args)) = cli.command else {
            panic!("expected simulate subcommand");
        };

        assert_eq!(args.transaction.tx.len(), 1);
    }

    #[test]
    fn simulate_parses_with_omitted_tx_for_stdin() {
        let cli = Cli::try_parse_from([
            "sonar",
            "simulate",
            "--rpc-url",
            "https://api.mainnet-beta.solana.com",
        ])
        .expect("should parse with omitted TX for stdin");

        let Some(Commands::Simulate(args)) = cli.command else {
            panic!("expected simulate subcommand");
        };

        assert!(args.transaction.tx.is_empty());
    }

    #[test]
    fn decode_parses_with_omitted_tx_for_stdin() {
        let cli = Cli::try_parse_from([
            "sonar",
            "decode",
            "--rpc-url",
            "https://api.mainnet-beta.solana.com",
        ])
        .expect("should parse with omitted TX for stdin");

        let Some(Commands::Decode(args)) = cli.command else {
            panic!("expected decode subcommand");
        };

        assert!(args.transaction.tx.is_empty());
    }

    #[test]
    fn decode_parses_cache_control_flags() {
        let cli = Cli::try_parse_from([
            "sonar",
            "decode",
            "3PtGYH77LhhQqTXP4SmDVJ85hmDieWsgXCUbn14v7gYyVYPjZzygUQhTk3bSTYnfA48vCM1rmWY7zWL3j1EVKmEy",
            "--rpc-url",
            "https://api.mainnet-beta.solana.com",
            "--no-cache",
            "--cache-dir",
            "/tmp/sonar-decode-cache",
        ])
        .expect("should parse decode cache control flags");

        let Some(Commands::Decode(args)) = cli.command else {
            panic!("expected decode subcommand");
        };

        assert!(args.no_cache);
        assert_eq!(
            args.cache_dir.as_ref().map(|p| p.to_string_lossy().to_string()),
            Some("/tmp/sonar-decode-cache".to_string())
        );
        assert!(!args.refresh_cache);
    }

    #[test]
    fn decode_parses_refresh_cache_flag() {
        let cli = Cli::try_parse_from([
            "sonar",
            "decode",
            "3PtGYH77LhhQqTXP4SmDVJ85hmDieWsgXCUbn14v7gYyVYPjZzygUQhTk3bSTYnfA48vCM1rmWY7zWL3j1EVKmEy",
            "--rpc-url",
            "https://api.mainnet-beta.solana.com",
            "--refresh-cache",
        ])
        .expect("should parse decode --refresh-cache");

        let Some(Commands::Decode(args)) = cli.command else {
            panic!("expected decode subcommand");
        };

        assert!(args.refresh_cache);
        assert!(!args.no_cache);
    }

    #[test]
    fn parse_ix_account_patch_valid() {
        let key = Pubkey::new_unique();
        let input = format!("1.3={key}");
        let parsed = parse_ix_account_patch(&input).expect("parses");
        assert_eq!(parsed.instruction_index, 0); // 1-based → 0-based
        assert_eq!(parsed.account_position, 2); // 1-based → 0-based
        assert_eq!(parsed.new_pubkey, key);
    }

    #[test]
    fn parse_ix_account_patch_rejects_missing_equals() {
        let err = parse_ix_account_patch("1.2").unwrap_err();
        assert!(err.contains("format"));
    }

    #[test]
    fn parse_ix_account_patch_rejects_missing_dot() {
        let key = Pubkey::new_unique();
        let err = parse_ix_account_patch(&format!("12={key}")).unwrap_err();
        assert!(err.contains("missing `.`"));
    }

    #[test]
    fn parse_ix_account_patch_rejects_invalid_index() {
        let key = Pubkey::new_unique();
        let err = parse_ix_account_patch(&format!("abc.1={key}")).unwrap_err();
        assert!(err.contains("instruction index"));
    }

    #[test]
    fn parse_ix_account_patch_rejects_zero_ix_index() {
        let key = Pubkey::new_unique();
        let err = parse_ix_account_patch(&format!("0.1={key}")).unwrap_err();
        assert!(err.contains("1-based"));
    }

    #[test]
    fn parse_ix_account_patch_rejects_zero_account_position() {
        let key = Pubkey::new_unique();
        let err = parse_ix_account_patch(&format!("1.0={key}")).unwrap_err();
        assert!(err.contains("1-based"));
    }

    #[test]
    fn parse_ix_account_patch_rejects_invalid_pubkey() {
        let err = parse_ix_account_patch("1.2=notakey").unwrap_err();
        assert!(err.contains("Failed to parse pubkey"));
    }

    #[test]
    fn parse_ix_account_patch_writable_suffix() {
        let key = Pubkey::new_unique();
        let input = format!("1.1={key}:w");
        let parsed = parse_ix_account_patch(&input).expect("parses");
        assert!(parsed.writable);
        assert_eq!(parsed.new_pubkey, key);
    }

    #[test]
    fn parse_ix_account_patch_readonly_suffix() {
        let key = Pubkey::new_unique();
        let input = format!("1.1={key}:r");
        let parsed = parse_ix_account_patch(&input).expect("parses");
        assert!(!parsed.writable);
        assert_eq!(parsed.new_pubkey, key);
    }

    #[test]
    fn parse_ix_account_patch_default_writable() {
        let key = Pubkey::new_unique();
        let input = format!("1.1={key}");
        let parsed = parse_ix_account_patch(&input).expect("parses");
        assert!(parsed.writable);
    }

    #[test]
    fn simulate_accepts_set_ix_account_flag() {
        let key = Pubkey::new_unique();
        let cli = Cli::try_parse_from([
            "sonar",
            "simulate",
            "1111111111111111111111111111111111111111111111111111111111111111111111111111111111111",
            "--patch-ix-account",
            &format!("1.3={key}"),
        ])
        .expect("should parse --set-ix-account");

        let Some(Commands::Simulate(args)) = cli.command else {
            panic!("expected simulate subcommand");
        };
        assert_eq!(args.ix_account_patches.len(), 1);
    }

    #[test]
    fn simulate_accepts_set_ix_account_short_flag_multiple() {
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let cli = Cli::try_parse_from([
            "sonar",
            "simulate",
            "1111111111111111111111111111111111111111111111111111111111111111111111111111111111111",
            "-A",
            &format!("1.3={key1}"),
            "-A",
            &format!("2.1={key2}"),
        ])
        .expect("should parse -A multiple times");

        let Some(Commands::Simulate(args)) = cli.command else {
            panic!("expected simulate subcommand");
        };
        assert_eq!(args.ix_account_patches.len(), 2);
    }

    #[test]
    fn parse_ix_data_patch_basic() {
        let input = "1=8:deadbeef";
        let parsed = parse_ix_data_patch(input).expect("parses");
        assert_eq!(parsed.instruction_index, 0); // 1-based → 0-based
        assert_eq!(parsed.offset, 8);
        assert_eq!(parsed.data, vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn parse_ix_data_patch_with_0x_prefix() {
        let input = "2=0:0xaabb";
        let parsed = parse_ix_data_patch(input).expect("parses");
        assert_eq!(parsed.instruction_index, 1);
        assert_eq!(parsed.offset, 0);
        assert_eq!(parsed.data, vec![0xaa, 0xbb]);
    }

    #[test]
    fn parse_ix_data_patch_rejects_zero_index() {
        let err = parse_ix_data_patch("0=0:aabb").unwrap_err();
        assert!(err.contains("1-based"));
    }

    #[test]
    fn parse_ix_data_patch_rejects_missing_equals() {
        let err = parse_ix_data_patch("1:0:aabb").unwrap_err();
        assert!(err.contains("format"));
    }

    #[test]
    fn parse_ix_data_patch_rejects_missing_colon() {
        let err = parse_ix_data_patch("1=0aabb").unwrap_err();
        assert!(err.contains("missing `:`"));
    }

    #[test]
    fn parse_ix_data_patch_rejects_empty_hex() {
        let err = parse_ix_data_patch("1=0:").unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn parse_ix_data_patch_rejects_odd_hex() {
        let err = parse_ix_data_patch("1=0:abc").unwrap_err();
        assert!(err.contains("odd length"));
    }

    #[test]
    fn simulate_accepts_patch_ix_data_flag() {
        let cli = Cli::try_parse_from([
            "sonar",
            "simulate",
            "1111111111111111111111111111111111111111111111111111111111111111111111111111111111111",
            "--patch-ix-data",
            "1=0:deadbeef",
            "-P",
            "2=4:aabb",
        ])
        .expect("should parse --patch-ix-data and -P");

        let Some(Commands::Simulate(args)) = cli.command else {
            panic!("expected simulate subcommand");
        };
        assert_eq!(args.ix_data_patches.len(), 2);
    }

    #[test]
    fn parse_ix_account_append_valid_writable() {
        let key = Pubkey::new_unique();
        let input = format!("1={key}");
        let parsed = parse_ix_account_append(&input).expect("parses");
        assert_eq!(parsed.instruction_index, 0); // 1-based → 0-based
        assert_eq!(parsed.new_pubkey, key);
        assert!(parsed.writable);
    }

    #[test]
    fn parse_ix_account_append_readonly_suffix() {
        let key = Pubkey::new_unique();
        let input = format!("2={key}:r");
        let parsed = parse_ix_account_append(&input).expect("parses");
        assert_eq!(parsed.instruction_index, 1);
        assert!(!parsed.writable);
    }

    #[test]
    fn parse_ix_account_append_writable_suffix() {
        let key = Pubkey::new_unique();
        let input = format!("1={key}:w");
        let parsed = parse_ix_account_append(&input).expect("parses");
        assert!(parsed.writable);
    }

    #[test]
    fn parse_ix_account_append_rejects_zero_index() {
        let key = Pubkey::new_unique();
        let err = parse_ix_account_append(&format!("0={key}")).unwrap_err();
        assert!(err.contains("1-based"));
    }

    #[test]
    fn parse_ix_account_append_rejects_missing_equals() {
        let err = parse_ix_account_append("1").unwrap_err();
        assert!(err.contains("format"));
    }

    #[test]
    fn parse_ix_account_append_rejects_invalid_pubkey() {
        let err = parse_ix_account_append("1=notakey").unwrap_err();
        assert!(err.contains("Failed to parse pubkey"));
    }

    #[test]
    fn simulate_accepts_append_ix_account_flag() {
        let key = Pubkey::new_unique();
        let cli = Cli::try_parse_from([
            "sonar",
            "simulate",
            "1111111111111111111111111111111111111111111111111111111111111111111111111111111111111",
            "--append-ix-account",
            &format!("1={key}"),
        ])
        .expect("should parse --append-ix-account");

        let Some(Commands::Simulate(args)) = cli.command else {
            panic!("expected simulate subcommand");
        };
        assert_eq!(args.ix_account_appends.len(), 1);
    }

    #[test]
    fn simulate_accepts_append_ix_account_multiple() {
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let cli = Cli::try_parse_from([
            "sonar",
            "simulate",
            "1111111111111111111111111111111111111111111111111111111111111111111111111111111111111",
            "--append-ix-account",
            &format!("1={key1}"),
            "--append-ix-account",
            &format!("2={key2}:r"),
        ])
        .expect("should parse --append-ix-account multiple times");

        let Some(Commands::Simulate(args)) = cli.command else {
            panic!("expected simulate subcommand");
        };
        assert_eq!(args.ix_account_appends.len(), 2);
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
