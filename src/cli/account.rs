//! Account command arguments.

use std::path::PathBuf;

use clap::Args;

#[derive(Args, Debug)]
pub struct AccountArgs {
    /// Account pubkey to parse
    pub account: String,

    /// Solana RPC node URL
    #[arg(long = "rpc-url", default_value = "https://api.mainnet-beta.solana.com")]
    pub rpc_url: String,

    /// Path to directory containing IDL files (named as <PROGRAM_ID>.json)
    #[arg(long = "idl-path")]
    pub idl_path: Option<PathBuf>,

    /// Only print raw account data without parsing
    #[arg(long)]
    pub raw: bool,

    /// Show verbose output (account info, owner, IDL details)
    #[arg(short, long)]
    pub verbose: bool,
}
