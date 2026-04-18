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
    /// The default (100) matches the Solana validator limit.
    /// Commercial RPC providers often support higher values.
    #[arg(long = "rpc-batch-size", env = "SONAR_RPC_BATCH_SIZE", default_value = "100")]
    pub rpc_batch_size: usize,

    /// Fetch account state from a historical slot via the non-standard
    /// getMultipleAccountsDataBySlot RPC method. Requires an RPC node that
    /// supports this method (e.g. certain archival providers).
    #[arg(long = "history-slot", value_name = "SLOT")]
    pub history_slot: Option<u64>,
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
    /// Parse a raw transaction without executing it
    ///
    /// Unlike simulate, decode does not run the transaction — it only parses
    /// instruction data and account metadata from the raw transaction bytes.
    #[command(alias = "dec", next_line_help = true)]
    Decode(DecodeArgs),
    /// Fetch and display a confirmed transaction's on-chain execution
    ///
    /// Unlike simulate, replay retrieves the actual execution results (logs,
    /// inner instructions, balance changes) from the RPC node — no local
    /// execution.
    #[command(next_line_help = true)]
    Replay(ReplayArgs),
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
    ///
    /// Either --output <FILE> (use "-" for stdout) or --verify-sha256 <HASH> is
    /// required. When both are given, the file is written only if the hash matches.
    #[command(name = "program-elf", next_line_help = true)]
    ProgramData(ProgramDataArgs),
    /// Send a signed transaction to the network
    ///
    /// Unlike simulate, send broadcasts to the RPC and mutates on-chain state.
    /// The TX must already be signed — sonar does not sign. Use --wait to block
    /// until the configured commitment level is reached.
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
