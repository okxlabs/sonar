//! CLI argument parsing and adapter for conversion logic.

use clap::{Args, ValueEnum};

use crate::converters::{self, ConvertRequest, InputFormat, OutputFormat};

/// Supported input formats.
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum ConvertInputFormat {
    /// Integer: decimal (e.g. 255) or 0x-prefixed hex
    Int,
    /// Hex string with 0x prefix, e.g. 0x1234abcd
    Hex,
    /// Hex byte array, e.g. [0x12,0x34] (alias: hb)
    #[value(alias = "hb")]
    HexBytes,
    /// Decimal byte array, e.g. [18,52,86,120]
    Bytes,
    /// Text input
    Text,
    /// Base64 encoded string (alias: b64)
    #[value(alias = "b64")]
    Base64,
    /// Binary bitstring with 0b prefix, e.g. 0b01010101 (alias: bin)
    #[value(alias = "bin")]
    Binary,
    /// Base58 encoded string, e.g. Solana pubkey (alias: b58)
    #[value(alias = "b58")]
    Base58,
    /// Solana pubkey (base58, 32-byte)
    #[value(alias = "pk")]
    Pubkey,
    /// Solana transaction signature (base58, 64-byte)
    #[value(alias = "sig")]
    Signature,
    /// Solana keypair bytes (64-byte: secret[32] + pubkey[32]) (alias: kp)
    #[value(alias = "kp")]
    Keypair,
    /// Unsigned 8-bit integer
    U8,
    /// Unsigned 16-bit integer
    U16,
    /// Unsigned 32-bit integer
    U32,
    /// Unsigned 64-bit integer
    U64,
    /// Unsigned 128-bit integer
    U128,
    /// Signed 8-bit integer
    I8,
    /// Signed 16-bit integer
    I16,
    /// Signed 32-bit integer
    I32,
    /// Signed 64-bit integer
    I64,
    /// Signed 128-bit integer
    I128,
    /// Lamports amount (alias: lam)
    #[value(alias = "lam")]
    Lamports,
    /// SOL amount as decimal string
    Sol,
}

/// Supported output formats.
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum ConvertOutputFormat {
    /// Integer output
    Int,
    /// Hex string output with 0x prefix
    Hex,
    /// Hex byte array output (alias: hb)
    #[value(alias = "hb")]
    HexBytes,
    /// Decimal byte array output
    Bytes,
    /// Text output
    Text,
    /// Binary bitstring output with 0b prefix (alias: bin)
    #[value(alias = "bin")]
    Binary,
    /// Base64 output (alias: b64)
    #[value(alias = "b64")]
    Base64,
    /// Base58 output (alias: b58)
    #[value(alias = "b58")]
    Base58,
    /// Solana pubkey output (base58, 32-byte)
    #[value(alias = "pk")]
    Pubkey,
    /// Solana transaction signature output (base58, 64-byte)
    #[value(alias = "sig")]
    Signature,
    /// Unsigned 8-bit integer output
    U8,
    /// Unsigned 16-bit integer output
    U16,
    /// Unsigned 32-bit integer output
    U32,
    /// Unsigned 64-bit integer output
    U64,
    /// Unsigned 128-bit integer output
    U128,
    /// Signed 8-bit integer output
    I8,
    /// Signed 16-bit integer output
    I16,
    /// Signed 32-bit integer output
    I32,
    /// Signed 64-bit integer output
    I64,
    /// Signed 128-bit integer output
    I128,
    /// Lamports output (alias: lam)
    #[value(alias = "lam")]
    Lamports,
    /// SOL output
    Sol,
}

