//! Send command arguments.

use clap::{Args, ValueEnum};

use super::RpcArgs;

#[derive(Args, Debug)]
pub struct SendArgs {
    /// Raw transaction string (Base58/Base64 encoded, must be signed)
    #[arg(value_name = "TX")]
    pub tx: String,

    #[command(flatten)]
    pub rpc: RpcArgs,

    /// Skip preflight transaction checks
    #[arg(long = "skip-preflight", env = "SONAR_SKIP_PREFLIGHT")]
    pub skip_preflight: bool,

    /// Wait for transaction confirmation after sending
    #[arg(long = "wait")]
    pub wait: bool,

    /// Timeout in seconds for --wait mode
    #[arg(long = "wait-timeout-secs", value_name = "SECONDS", requires = "wait")]
    pub wait_timeout_secs: Option<u64>,

    /// Commitment level to wait for when --wait is enabled
    #[arg(long = "wait-commitment", value_name = "LEVEL", value_enum, requires = "wait")]
    pub wait_commitment: Option<WaitCommitmentArg>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum WaitCommitmentArg {
    Processed,
    Confirmed,
    Finalized,
}
