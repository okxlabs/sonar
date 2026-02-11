//! Program data command arguments.

use std::path::PathBuf;

use clap::Args;

use super::RpcArgs;

#[derive(Args, Debug)]
pub struct ProgramDataArgs {
    /// Program ID or Buffer address to fetch data for
    pub address: String,

    #[command(flatten)]
    pub rpc: RpcArgs,

    /// Treat address as a buffer account instead of a program
    #[arg(long)]
    pub buffer: bool,

    /// Verify the program data matches the expected SHA256 hash (hex string)
    #[arg(long, value_name = "HASH")]
    pub verify_sha256: Option<String>,

    /// Output file path (writes to stdout if not specified)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}
