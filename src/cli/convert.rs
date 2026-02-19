//! Explicit data format conversion utilities.

use base64::Engine;
use clap::{Args, ValueEnum};

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
    /// Base58 encoded string, e.g. Solana pubkey (alias: b58)
    #[value(alias = "b58")]
    Base58,
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
    /// Base64 output (alias: b64)
    #[value(alias = "b64")]
    Base64,
    /// Base58 output (alias: b58)
    #[value(alias = "b58")]
    Base58,
    /// Lamports output (alias: lam)
    #[value(alias = "lam")]
    Lamports,
    /// SOL output
    Sol,
}

#[derive(Args, Debug)]
pub struct ConvertArgs {
    /// Input format
    #[arg(value_name = "FROM", index = 1)]
    pub from: ConvertInputFormat,

    /// Output format
    #[arg(value_name = "TO", index = 2)]
    pub to: ConvertOutputFormat,

    /// Input value
    #[arg(value_name = "INPUT", index = 3)]
    pub input: String,

    /// Use little-endian byte order; default is big-endian
    #[arg(long)]
    pub le: bool,

    /// Separator for array outputs (single character, default: ",")
    #[arg(long, value_name = "CHAR", default_value = ",")]
    pub sep: String,

    /// Disable 0x prefix in hex-bytes output
    #[arg(long)]
    pub no_prefix: bool,

    /// Show invalid text bytes as \xNN escape sequences (for text output)
    #[arg(short = 'e', long)]
    pub escape: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ByteFormat {
    Hex,
    HexBytes,
    Bytes,
}

#[derive(Clone, Debug)]
enum ConvertValue {
    Bytes(Vec<u8>),
    Number(num_bigint::BigUint),
    Lamports(u64),
}

/// 1 SOL = 1_000_000_000 lamports.
const LAMPORTS_PER_SOL: u64 = 1_000_000_000;
const LAMPORTS_PER_SOL_U128: u128 = 1_000_000_000;

/// Parse byte-oriented input.
fn parse_bytes_input(input: &str, format_hint: Option<ByteFormat>) -> Result<Vec<u8>, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Input cannot be empty".to_string());
    }

    if input.starts_with("0x") || input.starts_with("0X") {
        let hex_str: String = input[2..].chars().filter(|c| !c.is_whitespace()).collect();
        if hex_str.is_empty() {
            return Err("Hex string cannot be empty after 0x prefix".to_string());
        }
        let hex_str =
            if !hex_str.len().is_multiple_of(2) { format!("0{}", hex_str) } else { hex_str };
        return hex::decode(hex_str).map_err(|e| format!("Invalid hex string: {}", e));
    }

    if input.starts_with('[') && input.ends_with(']') {
        let inner = input[1..input.len() - 1].trim();
        if inner.is_empty() {
            return Ok(Vec::new());
        }

        let elements: Vec<&str> = if inner.contains(',') {
            inner.split(',').collect()
        } else {
            inner.split_whitespace().collect()
        };

        let force_hex = matches!(format_hint, Some(ByteFormat::HexBytes));
        let mut bytes = Vec::new();

        for element in elements {
            let element = element.trim();
            if element.is_empty() {
                continue;
            }

            let value = if element.starts_with("0x") || element.starts_with("0X") {
                let hex_str = &element[2..];
                u64::from_str_radix(hex_str, 16)
                    .map_err(|e| format!("Invalid hex value '{}': {}", element, e))?
            } else if force_hex {
                u64::from_str_radix(element, 16)
                    .map_err(|e| format!("Invalid hex value '{}': {}", element, e))?
            } else {
                element
                    .parse::<u64>()
                    .map_err(|e| format!("Invalid decimal value '{}': {}", element, e))?
            };

            if value > u8::MAX as u64 {
                return Err(format!("Byte value {} exceeds 255", value));
            }
            bytes.push(value as u8);
        }

        return Ok(bytes);
    }

    if matches!(format_hint, Some(ByteFormat::Hex)) {
        let hex_str: String = input.chars().filter(|c| !c.is_whitespace()).collect();
        if hex_str.is_empty() {
            return Err("Hex string cannot be empty".to_string());
        }
        let hex_str =
            if !hex_str.len().is_multiple_of(2) { format!("0{}", hex_str) } else { hex_str };
        return hex::decode(hex_str).map_err(|e| format!("Invalid hex string: {}", e));
    }

    Err("Invalid input format. Expected hex string (0x...) or byte array ([...])".to_string())
}

