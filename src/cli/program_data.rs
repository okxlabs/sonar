//! Program data command arguments.

use std::path::PathBuf;

use clap::Args;

#[derive(Args, Debug)]
pub struct ProgramDataArgs {
    /// Program ID or Buffer address to fetch data for
    pub address: String,

    /// Solana RPC node URL
    #[arg(long = "rpc-url", default_value = "https://api.mainnet-beta.solana.com")]
    pub rpc_url: String,

    /// Treat the address as a Buffer account instead of a Program
    #[arg(long)]
    pub buffer: bool,

    /// Verify the program data matches the expected SHA256 hash (hex string)
    #[arg(long)]
    pub verify: Option<String>,

    /// Output file path (writes to stdout if not specified)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}
