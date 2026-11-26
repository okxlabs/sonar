//! Write-buffer command arguments.

use std::path::PathBuf;

use clap::Args;

use super::RpcArgs;

/// Default keypair path used by the Solana CLI.
pub const DEFAULT_KEYPAIR_PATH: &str = "~/.config/solana/id.json";

#[derive(Args, Debug)]
#[command(after_help = "\
EXAMPLES:
  sonar write-buffer target/deploy/my_program.so --rpc-url https://api.devnet.solana.com
  sonar write-buffer program.so --buffer buffer-keypair.json --buffer-authority authority.json
  sonar write-buffer program.so --buffer <EXISTING_BUFFER_ADDRESS>   # resume a partial write
  sonar write-buffer program.so --max-len 400000 --priority-fee 10000
")]
pub struct WriteBufferArgs {
    /// Program ELF (.so) to write into the buffer
    #[arg(value_name = "PROGRAM_FILEPATH")]
    pub program: PathBuf,

    #[command(flatten)]
    pub rpc: RpcArgs,

    /// Default signer keypair file: the fee payer and buffer authority unless overridden
    #[arg(
        short = 'k',
        long = "keypair",
        value_name = "KEYPAIR",
        env = "SONAR_KEYPAIR",
        default_value = DEFAULT_KEYPAIR_PATH
    )]
    pub keypair: String,

    /// Fee payer keypair file [default: --keypair]
    #[arg(long = "fee-payer", value_name = "KEYPAIR")]
    pub fee_payer: Option<String>,

    /// Buffer keypair file, or the address of an existing buffer to resume
    /// [default: a newly generated keypair]
    #[arg(long = "buffer", value_name = "BUFFER_SIGNER")]
    pub buffer: Option<String>,

    /// Buffer authority keypair file [default: --keypair]
    #[arg(long = "buffer-authority", value_name = "KEYPAIR")]
    pub buffer_authority: Option<String>,

    /// Program data capacity of the buffer in bytes [default: the program size]
    #[arg(long = "max-len", value_name = "BYTES")]
    pub max_len: Option<usize>,

    /// Total priority fee per transaction in lamports (the v1 header priority fee)
    #[arg(long = "priority-fee", value_name = "LAMPORTS", conflicts_with = "compute_unit_price")]
    pub priority_fee: Option<u64>,

    /// Compute unit price in micro-lamports, converted to the v1 total priority fee
    /// using the compute unit limit
    #[arg(long = "with-compute-unit-price", value_name = "MICRO_LAMPORTS")]
    pub compute_unit_price: Option<u64>,

    /// Compute unit limit requested by each transaction
    #[arg(long = "compute-unit-limit", value_name = "UNITS", default_value = "10000")]
    pub compute_unit_limit: u32,

    /// Maximum number of times to re-sign and resend unconfirmed transactions
    /// after their blockhash expires
    #[arg(long = "max-sign-attempts", value_name = "N", default_value = "5",
          value_parser = clap::value_parser!(u32).range(1..))]
    pub max_sign_attempts: u32,

    /// Skip the check that the fee payer can cover rent and fees
    #[arg(long = "skip-fee-check")]
    pub skip_fee_check: bool,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Commands};

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("sonar").chain(args.iter().copied()))
    }

    #[test]
    fn defaults_match_the_solana_cli() {
        let cli = parse(&["write-buffer", "program.so"]).expect("parses");
        let Some(Commands::WriteBuffer(args)) = cli.command else {
            panic!("expected write-buffer");
        };
        assert_eq!(args.program.to_str(), Some("program.so"));
        assert_eq!(args.max_sign_attempts, 5);
        assert_eq!(args.compute_unit_limit, 10_000);
        assert!(args.buffer.is_none());
        assert!(args.fee_payer.is_none());
        assert!(args.buffer_authority.is_none());
        assert!(!args.skip_fee_check);
    }

    #[test]
    fn priority_fee_and_compute_unit_price_conflict() {
        let err = parse(&[
            "write-buffer",
            "program.so",
            "--priority-fee",
            "1",
            "--with-compute-unit-price",
            "1",
        ])
        .expect_err("conflicting fee flags");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn max_sign_attempts_must_be_positive() {
        let err = parse(&["write-buffer", "program.so", "--max-sign-attempts", "0"])
            .expect_err("zero attempts rejected");
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }
}
