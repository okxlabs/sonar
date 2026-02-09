//! Account command arguments.

use std::path::PathBuf;

use clap::Args;

use super::RpcArgs;

#[derive(Args, Debug)]
pub struct AccountArgs {
    /// Solana account address (base58 pubkey)
    pub account: String,

    #[command(flatten)]
    pub rpc: RpcArgs,

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
