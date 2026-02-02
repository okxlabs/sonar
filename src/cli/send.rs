//! Send command arguments.

use clap::Args;

#[derive(Args, Debug)]
pub struct SendArgs {
    /// Raw transaction string (Base58/Base64 encoded, must be signed)
    #[arg(value_name = "TX")]
    pub tx: String,

    /// Solana RPC node URL
    #[arg(long = "rpc-url", default_value = "https://api.mainnet-beta.solana.com")]
    pub rpc_url: String,

    /// Skip preflight transaction checks
    #[arg(long = "skip-preflight")]
    pub skip_preflight: bool,
}