#[derive(Args, Debug)]
#[command(after_help = "\
EXAMPLES:
  sonar convert hex text 0x48656c6c6f          Hex → text (\"Hello\")
  sonar convert text base64 'Hello World'      Text → Base64
  sonar convert sol lamports 1.5               SOL → lamports (1500000000)
  sonar convert lamports sol 1500000000        Lamports → SOL (1.5)
  sonar convert hex bytes 0x1234abcd           Hex → decimal byte array
  sonar convert base58 hex <PUBKEY>            Base58 pubkey → hex
  sonar convert u32 hex 305419896              u32 → hex (0x12345678)
  sonar convert hex u32 0x12345678             Hex → u32 (305419896)
  sonar convert hex binary 0xff                Hex → binary bitstring
  echo '0x48656c6c6f' | sonar convert hex text Pipe via stdin

FORMATS:
  Generic:  int, hex, hex-bytes (hb), bytes, text, binary (bin), base64 (b64), base58 (b58)
  Solana:   pubkey (pk), signature (sig), keypair (kp, input only), lamports (lam), sol
  Fixed:    u8, u16, u32, u64, u128, i8, i16, i32, i64, i128
")]
pub struct ConvertArgs {
    /// Input format
    #[arg(value_name = "FROM", index = 1)]
    pub from: ConvertInputFormat,

    /// Output format
    #[arg(value_name = "TO", index = 2)]
    pub to: ConvertOutputFormat,

    /// Input value (omit to read from stdin)
    #[arg(value_name = "INPUT", index = 3, required = false, allow_hyphen_values = true)]
    pub input: Option<String>,

    /// Use little-endian byte order; default is big-endian
    #[arg(long)]
    pub le: bool,

    /// Separator for array outputs (single character)
    #[arg(long, value_name = "CHAR", default_value = ",")]
    pub sep: String,

    /// Disable 0x prefix in hex-bytes output
    #[arg(long)]
    pub no_prefix: bool,

    /// Show invalid text bytes as \xNN escape sequences (for text output)
    #[arg(short = 'e', long)]
    pub escape: bool,
}

impl From<ConvertInputFormat> for InputFormat {
    fn from(value: ConvertInputFormat) -> Self {
        match value {
            ConvertInputFormat::Int => Self::Int,
            ConvertInputFormat::Hex => Self::Hex,
            ConvertInputFormat::HexBytes => Self::HexBytes,
            ConvertInputFormat::Bytes => Self::Bytes,
            ConvertInputFormat::Text => Self::Text,
            ConvertInputFormat::Base64 => Self::Base64,
            ConvertInputFormat::Binary => Self::Binary,
            ConvertInputFormat::Base58 => Self::Base58,
            ConvertInputFormat::Pubkey => Self::Pubkey,
            ConvertInputFormat::Signature => Self::Signature,
            ConvertInputFormat::Keypair => Self::Keypair,
            ConvertInputFormat::U8 => Self::U8,
            ConvertInputFormat::U16 => Self::U16,
            ConvertInputFormat::U32 => Self::U32,
            ConvertInputFormat::U64 => Self::U64,
            ConvertInputFormat::U128 => Self::U128,
            ConvertInputFormat::I8 => Self::I8,
            ConvertInputFormat::I16 => Self::I16,
            ConvertInputFormat::I32 => Self::I32,
            ConvertInputFormat::I64 => Self::I64,
            ConvertInputFormat::I128 => Self::I128,
            ConvertInputFormat::Lamports => Self::Lamports,
            ConvertInputFormat::Sol => Self::Sol,
        }
    }
}

impl From<ConvertOutputFormat> for OutputFormat {
    fn from(value: ConvertOutputFormat) -> Self {
        match value {
            ConvertOutputFormat::Int => Self::Int,
            ConvertOutputFormat::Hex => Self::Hex,
            ConvertOutputFormat::HexBytes => Self::HexBytes,
            ConvertOutputFormat::Bytes => Self::Bytes,
            ConvertOutputFormat::Text => Self::Text,
            ConvertOutputFormat::Binary => Self::Binary,
            ConvertOutputFormat::Base64 => Self::Base64,
            ConvertOutputFormat::Base58 => Self::Base58,
            ConvertOutputFormat::Pubkey => Self::Pubkey,
            ConvertOutputFormat::Signature => Self::Signature,
            ConvertOutputFormat::U8 => Self::U8,
            ConvertOutputFormat::U16 => Self::U16,
            ConvertOutputFormat::U32 => Self::U32,
            ConvertOutputFormat::U64 => Self::U64,
            ConvertOutputFormat::U128 => Self::U128,
            ConvertOutputFormat::I8 => Self::I8,
            ConvertOutputFormat::I16 => Self::I16,
            ConvertOutputFormat::I32 => Self::I32,
            ConvertOutputFormat::I64 => Self::I64,
            ConvertOutputFormat::I128 => Self::I128,
            ConvertOutputFormat::Lamports => Self::Lamports,
            ConvertOutputFormat::Sol => Self::Sol,
        }
    }
}

impl ConvertArgs {
    fn to_request(&self) -> ConvertRequest {
        ConvertRequest {
            from: self.from.into(),
            to: self.to.into(),
            input: self.input.clone(),
            le: self.le,
            sep: self.sep.clone(),
            no_prefix: self.no_prefix,
            escape: self.escape,
        }
    }
}

pub fn convert(args: &ConvertArgs) -> Result<String, String> {
    converters::convert(&args.to_request())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse_convert_args(argv: &[&str]) -> ConvertArgs {
        let cli = crate::cli::Cli::try_parse_from(argv).unwrap();
        match cli.command {
            Some(crate::cli::Commands::Convert(args)) => args,
            _ => panic!("expected convert command"),
        }
    }

    #[test]
    fn cli_parses_three_positionals() {
        let cli =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "hex", "int", "0x123"]).unwrap();
        match cli.command {
            Some(crate::cli::Commands::Convert(args)) => {
                assert_eq!(args.from, ConvertInputFormat::Hex);
                assert_eq!(args.to, ConvertOutputFormat::Int);
                assert_eq!(args.input.as_deref(), Some("0x123"));
            }
            _ => panic!("expected convert command"),
        }
    }

    #[test]
    fn cli_allows_missing_input_for_stdin() {
        let cli = crate::cli::Cli::try_parse_from(["sonar", "convert", "hex", "int"]).unwrap();
        match cli.command {
            Some(crate::cli::Commands::Convert(args)) => {
                assert_eq!(args.from, ConvertInputFormat::Hex);
                assert_eq!(args.to, ConvertOutputFormat::Int);
                assert_eq!(args.input, None);
            }
            _ => panic!("expected convert command"),
        }
    }

    #[test]
    fn cli_rejects_removed_from_flag() {
        let err =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "-f", "hex", "int", "0x123"])
                .unwrap_err();
        assert!(err.to_string().contains("unexpected argument '-f'"));
    }

    #[test]
    fn cli_rejects_removed_to_flag() {
        let err =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "hex", "int", "0x123", "-t"])
                .unwrap_err();
        assert!(err.to_string().contains("unexpected argument '-t'"));
    }

    #[test]
    fn cli_rejects_removed_hex_array_name() {
        let err =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "hex", "hex-array", "0x1234"])
                .unwrap_err();
        assert!(err.to_string().contains("invalid value 'hex-array'"));
    }

    #[test]
    fn cli_rejects_removed_number_alias() {
        let err = crate::cli::Cli::try_parse_from(["sonar", "convert", "number", "hex", "255"])
            .unwrap_err();
        assert!(err.to_string().contains("invalid value 'number'"));
    }

    #[test]
    fn cli_rejects_removed_utf8_alias() {
        let err =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "hex", "utf8", "0x48656c6c6f"])
                .unwrap_err();
        assert!(err.to_string().contains("invalid value 'utf8'"));
    }

    #[test]
    fn cli_rejects_removed_dec_array_alias() {
        let err =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "hex", "dec-array", "0x1234"])
                .unwrap_err();
        assert!(err.to_string().contains("invalid value 'dec-array'"));
    }

    #[test]
    fn cli_accepts_kept_scheme_b_aliases() {
        let cli =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "hb", "lam", "[0x01]"]).unwrap();
        match cli.command {
            Some(crate::cli::Commands::Convert(args)) => {
                assert_eq!(args.from, ConvertInputFormat::HexBytes);
                assert_eq!(args.to, ConvertOutputFormat::Lamports);
                assert_eq!(args.input.as_deref(), Some("[0x01]"));
            }
            _ => panic!("expected convert command"),
        }
    }

    #[test]
    fn cli_accepts_keypair_kp_alias() {
        let cli =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "kp", "pubkey", "0x00"]).unwrap();
        match cli.command {
            Some(crate::cli::Commands::Convert(args)) => {
                assert_eq!(args.from, ConvertInputFormat::Keypair);
                assert_eq!(args.to, ConvertOutputFormat::Pubkey);
            }
            _ => panic!("expected convert command"),
        }
    }

    #[test]
    fn cli_accepts_binary_and_bin_alias() {
        let full =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "hex", "binary", "0x01"]).unwrap();
        match full.command {
            Some(crate::cli::Commands::Convert(args)) => {
                assert_eq!(args.to, ConvertOutputFormat::Binary);
            }
            _ => panic!("expected convert command"),
        }

        let alias =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "hex", "bin", "0x01"]).unwrap();
        match alias.command {
            Some(crate::cli::Commands::Convert(args)) => {
                assert_eq!(args.to, ConvertOutputFormat::Binary);
            }
            _ => panic!("expected convert command"),
        }
    }

    #[test]
    fn cli_rejects_removed_short_aliases() {
        let cases = [
            ["sonar", "convert", "h", "int", "0x12"],
            ["sonar", "convert", "num", "hex", "12"],
            ["sonar", "convert", "hex", "u", "0x12"],
            ["sonar", "convert", "hex", "da", "0x12"],
            ["sonar", "convert", "ha", "hex", "[0x12]"],
            ["sonar", "convert", "x", "hex", "[0x12]"],
        ];

        for args in cases {
            let err = crate::cli::Cli::try_parse_from(args).unwrap_err();
            assert!(err.to_string().contains("invalid value"));
        }
    }

    #[test]
    fn cli_accepts_new_top3_formats() {
        let cases: Vec<Vec<&str>> = vec![
            vec!["sonar", "convert", "pubkey", "hex", "11111111111111111111111111111111"],
            vec![
                "sonar",
                "convert",
                "signature",
                "bytes",
                "3PtGYH77LhhQqTXP4SmDVJ85hmDieWsgXCUbn14v7gYyVYPjZzygUQhTk3bSTYnfA48vCM1rmWY7zWL3j1EVKmEy",
            ],
            vec![
                "sonar",
                "convert",
                "keypair",
                "pubkey",
                "0x01010101010101010101010101010101010101010101010101010101010101010000000000000000000000000000000000000000000000000000000000000000",
            ],
            vec![
                "sonar",
                "convert",
                "kp",
                "pubkey",
                "0x01010101010101010101010101010101010101010101010101010101010101010000000000000000000000000000000000000000000000000000000000000000",
            ],
            vec!["sonar", "convert", "u8", "hex", "255"],
            vec!["sonar", "convert", "u16", "hex", "65535"],
            vec!["sonar", "convert", "u32", "hex", "4294967295"],
            vec!["sonar", "convert", "u64", "hex", "18446744073709551615"],
            vec!["sonar", "convert", "u128", "hex", "340282366920938463463374607431768211455"],
            vec!["sonar", "convert", "i8", "hex", "-128"],
            vec!["sonar", "convert", "i16", "hex", "-32768"],
            vec!["sonar", "convert", "i32", "hex", "-2147483648"],
            vec!["sonar", "convert", "i64", "hex", "-9223372036854775808"],
            vec!["sonar", "convert", "i128", "hex", "-170141183460469231731687303715884105728"],
            vec![
                "sonar",
                "convert",
                "hex",
                "pubkey",
                "0x0000000000000000000000000000000000000000000000000000000000000000",
            ],
            vec![
                "sonar",
                "convert",
                "hex",
                "signature",
                "0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            ],
            vec!["sonar", "convert", "hex", "u8", "0xff"],
            vec!["sonar", "convert", "hex", "i8", "0xff"],
            vec!["sonar", "convert", "hex", "binary", "0xff"],
            vec!["sonar", "convert", "hex", "bin", "0xff"],
        ];

        for args in cases {
            let cli = crate::cli::Cli::try_parse_from(args).unwrap();
            match cli.command {
                Some(crate::cli::Commands::Convert(_)) => {}
                _ => panic!("expected convert command"),
            }
        }
    }

    #[test]
    fn cli_accepts_negative_input_without_separator() {
        let args = parse_convert_args(&["sonar", "convert", "i16", "hex", "-32768"]);
        assert_eq!(args.input.as_deref(), Some("-32768"));

        let args = parse_convert_args(&["sonar", "convert", "sol", "lamports", "-1.5"]);
        let err = convert(&args).unwrap_err();
        assert!(err.contains("SOL amount cannot be negative"));
    }

    #[test]
    fn cli_accepts_binary_input_format() {
        let parsed = parse_convert_args(&["sonar", "convert", "binary", "hex", "0b01001000"]);
        assert_eq!(parsed.from, ConvertInputFormat::Binary);
        assert_eq!(convert(&parsed).unwrap(), "0x48");
    }

    #[test]
    fn cli_accepts_bin_alias_as_input() {
        let parsed = parse_convert_args(&["sonar", "convert", "bin", "hex", "0b11111111"]);
        assert_eq!(parsed.from, ConvertInputFormat::Binary);
        assert_eq!(convert(&parsed).unwrap(), "0xff");
    }
}
