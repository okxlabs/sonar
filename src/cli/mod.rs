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
mod send;
mod simulate;
mod trace;

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
pub use send::*;
pub use simulate::*;
pub use trace::*;

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
}

#[derive(Parser, Debug)]
#[command(
    name = "sonar",
    version,
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
    /// Simulate a Solana transaction locally using LiteSVM
    #[command(alias = "sim", next_line_help = true)]
    Simulate(Box<SimulateArgs>),
    /// Decode and display a raw transaction without simulation
    #[command(alias = "dec", next_line_help = true)]
    Decode(DecodeArgs),
    /// Display the historical execution trace of a confirmed transaction
    #[command(next_line_help = true)]
    Trace(TraceArgs),
    /// Manage Anchor IDLs (fetch, sync, address)
    #[command(next_line_help = true)]
    Idl(IdlArgs),
    /// Fetch and decode a Solana account (`account`, alias: `acc`)
    #[command(name = "account", alias = "acc", next_line_help = true)]
    Account(AccountArgs),
    /// List, clean, or inspect cached account data for offline simulation
    #[command(next_line_help = true)]
    Cache(CacheArgs),
    /// Convert data formats (int, hex, arrays, text, base64, base58, lamports, sol)
    #[command(alias = "conv", next_line_help = true)]
    Convert(ConvertArgs),
    /// Derive a PDA (Program Derived Address) from seeds
    #[command(next_line_help = true)]
    Pda(PdaArgs),
    /// Get raw program data (ELF bytecode) from an upgradeable Program/ProgramData/Buffer account
    #[command(name = "program-elf", next_line_help = true)]
    ProgramData(ProgramDataArgs),
    /// Send a signed transaction to the network
    #[command(next_line_help = true)]
    Send(SendArgs),
    /// Generate shell completion scripts
    #[command(next_line_help = true)]
    Completions(CompletionsArgs),
    /// Serialize or deserialize data using Borsh-compatible type descriptors
    #[command(next_line_help = true)]
    Borsh(BorshArgs),
    /// View or modify ~/.config/sonar/config.toml
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
    /// The shell to generate completions for (bash, zsh, fish, elvish, powershell)
    pub shell: clap_complete::Shell,
}
