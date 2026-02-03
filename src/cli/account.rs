//! Account command arguments.

use std::path::PathBuf;

use clap::Args;

#[derive(Args, Debug)]
pub struct AccountArgs {
    /// Solana account address (base58 pubkey)
    pub account: String,

    /// RPC endpoint for fetching account data
    #[arg(long = "rpc-url", default_value = "https://api.mainnet-beta.solana.com")]
    pub rpc_url: String,

    /// Local IDL directory. Falls back to fetching from chain if not found.
    #[arg(long = "idl-path")]
    pub idl_path: Option<PathBuf>,

    /// Output raw account data in JSON format, skip IDL parsing
    #[arg(long)]
    pub raw: bool,

    /// Skip account metadata, only print parsed data
    #[arg(long)]
    pub no_account_meta: bool,
}
