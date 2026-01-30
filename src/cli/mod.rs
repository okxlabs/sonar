//! CLI argument parsing and validation.

mod convert;
mod fetch_idl;
mod pda;
mod simulate;

// Re-export all public types from submodules
pub use convert::*;
pub use fetch_idl::*;
pub use pda::*;
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
    /// Convert bytes to number (b2n = bytes to number)
    B2n(B2nArgs),
    /// Convert number to bytes (n2b = number to bytes)
    N2b(N2bArgs),
    /// Derive a PDA (Program Derived Address) from seeds
    Pda(PdaArgs),
}
