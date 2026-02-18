//! Account command arguments.

use std::path::PathBuf;

use clap::Args;

use super::RpcArgs;

#[derive(Args, Debug)]
pub struct AccountArgs {
    /// Solana account address (base58 pubkey)
    pub account: String,

    #[command(flatten)]
    pub rpc: RpcArgs,

    /// Local IDL directory. Falls back to fetching from chain if not found.
    #[arg(long = "idl-dir", env = "SONAR_IDL_DIR")]
    pub idl_dir: Option<PathBuf>,

    /// Output raw account data as base64 JSON, skip decoding
    #[arg(long)]
    pub raw: bool,

    /// Skip account metadata, only print parsed data
    #[arg(long = "no-account-meta")]
    pub no_account_meta: bool,

    /// For SPL Token legacy or Token-2022 mint accounts, decode Metaplex metadata PDA.
    /// If metadata is missing or invalid, prints a warning to stderr and falls back to mint data.
    #[arg(short = 'm', long = "mpl-metadata")]
    pub mpl_metadata: bool,
}

#[cfg(test)]
mod tests {
    use super::super::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn account_accepts_long_mpl_metadata_flag() {
        let cli = Cli::try_parse_from([
            "sonar",
            "account",
            "11111111111111111111111111111111",
            "--rpc-url",
            "http://localhost:8899",
            "--mpl-metadata",
        ])
        .expect("should parse --mpl-metadata");

        let Commands::Account(args) = cli.command else {
            panic!("expected account subcommand");
        };
        assert!(args.mpl_metadata);
    }

    #[test]
    fn account_accepts_short_mpl_metadata_flag() {
        let cli = Cli::try_parse_from([
            "sonar",
            "account",
            "11111111111111111111111111111111",
            "--rpc-url",
            "http://localhost:8899",
            "-m",
        ])
        .expect("should parse -m");

        let Commands::Account(args) = cli.command else {
            panic!("expected account subcommand");
        };
        assert!(args.mpl_metadata);
    }

    #[test]
    fn account_rejects_removed_metadata_flag() {
        let result = Cli::try_parse_from([
            "sonar",
            "account",
            "11111111111111111111111111111111",
            "--rpc-url",
            "http://localhost:8899",
            "--metadata",
        ]);

        assert!(result.is_err());
    }
}