fn parse_number(input: &str) -> Result<num_bigint::BigUint, String> {
    use num_bigint::BigUint;

    let input = input.trim();
    if input.is_empty() {
        return Err("Integer cannot be empty".to_string());
    }

    if input.starts_with("0x") || input.starts_with("0X") {
        let hex_str: String = input[2..].chars().filter(|c| !c.is_whitespace()).collect();
        if hex_str.is_empty() {
            return Err("Hex integer cannot be empty after 0x prefix".to_string());
        }
        BigUint::parse_bytes(hex_str.as_bytes(), 16)
            .ok_or_else(|| format!("Invalid hex integer: {}", input))
    } else {
        let dec_str: String = input.chars().filter(|c| !c.is_whitespace()).collect();
        BigUint::parse_bytes(dec_str.as_bytes(), 10)
            .ok_or_else(|| format!("Invalid decimal integer: {}", input))
    }
}

fn parse_sol_to_lamports(input: &str) -> Result<u64, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("SOL amount cannot be empty".to_string());
    }
    if input.starts_with('-') {
        return Err("SOL amount cannot be negative".to_string());
    }
    if input.starts_with('+') {
        return Err("SOL amount must not use '+' sign".to_string());
    }

    let parts: Vec<&str> = input.split('.').collect();
    if parts.len() > 2 {
        return Err(format!("Invalid SOL value '{}'", input));
    }

    let int_part_raw = parts[0];
    let frac_part_raw = if parts.len() == 2 { parts[1] } else { "" };

    if !int_part_raw.is_empty() && !int_part_raw.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("Invalid SOL value '{}'", input));
    }
    if !frac_part_raw.is_empty() && !frac_part_raw.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("Invalid SOL value '{}'", input));
    }
    if parts.len() == 2 && int_part_raw.is_empty() && frac_part_raw.is_empty() {
        return Err(format!("Invalid SOL value '{}'", input));
    }
    if frac_part_raw.len() > 9 {
        return Err("SOL supports up to 9 decimal places".to_string());
    }

    let int_part = if int_part_raw.is_empty() {
        0u128
    } else {
        int_part_raw
            .parse::<u128>()
            .map_err(|e| format!("Invalid SOL integer part '{}': {}", int_part_raw, e))?
    };

    let mut frac_scaled = 0u128;
    if !frac_part_raw.is_empty() {
        let frac_digits = frac_part_raw
            .parse::<u128>()
            .map_err(|e| format!("Invalid SOL fraction part '{}': {}", frac_part_raw, e))?;
        let scale = 10u128.pow(9 - frac_part_raw.len() as u32);
        frac_scaled = frac_digits * scale;
    }

    let lamports = int_part
        .checked_mul(LAMPORTS_PER_SOL_U128)
        .and_then(|v| v.checked_add(frac_scaled))
        .ok_or_else(|| "SOL value overflows lamports range".to_string())?;

    u64::try_from(lamports).map_err(|_| "SOL value overflows u64 lamports".to_string())
}

fn parse_input_with_format(
    input: &str,
    format: ConvertInputFormat,
) -> Result<ConvertValue, String> {
    match format {
        ConvertInputFormat::Int => Ok(ConvertValue::Number(parse_number(input)?)),
        ConvertInputFormat::Hex => {
            Ok(ConvertValue::Bytes(parse_bytes_input(input, Some(ByteFormat::Hex))?))
        }
        ConvertInputFormat::HexBytes => {
            Ok(ConvertValue::Bytes(parse_bytes_input(input, Some(ByteFormat::HexBytes))?))
        }
        ConvertInputFormat::Bytes => {
            Ok(ConvertValue::Bytes(parse_bytes_input(input, Some(ByteFormat::Bytes))?))
        }
        ConvertInputFormat::Text => Ok(ConvertValue::Bytes(input.as_bytes().to_vec())),
        ConvertInputFormat::Base64 => {
            let value = base64::engine::general_purpose::STANDARD
                .decode(input)
                .map_err(|e| format!("Invalid base64 input: {}", e))?;
            Ok(ConvertValue::Bytes(value))
        }
        ConvertInputFormat::Base58 => {
            let value = bs58::decode(input)
                .into_vec()
                .map_err(|e| format!("Invalid base58 input: {}", e))?;
            Ok(ConvertValue::Bytes(value))
        }
        ConvertInputFormat::Lamports => {
            let trimmed = input.trim();
            let lamports = trimmed
                .parse::<u64>()
                .map_err(|e| format!("Invalid lamports value '{}': {}", trimmed, e))?;
            Ok(ConvertValue::Lamports(lamports))
        }
        ConvertInputFormat::Sol => Ok(ConvertValue::Lamports(parse_sol_to_lamports(input)?)),
    }
}

