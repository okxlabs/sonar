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
    #[arg(long = "raw-ix-data", env = "SONAR_RAW_IX_DATA")]
    pub ix_data: bool,
    /// Directory containing Anchor IDL JSON files
    #[arg(long = "idl-dir", value_name = "DIR", env = "SONAR_IDL_DIR")]
    pub idl_dir: Option<PathBuf>,
    /// Skip auto-fetching missing IDLs from chain
    #[arg(long = "no-idl-fetch", env = "SONAR_NO_IDL_FETCH")]
    pub no_idl_fetch: bool,
    /// Disable account cache usage. Raw transaction cache lookup is still allowed.
    #[arg(long = "no-cache", default_value_t = false)]
    pub no_cache: bool,
    /// Override the cache root directory (default: ~/.sonar/cache)
    #[arg(long = "cache-dir", value_name = "DIR", env = "SONAR_CACHE_DIR")]
    pub cache_dir: Option<PathBuf>,
    /// Ignore existing cache entries and re-fetch from RPC (including signature->raw-tx).
    #[arg(long = "refresh-cache", default_value_t = false)]
    pub refresh_cache: bool,
}
