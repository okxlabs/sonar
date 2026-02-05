//! CLI argument parsing and validation.

mod account;
mod convert;
mod fetch_idl;
mod pda;
mod program_data;
mod send;
mod simulate;

// Re-export all public types from submodules
pub use account::*;
pub use convert::*;
pub use fetch_idl::*;
pub use pda::*;
pub use program_data::*;
pub use send::*;
pub use simulate::*;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "solsim", version, about = "Solana Transaction Simulator based on LiteSVM")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Simulate a specified raw transaction
    Simulate(SimulateArgs),
    /// Fetch Anchor IDL from on-chain program accounts
    FetchIdl(FetchIdlArgs),
    /// Fetch and decode Solana account data if IDL is available onchain or locally
    #[command(name = "account")]
    Account(AccountArgs),
    /// Convert between data formats (number, hex, arrays, utf8, base64, base58, lamports, sol)
    #[command(
        after_help = "EXAMPLES:\n  solsim convert 0x48656c6c6f -t utf8          # hex to UTF-8 -> Hello\n  solsim convert 1000000000 -f lam -t sol       # lamports to SOL -> 1\n  solsim convert 255 -t hex                    # number to hex (LE) -> 0xff\n  solsim convert SGVsbG8= -f base64 -t utf8    # base64 to UTF-8 -> Hello\n  solsim convert 0x12345678 -t dec-array       # hex to decimal byte array"
    )]
    Convert(ConvertArgs),
    /// Derive a PDA (Program Derived Address) from seeds
    Pda(PdaArgs),
    /// Get raw program data (ELF bytecode) from an upgradeable program or buffer
    #[command(name = "program-data")]
    ProgramData(ProgramDataArgs),
    /// Send a signed transaction to the network
    Send(SendArgs),
}
