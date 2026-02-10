//! Decode command arguments.

use std::path::PathBuf;

use clap::Args;

use super::RpcArgs;
use super::simulate::TransactionInputArgs;

/// Decode and display a raw transaction without simulation.
#[derive(Args, Debug)]
pub struct DecodeArgs {
    #[command(flatten)]
    pub transaction: TransactionInputArgs,
    #[command(flatten)]
    pub rpc: RpcArgs,
    /// Always print raw instruction data, even when parser succeeds
    #[arg(long = "raw-ix-data")]
    pub ix_data: bool,
    /// Directory containing Anchor IDLs; omit to disable IDL parsing
    #[arg(long = "idl-path", value_name = "PATH")]
    pub idl_path: Option<PathBuf>,
}
