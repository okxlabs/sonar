//! FetchIdl command arguments.

use std::path::PathBuf;

use clap::Args;

use super::RpcArgs;

#[derive(Args, Debug)]
pub struct FetchIdlArgs {
    /// Program IDs to fetch IDLs for
    #[arg(conflicts_with = "sync_dir")]
    pub programs: Vec<String>,
    /// Directory containing existing IDL files to sync (output defaults to this directory)
    #[arg(long = "sync-dir", value_name = "PATH", conflicts_with = "programs")]
    pub sync_dir: Option<PathBuf>,
    #[command(flatten)]
    pub rpc: RpcArgs,
    /// Output directory for IDL files (default: sync-dir if set, otherwise current directory)
    #[arg(long = "output-dir", value_name = "PATH")]
    pub output_dir: Option<PathBuf>,
}
