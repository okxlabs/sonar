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
    #[arg(long = "idl-dir", env = "SONAR_IDL_DIR")]
    pub idl_dir: Option<PathBuf>,

    /// Output raw account data as base64 JSON, skip decoding
    #[arg(long)]
    pub raw: bool,

    /// Skip account metadata, only print parsed data
    #[arg(long = "no-account-meta")]
    pub no_account_meta: bool,

    /// For legacy SPL mint accounts, also fetch and parse Metaplex metadata PDA
    #[arg(long)]
    pub metadata: bool,
}