fn value_to_bytes(value: &ConvertValue, big_endian: bool) -> Vec<u8> {
    match value {
        ConvertValue::Bytes(bytes) => bytes.clone(),
        ConvertValue::Number(num) => {
            if big_endian {
                num.to_bytes_be()
            } else {
                num.to_bytes_le()
            }
        }
        ConvertValue::Lamports(lamports) => {
            if big_endian {
                lamports.to_be_bytes().to_vec()
            } else {
                lamports.to_le_bytes().to_vec()
            }
        }
    }
}

fn bytes_to_u64(bytes: &[u8], big_endian: bool) -> u64 {
    if bytes.is_empty() {
        return 0;
    }

    let mut buf = [0u8; 8];
    let len = bytes.len().min(8);
    if big_endian {
        buf[8 - len..].copy_from_slice(&bytes[..len]);
        u64::from_be_bytes(buf)
    } else {
        buf[..len].copy_from_slice(&bytes[..len]);
        u64::from_le_bytes(buf)
    }
}

fn format_sol(lamports: u64) -> String {
    if lamports == 0 {
        return "0".to_string();
    }

    let integer = lamports / LAMPORTS_PER_SOL;
    let fraction = lamports % LAMPORTS_PER_SOL;
    if fraction == 0 {
        return integer.to_string();
    }

    let mut frac_str = format!("{:09}", fraction);
    while frac_str.ends_with('0') {
        frac_str.pop();
    }
    format!("{}.{}", integer, frac_str)
}

fn format_bytes(bytes: &[u8], format: ByteFormat, separator: &str, with_prefix: bool) -> String {
    match format {
        ByteFormat::Hex => {
            if bytes.is_empty() {
                "0x0".to_string()
            } else {
                format!("0x{}", hex::encode(bytes))
            }
        }
        ByteFormat::HexBytes => {
            let elements: Vec<String> = if with_prefix {
                bytes.iter().map(|b| format!("0x{:02x}", b)).collect()
            } else {
                bytes.iter().map(|b| format!("{:02x}", b)).collect()
            };
            format!("[{}]", elements.join(separator))
        }
        ByteFormat::Bytes => {
            let elements: Vec<String> = bytes.iter().map(|b| b.to_string()).collect();
            format!("[{}]", elements.join(separator))
        }
    }
}

fn bytes_to_utf8(bytes: &[u8], escape_invalid: bool) -> String {
    if !escape_invalid {
        return String::from_utf8_lossy(bytes).into_owned();
    }

    let mut result = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let remaining = &bytes[i..];
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                result.push_str(valid);
                break;
            }
            Err(err) => {
                let valid_up_to = err.valid_up_to();
                if valid_up_to > 0 {
                    let valid = std::str::from_utf8(&remaining[..valid_up_to])
                        .expect("valid_up_to indicates UTF-8 valid segment");
                    result.push_str(valid);
                    i += valid_up_to;
                } else {
                    result.push_str(&format!("\\x{:02x}", bytes[i]));
                    i += 1;
                }
            }
        }
    }
    result
}

