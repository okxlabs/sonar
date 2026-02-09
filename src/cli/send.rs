//! Send command arguments.

use clap::Args;

use super::RpcArgs;

#[derive(Args, Debug)]
pub struct SendArgs {
    /// Raw transaction string (Base58/Base64 encoded, must be signed)
    #[arg(value_name = "TX")]
    pub tx: String,

    #[command(flatten)]
    pub rpc: RpcArgs,

    /// Skip preflight transaction checks
    #[arg(long = "skip-preflight")]
    pub skip_preflight: bool,
}
