//! CLI argument parsing and validation.

mod account;
mod borsh;
mod cache;
mod config;
mod convert;
mod decode;
mod idl;
mod pda;
mod program_elf;
mod replay;
mod send;
mod simulate;

// Re-export all public types from submodules
pub use account::*;
pub use borsh::*;
pub use cache::*;
pub use config::*;
pub use convert::*;
pub use decode::*;
pub use idl::*;
pub use pda::*;
pub use program_elf::*;
pub use replay::*;
pub use send::*;
pub use simulate::*;

use clap::{Args, Parser, Subcommand};

/// Shared RPC connection arguments for all subcommands that need RPC access.
#[derive(Args, Debug, Clone)]
pub struct RpcArgs {
    /// Solana RPC node URL
    #[arg(
        short = 'u',
        long = "rpc-url",
        env = "RPC_URL",
        default_value = "https://api.mainnet-beta.solana.com"
    )]
    pub rpc_url: String,

    /// Maximum accounts per getMultipleAccounts RPC call.
    /// Matches the Solana validator limit; many commercial RPC providers allow higher.
    #[arg(long = "rpc-batch-size", env = "SONAR_RPC_BATCH_SIZE", default_value = "100")]
    pub rpc_batch_size: usize,
}

/// Crate version plus the git commit it was built from, e.g. `0.7.0 (7fe88d4)`.
/// `SONAR_GIT_HASH` is set by `build.rs`.
pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("SONAR_GIT_HASH"), ")");

#[derive(Parser, Debug)]
#[command(
    name = "sonar",
    version = VERSION,
    about = "Solana Developer Toolkit powered by LiteSVM",
    next_line_help = true
)]
pub struct Cli {
    /// Output as JSON instead of human-readable text
    #[arg(short = 'j', long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Simulate a transaction locally with LiteSVM
    #[command(visible_alias = "sim", next_line_help = true)]
    Simulate(Box<SimulateArgs>),
    /// Decode a transaction's instructions and accounts without executing it
    #[command(visible_alias = "dec", next_line_help = true)]
    Decode(DecodeArgs),
    /// Fetch a confirmed transaction's actual on-chain execution (logs, balances, inner ix)
    #[command(next_line_help = true)]
    Replay(ReplayArgs),
    /// Manage Anchor IDLs (fetch, sync, address)
    #[command(next_line_help = true)]
    Idl(IdlArgs),
    /// Fetch and decode an account
    #[command(name = "account", visible_alias = "acc", next_line_help = true)]
    Account(AccountArgs),
    /// Manage cached account snapshots for offline simulation
    #[command(next_line_help = true)]
    Cache(CacheArgs),
    /// Convert values between data, encoding, and numeric formats
    #[command(visible_alias = "conv", next_line_help = true)]
    Convert(ConvertArgs),
    /// Derive a PDA (Program Derived Address) from seeds
    #[command(next_line_help = true)]
    Pda(PdaArgs),
    /// Fetch program ELF bytecode from a Program/ProgramData/Buffer account
    ///
    /// Requires --output or --verify-sha256; with both, the file is written only if the hash matches.
    #[command(name = "program-elf", next_line_help = true)]
    ProgramData(ProgramDataArgs),
    /// Send a signed transaction to the network
    ///
    /// Broadcasts to RPC and mutates on-chain state; the transaction must already be signed (sonar does not sign).
    #[command(next_line_help = true)]
    Send(SendArgs),
    /// Generate shell completion scripts
    #[command(next_line_help = true)]
    Completions(CompletionsArgs),
    /// Serialize or deserialize data with Borsh type descriptors
    #[command(next_line_help = true)]
    Borsh(BorshArgs),
    /// View or edit the resolved config file (`SONAR_CONFIG`, else default path)
    #[command(next_line_help = true)]
    Config(ConfigArgs),
}

#[derive(Args, Debug)]
#[command(after_help = "\
EXAMPLES:
  sonar completions bash > ~/.local/share/bash-completion/completions/sonar
  sonar completions zsh > ~/.zsh/completions/_sonar
  sonar completions fish > ~/.config/fish/completions/sonar.fish
")]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    pub shell: clap_complete::Shell,
}
