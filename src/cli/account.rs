//! Account command arguments.

use std::path::PathBuf;

use clap::Args;

use super::RpcArgs;

#[derive(Args, Debug)]
pub struct AccountArgs {
    /// Solana account address (base58 pubkey) or path to a local JSON file
    /// exported via `solana account --output json`.
    /// Omit to read JSON from stdin (e.g. `solana account <PUBKEY> --output json | sonar account`).
    pub account: Option<String>,

    #[command(flatten)]
    pub rpc: RpcArgs,

    /// Local IDL directory. Falls back to fetching from chain if not found.
    #[arg(long = "idl-dir", env = "SONAR_IDL_DIR")]
    pub idl_dir: Option<PathBuf>,

    /// Output raw account data as base64 JSON, skip decoding
    #[arg(long)]
    pub raw: bool,
}

#[cfg(test)]
mod tests {
    use super::super::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn account_rejects_removed_mpl_metadata_flag() {
        let result = Cli::try_parse_from([
            "sonar",
            "account",
            "11111111111111111111111111111111",
            "--rpc-url",
            "http://localhost:8899",
            "--mpl-metadata",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn account_accepts_short_rpc_url_flag() {
        let cli = Cli::try_parse_from([
            "sonar",
            "account",
            "11111111111111111111111111111111",
            "-u",
            "http://localhost:8899",
        ])
        .expect("should parse -u for rpc-url");

        let Some(Commands::Account(args)) = cli.command else {
            panic!("expected account subcommand");
        };
        assert_eq!(args.rpc.rpc_url, "http://localhost:8899");
    }

    #[test]
    fn account_accepts_json_flag() {
        let cli = Cli::try_parse_from([
            "sonar",
            "account",
            "11111111111111111111111111111111",
            "--rpc-url",
            "http://localhost:8899",
            "--json",
        ])
        .expect("should parse --json");

        assert!(cli.json);
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

    #[test]
    fn account_rejects_removed_no_account_meta_flag() {
        let result = Cli::try_parse_from([
            "sonar",
            "account",
            "11111111111111111111111111111111",
            "--rpc-url",
            "http://localhost:8899",
            "--no-account-meta",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn account_parses_without_positional_for_stdin() {
        let cli = Cli::try_parse_from(["sonar", "account", "--rpc-url", "http://localhost:8899"])
            .expect("should parse with omitted account for stdin");

        let Some(Commands::Account(args)) = cli.command else {
            panic!("expected account subcommand");
        };
        assert!(args.account.is_none());
    }
}
