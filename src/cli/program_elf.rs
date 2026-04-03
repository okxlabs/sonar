//! Program ELF command arguments.

use std::path::PathBuf;

use clap::Args;

use super::RpcArgs;

#[derive(Args, Debug)]
pub struct ProgramDataArgs {
    /// Program, ProgramData, or Buffer address to fetch ELF data for
    pub address: String,

    #[command(flatten)]
    pub rpc: RpcArgs,

    /// Verify the program data matches the expected SHA256 hash (hex string)
    #[arg(long, value_name = "HASH")]
    pub verify_sha256: Option<String>,

    /// Output file path for raw ELF bytes (use "-" for stdout)
    #[arg(short, long, required_unless_present = "verify_sha256")]
    pub output: Option<PathBuf>,

    /// Fetch program data from a historical slot via the non-standard
    /// getMultipleAccountsDataBySlot RPC method.
    #[arg(long = "history-slot", value_name = "SLOT")]
    pub history_slot: Option<u64>,
}
