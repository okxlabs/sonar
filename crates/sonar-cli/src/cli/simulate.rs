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
#[command(after_help = "\
EXAMPLES:
  sonar simulate <TX>                           Single transaction
  sonar simulate <TX1> <TX2>                    Bundle (atomic multi-tx)
  sonar simulate --payer <PUBKEY> --ix <DSL>    Instruction input
  sonar simulate <TX> --fund-sol ALICE=1sol     Fund account before sim
  sonar simulate <TX> --override PROG=prog.so   Override on-chain program")]
pub struct SimulateArgs {
    #[command(flatten, next_help_heading = HELP_HEADING_INPUT_RPC)]
    pub transaction: TransactionInputArgs,
    #[command(flatten, next_help_heading = HELP_HEADING_INPUT_RPC)]
    pub rpc: RpcArgs,
    /// Fee payer for instruction input mode.
    /// Optional: omitting it uses a deterministic placeholder pubkey
    /// (`sha256("sonar-payer")`) auto-funded with 1 SOL for the simulation.
    #[arg(long = "payer", help_heading = HELP_HEADING_INPUT_RPC, value_name = "PUBKEY")]
    pub payer: Option<String>,
    /// Synthesize a transaction from raw instructions and simulate it (conflicts with TX).
    ///
    /// Repeat --ix for multiple instructions in one atomic transaction. Each
    /// value's format is auto-detected:
    ///   DSL    program=<PUBKEY> [accounts=<PUBKEY>[:sw],...] [data=0x..] [encoding=..]
    ///   JSON   starts with `{` or `[`, e.g. {"program":"<PUBKEY>","data":"0x.."}
    ///   @path  read from a file holding JSON or DSL (~ expands, @/dev/stdin pipes)
    ///
    /// Account flags: `s`=signer, `w`=writable; no suffix = read-only non-signer.
    /// JSON accounts take pubkey + is_signer/isSigner + is_writable/isWritable.
    /// data: hex by default (leading 0x optional); set encoding=base64|base58 otherwise.
    #[arg(
        long = "ix",
        alias = "instruction",
        help_heading = HELP_HEADING_INPUT_RPC,
        value_name = "IX|JSON|@PATH",
        // Preserve the aligned DSL/JSON/@path layout above in `--help`;
        // without this clap collapses it into one unreadable paragraph.
        verbatim_doc_comment,
        // Exactly one value per occurrence; repeat the flag for multiple
        // instructions. A greedy `num_args = 1..` would swallow a trailing
        // positional TX into this list, turning the "cannot combine --ix with
        // TX" guard into a confusing DSL parse error.
        num_args = 1,
        // Instruction input and positional TXs are mutually exclusive input
        // modes — clap rejects the combination directly (the `tx` arg id is the
        // flattened `TransactionInputArgs::tx` positional).
        conflicts_with = "tx",
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub instructions: Vec<String>,
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
    /// Format: <IX>.<ACCOUNT>=<NEW_PUBKEY>[:w] (1-based indices)
    /// Append :w for writable; omit the suffix for read-only (the default).
    /// Example: --patch-ix-account 1.3=So11111111111111111111111111111111111111112:w
    /// Ordering: see --insert-ix-account.
    #[arg(
        short = 'A',
        long = "patch-ix-account",
        help_heading = HELP_HEADING_STATE_PREPARATION,
        value_name = "PATCH",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub ix_account_patches: Vec<String>,
    /// Insert an account at a specific position within an instruction's account list.
    /// Format: <IX>.<POSITION>=<PUBKEY>[:w] (1-based indices)
    /// Existing accounts at and after POSITION shift right by one.
    /// To insert at the end, pass POSITION = current_count + 1.
    /// Append :w for writable; omit the suffix for read-only (the default).
    /// Example: --insert-ix-account 1.3=So11111111111111111111111111111111111111112:w
    /// Ordering: instruction-account ops apply in flag order — all --patch-ix-account
    /// first, then --insert-ix-account, then --remove-ix-account; within each flag,
    /// CLI argument order is preserved. Positions are interpreted at apply time, so
    /// to express positions relative to the pre-mutation list, list ops in
    /// descending position order.
    #[arg(
        long = "insert-ix-account",
        help_heading = HELP_HEADING_STATE_PREPARATION,
        value_name = "INSERT",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub ix_account_inserts: Vec<String>,
    /// Remove an account at a specific position from an instruction's account list.
    /// Format: <IX>.<POSITION> (1-based indices)
    /// Subsequent accounts in the same instruction shift left by one.
    /// Example: --remove-ix-account 1.3
    /// Ordering: see --insert-ix-account.
    #[arg(
        long = "remove-ix-account",
        help_heading = HELP_HEADING_STATE_PREPARATION,
        value_name = "REMOVE",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub ix_account_removes: Vec<String>,
    /// Patch bytes in an instruction's data field before simulation.
    /// Format: <IX>=<OFFSET>:<HEX_DATA> (1-based instruction index)
    /// HEX_DATA may optionally start with 0x.
    /// Example: --patch-ix-data 1=8:0xdeadbeef
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
    /// Example: --patch-account-data So11111111111111111111111111111111111111112=16:0xdeadbeef
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
    /// Directory containing Anchor IDL JSON files (matched by `<PROGRAM_ID>.json`
    /// filename or the `address` field declared inside each file)
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
    /// Pass multiple TX values to simulate a bundle (atomic multi-transaction).
    #[arg(value_name = "TX", required = false)]
    pub tx: Vec<String>,
}

