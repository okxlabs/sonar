use std::{path::PathBuf, str::FromStr};

use clap::{Args, Parser, Subcommand, ValueEnum};
use solana_pubkey::Pubkey;

#[derive(Parser, Debug)]
#[command(
    name = "solsim",
    version,
    about = "Solana Transaction Simulator based on LiteSVM"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Simulate a specified raw transaction
    Simulate(SimulateArgs),
}

#[derive(Args, Debug)]
pub struct SimulateArgs {
    #[command(flatten)]
    pub transaction: TransactionInputArgs,
    /// Solana RPC node URL
    #[arg(
        long = "rpc-url",
        default_value = "https://api.mainnet-beta.solana.com"
    )]
    pub rpc_url: String,
    /// Custom program replacement, format: <PROGRAM_ID>=<PATH_TO_ELF_OR_SO>
    #[arg(
        long = "replace",
        value_name = "MAPPING",
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub replacements: Vec<String>,
    /// Parse transaction only, skip simulation
    #[arg(long = "parse-only")]
    pub parse_only: bool,
    /// Verify transaction signatures during simulation
    #[arg(long = "check-sig")]
    pub verify_signatures: bool,
}

#[derive(Args, Debug, Clone)]
pub struct TransactionInputArgs {
    /// Raw transaction string (Base58/Base64) or transaction signature, mutually exclusive with --tx-file
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

pub fn parse_program_replacement(raw: &str) -> Result<ProgramReplacement, String> {
    let (program_str, path_str) = raw
        .split_once('=')
        .ok_or_else(|| "Replacement must be in <PROGRAM_ID>=<PATH> format".to_string())?;
    let program_id = Pubkey::from_str(program_str)
        .map_err(|err| format!("Failed to parse program address `{program_str}`: {err}"))?;
    let so_path = PathBuf::from(path_str.trim());
    if !so_path.exists() {
        return Err(format!(
            "Specified program file `{}` does not exist",
            so_path.display()
        ));
    }
    Ok(ProgramReplacement {
        program_id,
        so_path,
    })
}