fn format_target(
    value: &ConvertValue,
    target: ConvertOutputFormat,
    big_endian: bool,
    separator: &str,
    hex_array_with_prefix: bool,
    escape_text: bool,
) -> String {
    match target {
        ConvertOutputFormat::Lamports => {
            let lamports = match value {
                ConvertValue::Lamports(v) => *v,
                _ => bytes_to_u64(&value_to_bytes(value, big_endian), big_endian),
            };
            lamports.to_string()
        }
        ConvertOutputFormat::Sol => {
            let lamports = match value {
                ConvertValue::Lamports(v) => *v,
                _ => bytes_to_u64(&value_to_bytes(value, big_endian), big_endian),
            };
            format_sol(lamports)
        }
        ConvertOutputFormat::Int => {
            use num_bigint::BigUint;
            let bytes = value_to_bytes(value, big_endian);
            let num = if big_endian {
                BigUint::from_bytes_be(&bytes)
            } else {
                BigUint::from_bytes_le(&bytes)
            };
            num.to_string()
        }
        ConvertOutputFormat::Hex => {
            let bytes = value_to_bytes(value, big_endian);
            format_bytes(&bytes, ByteFormat::Hex, separator, hex_array_with_prefix)
        }
        ConvertOutputFormat::HexBytes => {
            let bytes = value_to_bytes(value, big_endian);
            format_bytes(&bytes, ByteFormat::HexBytes, separator, hex_array_with_prefix)
        }
        ConvertOutputFormat::Bytes => {
            let bytes = value_to_bytes(value, big_endian);
            format_bytes(&bytes, ByteFormat::Bytes, separator, hex_array_with_prefix)
        }
        ConvertOutputFormat::Text => {
            let bytes = value_to_bytes(value, big_endian);
            bytes_to_utf8(&bytes, escape_text)
        }
        ConvertOutputFormat::Base64 => {
            let bytes = value_to_bytes(value, big_endian);
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        }
        ConvertOutputFormat::Base58 => {
            let bytes = value_to_bytes(value, big_endian);
            bs58::encode(&bytes).into_string()
        }
    }
}

fn normalize_separator(raw: &str) -> Result<&str, String> {
    if raw.chars().count() != 1 {
        return Err("--sep expects exactly one character".to_string());
    }
    Ok(raw)
}

