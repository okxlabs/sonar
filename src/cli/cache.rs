use std::path::PathBuf;

use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct CacheArgs {
    #[command(subcommand)]
    pub command: CacheCommands,
}

#[derive(Subcommand, Debug)]
pub enum CacheCommands {
    /// List all cached transaction account snapshots
    List,
    /// Remove cached account snapshots
    Clean(CacheCleanArgs),
    /// Show details of a specific cache entry
    Info(CacheInfoArgs),
}

#[derive(Args, Debug)]
pub struct CacheCleanArgs {
    /// Only remove caches older than the specified duration (e.g. 7d, 24h)
    #[arg(long, value_name = "DURATION")]
    pub older_than: Option<String>,
}

#[derive(Args, Debug)]
pub struct CacheInfoArgs {
    /// Cache key (transaction signature or bundle-<hash>)
    pub key: String,
    /// Override the cache root directory
    #[arg(long, value_name = "DIR", env = "SONAR_CACHE_DIR")]
    pub cache_dir: Option<PathBuf>,
}
