//! Simulate command arguments and related types.

use std::{path::PathBuf, str::FromStr};

use clap::{Args, ValueEnum};
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
    /// Replace an on-chain account for simulation.
    /// Format: <PUBKEY>=<PATH>
    /// .so/.elf files are loaded as programs; .json files are loaded as account data.
    #[arg(
        long = "replace",
        value_name = "MAPPING",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub replacements: Vec<String>,
    /// Fund a system account, format: <PUBKEY>=<LAMPORTS> or <PUBKEY>=<AMOUNT>sol
    #[arg(
        long = "fund-sol",
        value_name = "FUNDING",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub fundings: Vec<String>,
    /// Fund a token account with raw token amount.
    /// Format: <TOKEN_ACCOUNT>=<AMOUNT_RAW> (mint auto-detected from on-chain data)
    /// or <TOKEN_ACCOUNT>:<MINT>=<AMOUNT_RAW> (mint required if account does not exist on-chain)
    #[arg(
        long = "fund-token",
        value_name = "FUNDING",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub token_fundings: Vec<String>,
    /// Always print raw instruction data, even when parser succeeds
    #[arg(long = "raw-ix-data")]
    pub ix_data: bool,
    /// Verify transaction signatures during simulation
    #[arg(long = "check-sig")]
    pub verify_signatures: bool,
    /// Directory containing Anchor IDLs; omit to disable IDL parsing
    #[arg(long = "idl-path", value_name = "PATH")]
    pub idl_path: Option<PathBuf>,
    /// Show SOL and token balance changes after simulation
    #[arg(long = "show-balance-change")]
    pub show_balance_change: bool,
    /// Print raw program logs instead of structured execution trace
    #[arg(long = "show-raw-log")]
    pub show_raw_log: bool,
    /// Show detailed instruction information (accounts, parsed fields, inner instructions)
    #[arg(long = "show-ix-detail")]
    pub show_ix_detail: bool,
    /// Override the Clock sysvar's unix_timestamp for simulation
    #[arg(long = "timestamp", value_name = "UNIX_TIMESTAMP")]
    pub timestamp: Option<i64>,
    /// Override the simulation slot (warp SVM clock to this slot)
    #[arg(long = "slot", value_name = "SLOT")]
    pub slot: Option<u64>,
    /// Patch bytes in an account's data field before simulation.
    /// Format: <PUBKEY>=<OFFSET>:<HEX_DATA>
    /// HEX_DATA may optionally start with 0x.
    #[arg(
        long = "patch-data",
        value_name = "PATCH",
        num_args = 1..,
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub data_patches: Vec<String>,
}

#[derive(Args, Debug, Clone)]
pub struct TransactionInputArgs {
    /// Raw transaction string (Base58/Base64) or transaction signature.
    /// Multiple transactions for bundle simulation can be provided as separate arguments.
    /// Mutually exclusive with --tx-file
    #[arg(value_name = "TX", conflicts_with = "tx_file")]
    pub tx: Vec<String>,
    /// File path containing raw transaction, mutually exclusive with positional TX args
    #[arg(long = "tx-file", value_name = "PATH")]
    pub tx_file: Option<PathBuf>,
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub output: OutputFormat,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
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

#[derive(Clone, Debug)]
pub struct TokenFunding {
    pub account: Pubkey,
    pub mint: Option<Pubkey>,
    pub amount_raw: u64,
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
    let path = PathBuf::from(path_str.trim());
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

/// JSON structure for deserializing an account file.
/// Supports Solana CLI compatible format.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountJson {
    lamports: u64,
    data: AccountDataJson,
    owner: String,
    #[serde(default)]
    executable: bool,
    #[serde(default)]
    rent_epoch: u64,
}

/// Account data can be either a plain base64 string or a tuple `["base64data", "base64"]`.
#[derive(Deserialize)]
#[serde(untagged)]
enum AccountDataJson {
    Plain(String),
    Tuple(String, String),
}

fn parse_account_json(path: &PathBuf) -> Result<Account, String> {
    use base64::Engine;

    let contents = std::fs::read_to_string(path)
        .map_err(|err| format!("Failed to read account file `{}`: {err}", path.display()))?;
    let json: AccountJson = serde_json::from_str(&contents)
        .map_err(|err| format!("Failed to parse account JSON `{}`: {err}", path.display()))?;

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
    let amount_raw = amount_str
        .trim()
        .parse::<u64>()
        .map_err(|err| format!("Failed to parse token amount `{amount_str}`: {err}"))?;

    Ok(TokenFunding { account, mint, amount_raw })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_token_funding_accepts_valid_input_with_mint() {
        let token = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let input = format!("{token}:{mint}=12345");
        let parsed = parse_token_funding(&input).expect("parses");
        assert_eq!(parsed.account, token);
        assert_eq!(parsed.mint, Some(mint));
        assert_eq!(parsed.amount_raw, 12_345);
    }

    #[test]
    fn parse_token_funding_accepts_valid_input_without_mint() {
        let token = Pubkey::new_unique();
        let input = format!("{token}=99999");
        let parsed = parse_token_funding(&input).expect("parses");
        assert_eq!(parsed.account, token);
        assert_eq!(parsed.mint, None);
        assert_eq!(parsed.amount_raw, 99_999);
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
}
