//! ParseAccount command arguments.

use clap::Args;

#[derive(Args, Debug)]
pub struct ParseAccountArgs {
    /// Account pubkey to parse
    pub account: String,

    /// Solana RPC node URL
    #[arg(long = "rpc-url", default_value = "https://api.mainnet-beta.solana.com")]
    pub rpc_url: String,

    /// Show verbose output (account info, owner, IDL details)
    #[arg(short, long)]
    pub verbose: bool,
}
