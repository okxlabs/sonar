//! Simulate command arguments and related types.

use std::{path::PathBuf, str::FromStr};

use clap::{Args, ValueEnum};
use solana_pubkey::Pubkey;

#[derive(Args, Debug)]
pub struct SimulateArgs {
    #[command(flatten)]
    pub transaction: TransactionInputArgs,
    /// Solana RPC node URL
    #[arg(long = "rpc-url", default_value = "https://api.mainnet-beta.solana.com")]
    pub rpc_url: String,
    /// Custom program replacement, format: <PROGRAM_ID>=<PATH_TO_ELF_OR_SO>
    #[arg(
        long = "replace",
        value_name = "MAPPING",
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub replacements: Vec<String>,
    /// Fund a system account with SOL, format: <PUBKEY>=<AMOUNT_IN_SOL>
    #[arg(
        long = "fund-sol",
        value_name = "FUNDING",
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub fundings: Vec<String>,
    /// Fund a token account with raw token amount, format: <TOKEN_ACCOUNT>:<MINT_ACCOUNT>:<AMOUNT_RAW>
    #[arg(
        long = "fund-token",
        value_name = "FUNDING",
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub token_fundings: Vec<String>,
    /// Parse transaction only, skip simulation
    #[arg(long = "parse-only")]
    pub parse_only: bool,
    /// Always print raw instruction data, even when parser succeeds
    #[arg(long = "ix-data")]
    pub ix_data: bool,
    /// Verify transaction signatures during simulation
    #[arg(long = "check-sig")]
    pub verify_signatures: bool,
    /// Directory containing Anchor IDLs; omit to disable IDL parsing
    #[arg(long = "idl-path", value_name = "PATH")]
    pub idl_path: Option<PathBuf>,
    /// Show SOL and token balance changes after simulation
    #[arg(long = "balance-change")]
    pub balance_change: bool,
}

#[derive(Args, Debug, Clone)]
pub struct TransactionInputArgs {
    /// Raw transaction string (Base58/Base64) or transaction signature.
    /// For bundle simulation, use comma-separated values: tx1,tx2,tx3
    /// Mutually exclusive with --tx-file
    #[arg(short = 't', long, conflicts_with = "tx_file", value_name = "STRING")]
    pub tx: Option<String>,
    /// File path containing raw transaction, mutually exclusive with --tx
    #[arg(long = "tx-file", value_name = "PATH", conflicts_with = "tx")]
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
pub struct ProgramReplacement {
    pub program_id: Pubkey,
    pub so_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct Funding {
    pub pubkey: Pubkey,
    pub amount_sol: f64,
}

#[derive(Clone, Debug)]
pub struct TokenFunding {
    pub account: Pubkey,
    pub mint: Pubkey,
    pub amount_raw: u64,
}

pub fn parse_program_replacement(raw: &str) -> Result<ProgramReplacement, String> {
    let (program_str, path_str) = raw
        .split_once('=')
        .ok_or_else(|| "Replacement must be in <PROGRAM_ID>=<PATH> format".to_string())?;
    let program_id = Pubkey::from_str(program_str)
        .map_err(|err| format!("Failed to parse program address `{program_str}`: {err}"))?;
    let so_path = PathBuf::from(path_str.trim());
    if !so_path.exists() {
        return Err(format!("Specified program file `{}` does not exist", so_path.display()));
    }
    Ok(ProgramReplacement { program_id, so_path })
}

pub fn parse_funding(raw: &str) -> Result<Funding, String> {
    let (pubkey_str, amount_str) = raw
        .split_once('=')
        .ok_or_else(|| "Funding must be in <PUBKEY>=<AMOUNT> format".to_string())?;
    let pubkey = Pubkey::from_str(pubkey_str)
        .map_err(|err| format!("Failed to parse pubkey `{pubkey_str}`: {err}"))?;
    let amount_sol = amount_str
        .trim()
        .parse::<f64>()
        .map_err(|err| format!("Failed to parse amount `{amount_str}`: {err}"))?;

    if amount_sol < 0.0 {
        return Err("Funding amount must be non-negative".to_string());
    }

    Ok(Funding { pubkey, amount_sol })
}

/// Parse comma-separated transaction inputs for bundle simulation.
/// Returns a vector of individual transaction strings.
pub fn parse_multi_tx(input: &str) -> Vec<String> {
    input.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect()
}

pub fn parse_token_funding(raw: &str) -> Result<TokenFunding, String> {
    let mut parts = raw.split(':');
    let token_str = parts.next().ok_or_else(|| {
        "Token funding must be in <TOKEN_ACCOUNT>:<MINT_ACCOUNT>:<AMOUNT> format".to_string()
    })?;
    let mint_str = parts.next().ok_or_else(|| {
        "Token funding must be in <TOKEN_ACCOUNT>:<MINT_ACCOUNT>:<AMOUNT> format".to_string()
    })?;
    let amount_str = parts.next().ok_or_else(|| {
        "Token funding must be in <TOKEN_ACCOUNT>:<MINT_ACCOUNT>:<AMOUNT> format".to_string()
    })?;
    if parts.next().is_some() {
        return Err(
            "Token funding must be in <TOKEN_ACCOUNT>:<MINT_ACCOUNT>:<AMOUNT> format".to_string()
        );
    }

    let account = Pubkey::from_str(token_str)
        .map_err(|err| format!("Failed to parse token account `{token_str}`: {err}"))?;
    let mint = Pubkey::from_str(mint_str)
        .map_err(|err| format!("Failed to parse mint account `{mint_str}`: {err}"))?;
    let amount_raw = amount_str
        .trim()
        .parse::<u64>()
        .map_err(|err| format!("Failed to parse token amount `{amount_str}`: {err}"))?;

    Ok(TokenFunding { account, mint, amount_raw })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_multi_tx_single_transaction() {
        let result = parse_multi_tx("tx1");
        assert_eq!(result, vec!["tx1"]);
    }

    #[test]
    fn parse_multi_tx_multiple_transactions() {
        let result = parse_multi_tx("tx1,tx2,tx3");
        assert_eq!(result, vec!["tx1", "tx2", "tx3"]);
    }

    #[test]
    fn parse_multi_tx_with_whitespace() {
        let result = parse_multi_tx("tx1, tx2 , tx3");
        assert_eq!(result, vec!["tx1", "tx2", "tx3"]);
    }

    #[test]
    fn parse_multi_tx_filters_empty() {
        let result = parse_multi_tx("tx1,,tx2,");
        assert_eq!(result, vec!["tx1", "tx2"]);
    }

    #[test]
    fn parse_token_funding_accepts_valid_input() {
        let token = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let input = format!("{token}:{mint}:12345");
        let parsed = parse_token_funding(&input).expect("parses");
        assert_eq!(parsed.account, token);
        assert_eq!(parsed.mint, mint);
        assert_eq!(parsed.amount_raw, 12_345);
    }

    #[test]
    fn parse_token_funding_rejects_invalid_format() {
        let err = parse_token_funding("invalid").unwrap_err();
        assert!(err.contains("<TOKEN_ACCOUNT>:<MINT_ACCOUNT>:<AMOUNT>"));
    }

    #[test]
    fn parse_token_funding_rejects_negative_amount() {
        let key = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let err = parse_token_funding(&format!("{key}:{mint}:-1")).unwrap_err();
        assert!(err.contains("Failed to parse token amount"));
    }
}
