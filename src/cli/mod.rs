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
    /// Parse account data using Anchor IDL
    #[command(name = "account")]
    Account(AccountArgs),
    /// Convert bytes to number (b2n = bytes to number)
    B2n(B2nArgs),
    /// Convert number to bytes (n2b = number to bytes)
    N2b(N2bArgs),
    /// Derive a PDA (Program Derived Address) from seeds
    Pda(PdaArgs),
    /// Convert base64 string to base58 string
    #[command(name = "b64-b58")]
    B64B58(B64B58Args),
    /// Convert base58 string to base64 string
    #[command(name = "b58-b64")]
    B58B64(B58B64Args),
    /// Get raw program data (ELF bytecode) from an upgradeable program or buffer
    #[command(name = "program-data")]
    ProgramData(ProgramDataArgs),
    /// Send a signed transaction to the network
    Send(SendArgs),
}