pub use sonar_sim::internals::{
    AccountDataPatch, AccountOverride, InstructionAccountOp, InstructionDataPatch, SolFunding,
    TokenAmount, TokenFunding,
};

/// Parse the `<NEW_PUBKEY>[:<flags>]` value of an instruction-account op using
/// the shared `<PUBKEY>[:<s|w>]` grammar (absent suffix = read-only non-signer).
///
/// Signer (`s`) is rejected: `--patch-ix-account` / `--insert-ix-account` only
/// rewrite an account's writability in an existing instruction, whereas
/// declaring a signer requires building the instruction from scratch with
/// `--ix`. Returns the pubkey and whether it is writable.
fn parse_ix_account_op_value(value_str: &str, flag: &str) -> Result<(Pubkey, bool), String> {
    let meta = crate::utils::parse_account_meta_flags(value_str)?;
    if meta.is_signer {
        return Err(format!(
            "{flag} cannot set the signer flag `s`; signer accounts can only be \
             declared with --ix. Use `:w` for writable, or omit the suffix for read-only."
        ));
    }
    Ok((meta.pubkey, meta.is_writable))
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

/// Parse a `<IX>.<POSITION>` head (1-based) optionally followed by `=<rest>`.
/// Returns 0-based indices and the remainder after `=`, if any.
fn parse_ix_pos_prefix(raw: &str) -> Result<(usize, usize, Option<&str>), String> {
    let (head, rest) = match raw.split_once('=') {
        Some((h, r)) => (h, Some(r)),
        None => (raw, None),
    };
    let (ix_str, pos_str) = head.split_once('.').ok_or_else(|| {
        "Expected `<IX>.<POSITION>` format (1-based indices, missing `.`)".to_string()
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
    Ok((ix_1based - 1, pos_1based - 1, rest))
}

pub fn parse_ix_account_patch(raw: &str) -> Result<InstructionAccountOp, String> {
    let (instruction_index, account_position, rest) = parse_ix_pos_prefix(raw)?;
    let value_str = rest
        .ok_or_else(|| "Patch must be in <IX>.<ACCOUNT>=<NEW_PUBKEY>[:w] format".to_string())?;
    let (new_pubkey, writable) = parse_ix_account_op_value(value_str, "--patch-ix-account")?;
    Ok(InstructionAccountOp::Patch { instruction_index, account_position, new_pubkey, writable })
}

pub fn parse_ix_account_insert(raw: &str) -> Result<InstructionAccountOp, String> {
    let (instruction_index, account_position, rest) = parse_ix_pos_prefix(raw)?;
    let value_str = rest
        .ok_or_else(|| "Insert must be in <IX>.<POSITION>=<PUBKEY>[:w] format".to_string())?;
    let (new_pubkey, writable) = parse_ix_account_op_value(value_str, "--insert-ix-account")?;
    Ok(InstructionAccountOp::Insert { instruction_index, account_position, new_pubkey, writable })
}

pub fn parse_ix_account_remove(raw: &str) -> Result<InstructionAccountOp, String> {
    let (instruction_index, account_position, rest) = parse_ix_pos_prefix(raw)?;
    if rest.is_some() {
        return Err("Remove must be in <IX>.<POSITION> format (no `=<value>`)".to_string());
    }
    Ok(InstructionAccountOp::Remove { instruction_index, account_position })
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
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvRestore {
        key: &'static str,
        value: Option<std::ffi::OsString>,
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            match &self.value {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn unique_test_file_path(base_dir: &std::path::Path, ext: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after UNIX_EPOCH")
            .as_nanos();
        base_dir.join(format!(".sonar_replace_test_{}_{}.{}", std::process::id(), nanos, ext))
    }

    #[test]
    fn parse_override_expands_tilde_path() {
        let _guard = ENV_LOCK.lock().expect("env lock should not be poisoned");
        let _home_restore = EnvRestore { key: "HOME", value: std::env::var_os("HOME") };
        let temp_home = tempfile::tempdir().expect("create temp home");
        std::env::set_var("HOME", temp_home.path());

        let absolute_path = unique_test_file_path(temp_home.path(), "so");
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
    fn simulate_accepts_repeated_ix_dsl_flags() {
        let payer = Pubkey::new_unique();
        let program = Pubkey::new_unique();
        let account = Pubkey::new_unique();
        let first = format!("program={program} accounts={account}:sw data=0x01");
        let second = format!("program={program} data=0x02");

        let cli = Cli::try_parse_from([
            "sonar",
            "simulate",
            "--payer",
            &payer.to_string(),
            "--ix",
            &first,
            "--ix",
            &second,
        ])
        .expect("repeated --ix should parse");

        let Some(Commands::Simulate(args)) = cli.command else {
            panic!("expected simulate subcommand");
        };
        assert!(args.transaction.tx.is_empty());
        assert_eq!(args.payer.as_deref(), Some(payer.to_string().as_str()));
        assert_eq!(args.instructions, vec![first, second]);
    }

    #[test]
    fn simulate_ix_conflicts_with_positional_tx() {
        // `--ix` takes exactly one value per occurrence (so a trailing token is
        // not swallowed as a second instruction) and is declared mutually
        // exclusive with the positional TX, so clap rejects the combination
        // directly regardless of flag/positional ordering.
        let program = Pubkey::new_unique();
        let ix = format!("program={program} data=0x01");

        for order in [
            vec!["sonar", "simulate", "--ix", ix.as_str(), "SOME_TX_VALUE"],
            vec!["sonar", "simulate", "SOME_TX_VALUE", "--ix", ix.as_str()],
        ] {
            let err = Cli::try_parse_from(order).expect_err("--ix + TX must conflict");
            assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
        }
    }

    #[test]
    fn simulate_ix_collects_mixed_dsl_and_json_into_one_list() {
        let payer = Pubkey::new_unique();
        let program = Pubkey::new_unique();
        let dsl = format!("program={program} data=0x01");
        let json = format!(r#"{{"program":"{program}","data":"0x02"}}"#);

        // Repeated --ix flags accept a mix of DSL and JSON values; they feed
        // the same list in CLI order with no conflict between formats.
        let cli = Cli::try_parse_from([
            "sonar",
            "simulate",
            "--payer",
            &payer.to_string(),
            "--ix",
            &dsl,
            "--ix",
            &json,
        ])
        .expect("mixed --ix formats should coexist");

        let Some(Commands::Simulate(args)) = cli.command else {
            panic!("expected simulate subcommand");
        };
        assert_eq!(args.instructions, vec![dsl, json]);
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
        let InstructionAccountOp::Patch { instruction_index, account_position, new_pubkey, .. } =
            parsed
        else {
            panic!("expected Patch variant");
        };
        assert_eq!(instruction_index, 0); // 1-based → 0-based
        assert_eq!(account_position, 2); // 1-based → 0-based
        assert_eq!(new_pubkey, key);
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
        assert!(err.contains("Failed to parse account pubkey"));
    }

    #[test]
    fn parse_ix_account_patch_writable_suffix() {
        let key = Pubkey::new_unique();
        let parsed = parse_ix_account_patch(&format!("1.1={key}:w")).expect("parses");
        assert!(matches!(
            parsed,
            InstructionAccountOp::Patch { writable: true, new_pubkey, .. } if new_pubkey == key
        ));
    }

    #[test]
    fn parse_ix_account_patch_default_readonly() {
        // No suffix means read-only, matching the shared --ix account grammar.
        let key = Pubkey::new_unique();
        let parsed = parse_ix_account_patch(&format!("1.1={key}")).expect("parses");
        assert!(matches!(
            parsed,
            InstructionAccountOp::Patch { writable: false, new_pubkey, .. } if new_pubkey == key
        ));
    }

    #[test]
    fn parse_ix_account_patch_rejects_signer_flag() {
        let key = Pubkey::new_unique();
        let err = parse_ix_account_patch(&format!("1.1={key}:s")).unwrap_err();
        assert!(err.contains("signer"), "expected signer rejection, got: {err}");
    }

    #[test]
    fn parse_ix_account_patch_rejects_legacy_readonly_suffix() {
        // `:r` was the old read-only marker; the unified grammar drops it
        // (read-only is now the no-suffix default).
        let key = Pubkey::new_unique();
        let err = parse_ix_account_patch(&format!("1.1={key}:r")).unwrap_err();
        assert!(err.contains("Unknown account flag"), "got: {err}");
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
    fn parse_ix_account_insert_basic() {
        let key = Pubkey::new_unique();
        let parsed = parse_ix_account_insert(&format!("2.3={key}")).unwrap();
        let InstructionAccountOp::Insert {
            instruction_index,
            account_position,
            new_pubkey,
            writable,
        } = parsed
        else {
            panic!("expected Insert variant");
        };
        assert_eq!(instruction_index, 1);
        assert_eq!(account_position, 2);
        assert_eq!(new_pubkey, key);
        // No suffix => read-only, matching the shared --ix account grammar.
        assert!(!writable);
    }

    #[test]
    fn parse_ix_account_insert_writable_suffix() {
        let key = Pubkey::new_unique();
        let parsed = parse_ix_account_insert(&format!("1.1={key}:w")).unwrap();
        assert!(matches!(
            parsed,
            InstructionAccountOp::Insert {
                instruction_index: 0,
                account_position: 0,
                writable: true,
                ..
            }
        ));
    }

    #[test]
    fn parse_ix_account_insert_rejects_signer_flag() {
        let key = Pubkey::new_unique();
        let err = parse_ix_account_insert(&format!("1.1={key}:s")).unwrap_err();
        assert!(err.contains("signer"), "expected signer rejection, got: {err}");
    }

    #[test]
    fn parse_ix_account_insert_rejects_zero_indices() {
        let key = Pubkey::new_unique();
        let err = parse_ix_account_insert(&format!("0.1={key}")).unwrap_err();
        assert!(err.contains("Instruction index"));
        let err = parse_ix_account_insert(&format!("1.0={key}")).unwrap_err();
        assert!(err.contains("Account position"));
    }

    #[test]
    fn parse_ix_account_insert_rejects_invalid_format() {
        let key = Pubkey::new_unique();
        let err = parse_ix_account_insert(&format!("1={key}")).unwrap_err();
        assert!(err.contains("format"));
    }

    #[test]
    fn parse_ix_account_insert_rejects_invalid_pubkey() {
        let err = parse_ix_account_insert("1.1=notakey").unwrap_err();
        assert!(err.contains("Failed to parse account pubkey"));
    }

    #[test]
    fn simulate_accepts_insert_ix_account_flag() {
        let key = Pubkey::new_unique();
        let cli = Cli::try_parse_from([
            "sonar",
            "simulate",
            "1111111111111111111111111111111111111111111111111111111111111111111111111111111111111",
            "--insert-ix-account",
            &format!("1.2={key}"),
        ])
        .expect("should parse --insert-ix-account");

        let Some(Commands::Simulate(args)) = cli.command else {
            panic!("expected simulate subcommand");
        };
        assert_eq!(args.ix_account_inserts.len(), 1);
    }

    #[test]
    fn simulate_accepts_insert_ix_account_multiple() {
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let cli = Cli::try_parse_from([
            "sonar",
            "simulate",
            "1111111111111111111111111111111111111111111111111111111111111111111111111111111111111",
            "--insert-ix-account",
            &format!("1.1={key1}"),
            "--insert-ix-account",
            &format!("2.3={key2}:w"),
        ])
        .expect("should parse --insert-ix-account multiple times");

        let Some(Commands::Simulate(args)) = cli.command else {
            panic!("expected simulate subcommand");
        };
        assert_eq!(args.ix_account_inserts.len(), 2);
    }

    #[test]
    fn parse_ix_account_remove_basic() {
        let parsed = parse_ix_account_remove("2.3").unwrap();
        assert!(matches!(
            parsed,
            InstructionAccountOp::Remove { instruction_index: 1, account_position: 2 }
        ));
    }

    #[test]
    fn parse_ix_account_remove_rejects_zero_indices() {
        let err = parse_ix_account_remove("0.1").unwrap_err();
        assert!(err.contains("Instruction index"));
        let err = parse_ix_account_remove("1.0").unwrap_err();
        assert!(err.contains("Account position"));
    }

    #[test]
    fn parse_ix_account_remove_rejects_invalid_format() {
        let err = parse_ix_account_remove("1=2").unwrap_err();
        assert!(err.contains("format"));
    }

    #[test]
    fn simulate_accepts_remove_ix_account_flag() {
        let cli = Cli::try_parse_from([
            "sonar",
            "simulate",
            "1111111111111111111111111111111111111111111111111111111111111111111111111111111111111",
            "--remove-ix-account",
            "1.2",
        ])
        .expect("should parse --remove-ix-account");

        let Some(Commands::Simulate(args)) = cli.command else {
            panic!("expected simulate subcommand");
        };
        assert_eq!(args.ix_account_removes.len(), 1);
    }

    #[test]
    fn simulate_accepts_remove_ix_account_multiple() {
        let cli = Cli::try_parse_from([
            "sonar",
            "simulate",
            "1111111111111111111111111111111111111111111111111111111111111111111111111111111111111",
            "--remove-ix-account",
            "1.3",
            "--remove-ix-account",
            "1.1",
        ])
        .expect("should parse --remove-ix-account multiple times");

        let Some(Commands::Simulate(args)) = cli.command else {
            panic!("expected simulate subcommand");
        };
        assert_eq!(args.ix_account_removes.len(), 2);
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
