//! Trace command arguments.

use std::path::PathBuf;

use clap::Args;

use super::RpcArgs;

/// Display the historical execution trace of a confirmed transaction.
#[derive(Args, Debug)]
pub struct TraceArgs {
    /// Transaction signature to look up
    pub signature: String,
    #[command(flatten)]
    pub rpc: RpcArgs,
    /// Directory containing Anchor IDL JSON files
    #[arg(long = "idl-dir", value_name = "DIR", env = "SONAR_IDL_DIR")]
    pub idl_dir: Option<PathBuf>,
    /// Skip auto-fetching missing IDLs from chain
    #[arg(long = "no-idl-fetch", env = "SONAR_NO_IDL_FETCH")]
    pub no_idl_fetch: bool,
    /// Always print raw instruction data, even when parser succeeds
    #[arg(long = "raw-ix-data", env = "SONAR_RAW_IX_DATA")]
    pub ix_data: bool,
    /// Print raw program logs instead of structured execution trace
    #[arg(short = 'l', long = "raw-log", env = "SONAR_RAW_LOG")]
    pub raw_log: bool,
    /// Show detailed instruction information (accounts, parsed fields, inner instructions)
    #[arg(short = 'd', long = "show-ix-detail", env = "SONAR_SHOW_IX_DETAIL")]
    pub show_ix_detail: bool,
    /// Show SOL and token balance changes
    #[arg(short = 'b', long = "show-balance-change", env = "SONAR_SHOW_BALANCE_CHANGE")]
    pub show_balance_change: bool,
}
