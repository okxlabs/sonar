//! IDL command group arguments.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use super::RpcArgs;

/// Manage Anchor IDLs: fetch, sync, and derive IDL account address.
#[derive(Args, Debug)]
#[command(after_help = "\
EXAMPLES:
  sonar idl fetch <PROG>                       Fetch one IDL to cwd
  sonar idl fetch <PROG1> <PROG2> -o ./idls    Fetch many into ./idls
  sonar idl sync ./idls                        Upload all IDLs in dir
  sonar idl sync ./idls/<PROG>.json            Upload a single IDL file
  sonar idl address <PROG>                     Derive the IDL account PDA")]
pub struct IdlArgs {
    #[command(subcommand)]
    pub command: IdlSubcommands,
}

#[derive(Subcommand, Debug)]
pub enum IdlSubcommands {
    /// Fetch Anchor IDLs from on-chain program accounts
    ///
    /// Use when you have the program ID and want the latest on-chain IDL.
    /// Writes one `<PUBKEY>.json` per program to --output-dir (default: cwd).
    Fetch(IdlFetchArgs),
    /// Sync IDLs using a directory or one `<PUBKEY>.json` file as source
    ///
    /// Inverse of fetch: re-uploads local IDL files to the IDL account on chain.
    Sync(IdlSyncArgs),
    /// Calculate Anchor IDL account address for a program
    ///
    /// Pure derivation — no RPC call. Equivalent to the Anchor IDL seed convention.
    Address(IdlAddressArgs),
}

#[derive(Args, Debug)]
pub struct IdlFetchArgs {
    /// Program IDs to fetch IDLs for
    #[arg(value_name = "PUBKEY", required = true)]
    pub programs: Vec<String>,
    #[command(flatten)]
    pub rpc: RpcArgs,
    /// Output directory for IDL files
    #[arg(short = 'o', long = "output-dir", value_name = "DIR")]
    pub output_dir: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct IdlSyncArgs {
    /// Source path: IDL directory or one `<PUBKEY>.json` file
    #[arg(value_name = "PATH")]
    pub path: PathBuf,
    #[command(flatten)]
    pub rpc: RpcArgs,
    /// Output directory for IDL files
    #[arg(short = 'o', long = "output-dir", value_name = "DIR")]
    pub output_dir: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct IdlAddressArgs {
    /// Program ID
    #[arg(value_name = "PUBKEY")]
    pub program: String,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Commands};

    #[test]
    fn parses_idl_fetch() {
        let cli = Cli::try_parse_from([
            "sonar",
            "idl",
            "fetch",
            "11111111111111111111111111111111",
            "--rpc-url",
            "http://localhost:8899",
            "-o",
            "./idls",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Idl(args)) => match args.command {
                super::IdlSubcommands::Fetch(fetch) => {
                    assert_eq!(fetch.programs, vec!["11111111111111111111111111111111"]);
                    assert_eq!(fetch.output_dir.unwrap(), std::path::PathBuf::from("./idls"));
                }
                _ => panic!("expected idl fetch subcommand"),
            },
            _ => panic!("expected idl command"),
        }
    }

    #[test]
    fn parses_idl_sync_dir() {
        let cli = Cli::try_parse_from([
            "sonar",
            "idl",
            "sync",
            "./idls",
            "--rpc-url",
            "http://localhost:8899",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Idl(args)) => match args.command {
                super::IdlSubcommands::Sync(sync) => {
                    assert_eq!(sync.path, std::path::PathBuf::from("./idls"));
                }
                _ => panic!("expected idl sync subcommand"),
            },
            _ => panic!("expected idl command"),
        }
    }

    #[test]
    fn parses_idl_sync_file() {
        let cli = Cli::try_parse_from([
            "sonar",
            "idl",
            "sync",
            "./idls/11111111111111111111111111111111.json",
            "--rpc-url",
            "http://localhost:8899",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Idl(args)) => match args.command {
                super::IdlSubcommands::Sync(sync) => {
                    assert_eq!(
                        sync.path,
                        std::path::PathBuf::from("./idls/11111111111111111111111111111111.json")
                    );
                }
                _ => panic!("expected idl sync subcommand"),
            },
            _ => panic!("expected idl command"),
        }
    }

    #[test]
    fn parses_idl_address() {
        let cli =
            Cli::try_parse_from(["sonar", "idl", "address", "11111111111111111111111111111111"])
                .unwrap();
        match cli.command {
            Some(Commands::Idl(args)) => match args.command {
                super::IdlSubcommands::Address(addr) => {
                    assert_eq!(addr.program, "11111111111111111111111111111111");
                }
                _ => panic!("expected idl address subcommand"),
            },
            _ => panic!("expected idl command"),
        }
    }

    #[test]
    fn rejects_removed_fetch_idl_command() {
        let err = Cli::try_parse_from(["sonar", "fetch-idl", "11111111111111111111111111111111"])
            .unwrap_err();
        assert!(err.to_string().contains("unrecognized subcommand 'fetch-idl'"));
    }
}
