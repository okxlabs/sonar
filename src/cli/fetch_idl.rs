//! FetchIdl command arguments.

use std::path::PathBuf;

use clap::Args;

#[derive(Args, Debug)]
pub struct FetchIdlArgs {
    /// Comma-separated list of program IDs to fetch IDLs for
    #[arg(long = "programs", value_name = "PROGRAM_IDS", conflicts_with = "sync_dir")]
    pub programs: Option<String>,
    /// Directory containing existing IDL files to sync (output defaults to this directory)
    #[arg(long = "sync-dir", value_name = "PATH", conflicts_with = "programs")]
    pub sync_dir: Option<PathBuf>,
    /// Solana RPC node URL
    #[arg(long = "rpc-url", default_value = "https://api.mainnet-beta.solana.com")]
    pub rpc_url: String,
    /// Output directory for IDL files (default: sync-dir if set, otherwise current directory)
    #[arg(long = "output-dir", value_name = "PATH")]
    pub output_dir: Option<PathBuf>,
}
