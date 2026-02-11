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
    /// Directory containing Anchor IDL JSON files
    #[arg(long = "idl-dir", value_name = "DIR", env = "SONAR_IDL_DIR")]
    pub idl_dir: Option<PathBuf>,
}