/// Perform the complete conversion from input to output.
pub fn convert(args: &ConvertArgs) -> Result<String, String> {
    let separator = normalize_separator(&args.sep)?;
    let big_endian = !args.le;
    let value = parse_input_with_format(&args.input, args.from)?;
    Ok(format_target(&value, args.to, big_endian, separator, !args.no_prefix, args.escape))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(from: ConvertInputFormat, input: &str, to: ConvertOutputFormat) -> ConvertArgs {
        ConvertArgs {
            from,
            to,
            input: input.to_string(),
            le: false,
            sep: ",".to_string(),
            no_prefix: false,
            escape: false,
        }
    }

    #[test]
    fn convert_hex_to_text() {
        let output =
            convert(&args(ConvertInputFormat::Hex, "0x48656c6c6f", ConvertOutputFormat::Text))
                .unwrap();
        assert_eq!(output, "Hello");
    }

    #[test]
    fn convert_int_to_hex_default_be() {
        let output =
            convert(&args(ConvertInputFormat::Int, "305419896", ConvertOutputFormat::Hex)).unwrap();
        assert_eq!(output, "0x12345678");
    }

    #[test]
    fn convert_int_to_hex_le() {
        let mut value = args(ConvertInputFormat::Int, "305419896", ConvertOutputFormat::Hex);
        value.le = true;
        let output = convert(&value).unwrap();
        assert_eq!(output, "0x78563412");
    }

    #[test]
    fn convert_sol_to_lamports() {
        let output =
            convert(&args(ConvertInputFormat::Sol, "1.5", ConvertOutputFormat::Lamports)).unwrap();
        assert_eq!(output, "1500000000");
    }

    #[test]
    fn convert_lamports_to_sol() {
        let output =
            convert(&args(ConvertInputFormat::Lamports, "1500000000", ConvertOutputFormat::Sol))
                .unwrap();
        assert_eq!(output, "1.5");
    }

    #[test]
    fn convert_hex_bytes_with_no_prefix() {
        let mut value = args(ConvertInputFormat::Hex, "0x123456", ConvertOutputFormat::HexBytes);
        value.no_prefix = true;
        let output = convert(&value).unwrap();
        assert_eq!(output, "[12,34,56]");
    }

    #[test]
    fn convert_bytes_separator_space() {
        let mut value = args(ConvertInputFormat::Hex, "0x123456", ConvertOutputFormat::Bytes);
        value.sep = " ".to_string();
        let output = convert(&value).unwrap();
        assert_eq!(output, "[18 52 86]");
    }

    #[test]
    fn convert_rejects_invalid_separator() {
        let mut value = args(ConvertInputFormat::Hex, "0x1234", ConvertOutputFormat::Bytes);
        value.sep = "::".to_string();
        let err = convert(&value).unwrap_err();
        assert!(err.contains("exactly one character"));
    }

    #[test]
    fn convert_text_escape_invalid() {
        let mut value = args(ConvertInputFormat::Hex, "0xff", ConvertOutputFormat::Text);
        value.escape = true;
        let output = convert(&value).unwrap();
        assert_eq!(output, "\\xff");
    }

    #[test]
    fn parse_sol_to_lamports_precision() {
        assert_eq!(parse_sol_to_lamports("1").unwrap(), 1_000_000_000);
        assert_eq!(parse_sol_to_lamports(".5").unwrap(), 500_000_000);
        assert_eq!(parse_sol_to_lamports("1.23456789").unwrap(), 1_234_567_890);
        assert_eq!(parse_sol_to_lamports("0.000000001").unwrap(), 1);
    }

    #[test]
    fn parse_sol_to_lamports_rejects_too_many_decimals() {
        let err = parse_sol_to_lamports("1.0000000001").unwrap_err();
        assert!(err.contains("up to 9"));
    }

    #[test]
    fn format_sol_trim_zeros() {
        assert_eq!(format_sol(0), "0");
        assert_eq!(format_sol(1_000_000_000), "1");
        assert_eq!(format_sol(1_500_000_000), "1.5");
        assert_eq!(format_sol(1_234_567_890), "1.23456789");
    }

    #[test]
    fn cli_parses_three_positionals() {
        use clap::Parser;

        let cli =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "hex", "int", "0x123"]).unwrap();
        match cli.command {
            crate::cli::Commands::Convert(args) => {
                assert_eq!(args.from, ConvertInputFormat::Hex);
                assert_eq!(args.to, ConvertOutputFormat::Int);
                assert_eq!(args.input, "0x123");
            }
            _ => panic!("expected convert command"),
        }
    }

    #[test]
    fn cli_rejects_missing_input() {
        use clap::Parser;

        let err = crate::cli::Cli::try_parse_from(["sonar", "convert", "hex", "int"]).unwrap_err();
        assert!(err.to_string().contains("<INPUT>"));
    }

    #[test]
    fn cli_rejects_removed_from_flag() {
        use clap::Parser;

        let err =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "-f", "hex", "int", "0x123"])
                .unwrap_err();
        assert!(err.to_string().contains("unexpected argument '-f'"));
    }

    #[test]
    fn cli_rejects_removed_to_flag() {
        use clap::Parser;

        let err =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "hex", "int", "0x123", "-t"])
                .unwrap_err();
        assert!(err.to_string().contains("unexpected argument '-t'"));
    }

    #[test]
    fn cli_rejects_removed_hex_array_name() {
        use clap::Parser;

        let err =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "hex", "hex-array", "0x1234"])
                .unwrap_err();
        assert!(err.to_string().contains("invalid value 'hex-array'"));
    }

    #[test]
    fn cli_rejects_removed_number_alias() {
        use clap::Parser;

        let err = crate::cli::Cli::try_parse_from(["sonar", "convert", "number", "hex", "255"])
            .unwrap_err();
        assert!(err.to_string().contains("invalid value 'number'"));
    }

    #[test]
    fn cli_rejects_removed_utf8_alias() {
        use clap::Parser;

        let err =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "hex", "utf8", "0x48656c6c6f"])
                .unwrap_err();
        assert!(err.to_string().contains("invalid value 'utf8'"));
    }

    #[test]
    fn cli_rejects_removed_dec_array_alias() {
        use clap::Parser;

        let err =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "hex", "dec-array", "0x1234"])
                .unwrap_err();
        assert!(err.to_string().contains("invalid value 'dec-array'"));
    }

    #[test]
    fn cli_accepts_kept_scheme_b_aliases() {
        use clap::Parser;

        let cli =
            crate::cli::Cli::try_parse_from(["sonar", "convert", "hb", "lam", "[0x01]"]).unwrap();
        match cli.command {
            crate::cli::Commands::Convert(args) => {
                assert_eq!(args.from, ConvertInputFormat::HexBytes);
                assert_eq!(args.to, ConvertOutputFormat::Lamports);
                assert_eq!(args.input, "[0x01]");
            }
            _ => panic!("expected convert command"),
        }
    }

    #[test]
    fn cli_rejects_removed_short_aliases() {
        use clap::Parser;

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
}
