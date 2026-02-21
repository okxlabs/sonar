//! CLI argument parsing and validation.

mod account;
mod convert;
mod decode;
mod fetch_idl;
mod pda;
mod program_elf;
mod send;
mod simulate;

// Re-export all public types from submodules
pub use account::*;
pub use convert::*;
pub use decode::*;
pub use fetch_idl::*;
pub use pda::*;
pub use program_elf::*;
pub use send::*;
pub use simulate::*;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Shared RPC connection arguments for all subcommands that need RPC access.
#[derive(Args, Debug, Clone)]
pub struct RpcArgs {
    /// Solana RPC node URL
    #[arg(
        long = "rpc-url",
        env = "RPC_URL",
        default_value = "https://api.mainnet-beta.solana.com"
    )]
    pub rpc_url: String,
}

/// Color output mode.
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Default)]
pub enum ColorMode {
    /// Auto-detect: enable color when stdout is a terminal and NO_COLOR is not set
    #[default]
    Auto,
    /// Always enable color output
    Always,
    /// Never use color output
    Never,
}

#[derive(Parser, Debug)]
#[command(name = "sonar", version, about = "Solana Developer Toolkit powered by LiteSVM", next_line_help = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Control color output (auto, always, never)
    #[arg(long, global = true, value_enum, default_value_t = ColorMode::Auto, env = "SONAR_COLOR")]
    pub color: ColorMode,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Simulate a Solana transaction locally using LiteSVM
    #[command(alias = "sim", next_line_help = true)]
    Simulate(SimulateArgs),
    /// Decode and display a raw transaction without simulation
    #[command(alias = "dec", next_line_help = true)]
    Decode(DecodeArgs),
    /// Fetch Anchor IDL from on-chain program accounts
    #[command(next_line_help = true)]
    FetchIdl(FetchIdlArgs),
    /// Fetch and decode a Solana account (`account`, alias: `acc`)
    #[command(name = "account", alias = "acc", next_line_help = true)]
    Account(AccountArgs),
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
}

#[derive(Args, Debug)]
#[command(after_help = "\
EXAMPLES:
  sonar completions bash > ~/.local/share/bash-completion/completions/sonar
  sonar completions zsh > ~/.zsh/completions/_sonar
  sonar completions fish > ~/.config/fish/completions/sonar.fish")]
pub struct CompletionsArgs {
    /// The shell to generate completions for (bash, zsh, fish, elvish, powershell)
    pub shell: clap_complete::Shell,
}
