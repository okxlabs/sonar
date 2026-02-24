//! Explicit data format conversion utilities.

use std::{
    io::{IsTerminal, Read},
    str::FromStr,
};

use base64::Engine;
use clap::{Args, ValueEnum};
use solana_pubkey::Pubkey;
use solana_signature::Signature;

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
  Generic:  int, hex, hex-bytes (hb), bytes, text, base64 (b64), base58 (b58), binary (bin, output only)
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
    #[arg(value_name = "INPUT", index = 3, required = false)]
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
    FixedUnsigned { value: u128, bits: u16 },
    FixedSigned { value: i128, bits: u16 },
    Lamports(u64),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum FixedIntSpec {
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
}

impl FixedIntSpec {
    fn bits(self) -> u16 {
        match self {
            Self::U8 | Self::I8 => 8,
            Self::U16 | Self::I16 => 16,
            Self::U32 | Self::I32 => 32,
            Self::U64 | Self::I64 => 64,
            Self::U128 | Self::I128 => 128,
        }
    }

    fn bytes(self) -> usize {
        usize::from(self.bits() / 8)
    }

    fn is_signed(self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::I128)
    }

    fn name(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::U128 => "u128",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::I128 => "i128",
        }
    }
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

fn input_fixed_int_spec(format: ConvertInputFormat) -> Option<FixedIntSpec> {
    match format {
        ConvertInputFormat::U8 => Some(FixedIntSpec::U8),
        ConvertInputFormat::U16 => Some(FixedIntSpec::U16),
        ConvertInputFormat::U32 => Some(FixedIntSpec::U32),
        ConvertInputFormat::U64 => Some(FixedIntSpec::U64),
        ConvertInputFormat::U128 => Some(FixedIntSpec::U128),
        ConvertInputFormat::I8 => Some(FixedIntSpec::I8),
        ConvertInputFormat::I16 => Some(FixedIntSpec::I16),
        ConvertInputFormat::I32 => Some(FixedIntSpec::I32),
        ConvertInputFormat::I64 => Some(FixedIntSpec::I64),
        ConvertInputFormat::I128 => Some(FixedIntSpec::I128),
        _ => None,
    }
}

fn output_fixed_int_spec(format: ConvertOutputFormat) -> Option<FixedIntSpec> {
    match format {
        ConvertOutputFormat::U8 => Some(FixedIntSpec::U8),
        ConvertOutputFormat::U16 => Some(FixedIntSpec::U16),
        ConvertOutputFormat::U32 => Some(FixedIntSpec::U32),
        ConvertOutputFormat::U64 => Some(FixedIntSpec::U64),
        ConvertOutputFormat::U128 => Some(FixedIntSpec::U128),
        ConvertOutputFormat::I8 => Some(FixedIntSpec::I8),
        ConvertOutputFormat::I16 => Some(FixedIntSpec::I16),
        ConvertOutputFormat::I32 => Some(FixedIntSpec::I32),
        ConvertOutputFormat::I64 => Some(FixedIntSpec::I64),
        ConvertOutputFormat::I128 => Some(FixedIntSpec::I128),
        _ => None,
    }
}

fn unsigned_max(bits: u16) -> u128 {
    if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 }
}

fn signed_bounds(bits: u16) -> (i128, i128) {
    if bits == 128 {
        (i128::MIN, i128::MAX)
    } else {
        let max = (1i128 << (bits - 1)) - 1;
        let min = -(1i128 << (bits - 1));
        (min, max)
    }
}

fn parse_fixed_integer(input: &str, spec: FixedIntSpec) -> Result<ConvertValue, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(format!("{} cannot be empty", spec.name()));
    }

    if spec.is_signed() {
        let value = trimmed
            .parse::<i128>()
            .map_err(|e| format!("Invalid {} value '{}': {}", spec.name(), trimmed, e))?;
        let (min, max) = signed_bounds(spec.bits());
        if value < min || value > max {
            return Err(format!("{} value {} is out of range", spec.name(), value));
        }
        Ok(ConvertValue::FixedSigned { value, bits: spec.bits() })
    } else {
        let value = trimmed
            .parse::<u128>()
            .map_err(|e| format!("Invalid {} value '{}': {}", spec.name(), trimmed, e))?;
        if value > unsigned_max(spec.bits()) {
            return Err(format!("{} value {} is out of range", spec.name(), value));
        }
        Ok(ConvertValue::FixedUnsigned { value, bits: spec.bits() })
    }
}

fn parse_input_with_format(
    input: &str,
    format: ConvertInputFormat,
) -> Result<ConvertValue, String> {
    if let Some(spec) = input_fixed_int_spec(format) {
        return parse_fixed_integer(input, spec);
    }

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
                .map_err(|e| format!("Invalid base64 input: {}", format_base64_error(&e)))?;
            Ok(ConvertValue::Bytes(value))
        }
        ConvertInputFormat::Base58 => {
            let value = bs58::decode(input)
                .into_vec()
                .map_err(|e| format!("Invalid base58 input: {}", e))?;
            Ok(ConvertValue::Bytes(value))
        }
        ConvertInputFormat::Pubkey => {
            let trimmed = input.trim();
            let pubkey = Pubkey::from_str(trimmed)
                .map_err(|e| format!("Invalid pubkey '{}': {}", trimmed, e))?;
            Ok(ConvertValue::Bytes(pubkey.to_bytes().to_vec()))
        }
        ConvertInputFormat::Signature => {
            let trimmed = input.trim();
            let signature = Signature::from_str(trimmed)
                .map_err(|e| format!("Invalid signature '{}': {}", trimmed, e))?;
            Ok(ConvertValue::Bytes(signature.as_ref().to_vec()))
        }
        ConvertInputFormat::Keypair => {
            let bytes = parse_bytes_input(input, Some(ByteFormat::Bytes))?;
            if bytes.len() != 64 {
                return Err(format!("keypair requires exactly 64 bytes, got {}", bytes.len()));
            }
            Ok(ConvertValue::Bytes(bytes[32..].to_vec()))
        }
        ConvertInputFormat::Lamports => {
            let trimmed = input.trim();
            let lamports = trimmed
                .parse::<u64>()
                .map_err(|e| format!("Invalid lamports value '{}': {}", trimmed, e))?;
            Ok(ConvertValue::Lamports(lamports))
        }
        ConvertInputFormat::Sol => Ok(ConvertValue::Lamports(parse_sol_to_lamports(input)?)),
        _ => unreachable!("fixed integer formats handled before match"),
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
        ConvertValue::FixedUnsigned { value, bits } => {
            let width = usize::from(bits / 8);
            if big_endian {
                value.to_be_bytes()[16 - width..].to_vec()
            } else {
                value.to_le_bytes()[..width].to_vec()
            }
        }
        ConvertValue::FixedSigned { value, bits } => {
            let width = usize::from(bits / 8);
            if big_endian {
                value.to_be_bytes()[16 - width..].to_vec()
            } else {
                value.to_le_bytes()[..width].to_vec()
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

fn biguint_to_u128(value: &num_bigint::BigUint) -> Option<u128> {
    let bytes = value.to_bytes_be();
    if bytes.len() > 16 {
        return None;
    }
    let mut buf = [0u8; 16];
    buf[16 - bytes.len()..].copy_from_slice(&bytes);
    Some(u128::from_be_bytes(buf))
}

fn decode_fixed_unsigned(bytes: &[u8], big_endian: bool) -> u128 {
    let mut buf = [0u8; 16];
    if big_endian {
        buf[16 - bytes.len()..].copy_from_slice(bytes);
        u128::from_be_bytes(buf)
    } else {
        buf[..bytes.len()].copy_from_slice(bytes);
        u128::from_le_bytes(buf)
    }
}

fn decode_fixed_signed(bytes: &[u8], big_endian: bool) -> i128 {
    let fill = if big_endian {
        if (bytes[0] & 0x80) != 0 { 0xFF } else { 0x00 }
    } else if (bytes[bytes.len() - 1] & 0x80) != 0 {
        0xFF
    } else {
        0x00
    };

    let mut buf = [fill; 16];
    if big_endian {
        buf[16 - bytes.len()..].copy_from_slice(bytes);
        i128::from_be_bytes(buf)
    } else {
        buf[..bytes.len()].copy_from_slice(bytes);
        i128::from_le_bytes(buf)
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

fn format_binary(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "0b0".to_string();
    }

    let bits: String = bytes.iter().map(|b| format!("{:08b}", b)).collect();
    format!("0b{}", bits)
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

fn format_fixed_integer(
    value: &ConvertValue,
    spec: FixedIntSpec,
    big_endian: bool,
) -> Result<String, String> {
    let out_of_range = |value: &str| format!("{} value {} is out of range", spec.name(), value);

    if spec.is_signed() {
        let (min, max) = signed_bounds(spec.bits());
        let signed_value = match value {
            ConvertValue::Bytes(bytes) => {
                if bytes.len() != spec.bytes() {
                    return Err(format!(
                        "{} requires exactly {} bytes, got {}",
                        spec.name(),
                        spec.bytes(),
                        bytes.len()
                    ));
                }
                decode_fixed_signed(bytes, big_endian)
            }
            ConvertValue::Number(num) => {
                let raw = num.to_string();
                let as_u128 = biguint_to_u128(num).ok_or_else(|| out_of_range(&raw))?;
                let max_u128 =
                    u128::try_from(max).expect("signed max should always be non-negative");
                if as_u128 > max_u128 {
                    return Err(out_of_range(&raw));
                }
                as_u128 as i128
            }
            ConvertValue::FixedUnsigned { value, .. } => {
                let max_u128 =
                    u128::try_from(max).expect("signed max should always be non-negative");
                if *value > max_u128 {
                    return Err(out_of_range(&value.to_string()));
                }
                *value as i128
            }
            ConvertValue::FixedSigned { value, .. } => *value,
            ConvertValue::Lamports(value) => {
                let as_u128 = u128::from(*value);
                let max_u128 =
                    u128::try_from(max).expect("signed max should always be non-negative");
                if as_u128 > max_u128 {
                    return Err(out_of_range(&value.to_string()));
                }
                *value as i128
            }
        };

        if signed_value < min || signed_value > max {
            return Err(out_of_range(&signed_value.to_string()));
        }
        Ok(signed_value.to_string())
    } else {
        let max = unsigned_max(spec.bits());
        let unsigned_value = match value {
            ConvertValue::Bytes(bytes) => {
                if bytes.len() != spec.bytes() {
                    return Err(format!(
                        "{} requires exactly {} bytes, got {}",
                        spec.name(),
                        spec.bytes(),
                        bytes.len()
                    ));
                }
                decode_fixed_unsigned(bytes, big_endian)
            }
            ConvertValue::Number(num) => {
                let raw = num.to_string();
                let as_u128 = biguint_to_u128(num).ok_or_else(|| out_of_range(&raw))?;
                if as_u128 > max {
                    return Err(out_of_range(&raw));
                }
                as_u128
            }
            ConvertValue::FixedUnsigned { value, .. } => *value,
            ConvertValue::FixedSigned { value, .. } => {
                if *value < 0 {
                    return Err(out_of_range(&value.to_string()));
                }
                *value as u128
            }
            ConvertValue::Lamports(value) => u128::from(*value),
        };

        if unsigned_value > max {
            return Err(out_of_range(&unsigned_value.to_string()));
        }
        Ok(unsigned_value.to_string())
    }
}

fn format_target(
    value: &ConvertValue,
    target: ConvertOutputFormat,
    big_endian: bool,
    separator: &str,
    hex_array_with_prefix: bool,
    escape_text: bool,
) -> Result<String, String> {
    if let Some(spec) = output_fixed_int_spec(target) {
        return format_fixed_integer(value, spec, big_endian);
    }

    match target {
        ConvertOutputFormat::Lamports => {
            let lamports = match value {
                ConvertValue::Lamports(v) => *v,
                _ => bytes_to_u64(&value_to_bytes(value, big_endian), big_endian),
            };
            Ok(lamports.to_string())
        }
        ConvertOutputFormat::Sol => {
            let lamports = match value {
                ConvertValue::Lamports(v) => *v,
                _ => bytes_to_u64(&value_to_bytes(value, big_endian), big_endian),
            };
            Ok(format_sol(lamports))
        }
        ConvertOutputFormat::Int => {
            use num_bigint::BigUint;
            let bytes = value_to_bytes(value, big_endian);
            let num = if big_endian {
                BigUint::from_bytes_be(&bytes)
            } else {
                BigUint::from_bytes_le(&bytes)
            };
            Ok(num.to_string())
        }
        ConvertOutputFormat::Hex => {
            let bytes = value_to_bytes(value, big_endian);
            Ok(format_bytes(&bytes, ByteFormat::Hex, separator, hex_array_with_prefix))
        }
        ConvertOutputFormat::HexBytes => {
            let bytes = value_to_bytes(value, big_endian);
            Ok(format_bytes(&bytes, ByteFormat::HexBytes, separator, hex_array_with_prefix))
        }
        ConvertOutputFormat::Bytes => {
            let bytes = value_to_bytes(value, big_endian);
            Ok(format_bytes(&bytes, ByteFormat::Bytes, separator, hex_array_with_prefix))
        }
        ConvertOutputFormat::Text => {
            let bytes = value_to_bytes(value, big_endian);
            Ok(bytes_to_utf8(&bytes, escape_text))
        }
        ConvertOutputFormat::Binary => {
            let bytes = value_to_bytes(value, big_endian);
            Ok(format_binary(&bytes))
        }
        ConvertOutputFormat::Base64 => {
            let bytes = value_to_bytes(value, big_endian);
            Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
        }
        ConvertOutputFormat::Base58 => {
            let bytes = value_to_bytes(value, big_endian);
            Ok(bs58::encode(&bytes).into_string())
        }
        ConvertOutputFormat::Pubkey => {
            let bytes = value_to_bytes(value, big_endian);
            if bytes.len() != 32 {
                return Err(format!("pubkey requires exactly 32 bytes, got {}", bytes.len()));
            }
            let bytes: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| "pubkey requires exactly 32 bytes".to_string())?;
            Ok(Pubkey::new_from_array(bytes).to_string())
        }
        ConvertOutputFormat::Signature => {
            let bytes = value_to_bytes(value, big_endian);
            if bytes.len() != 64 {
                return Err(format!("signature requires exactly 64 bytes, got {}", bytes.len()));
            }
            let bytes: [u8; 64] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| "signature requires exactly 64 bytes".to_string())?;
            Ok(Signature::from(bytes).to_string())
        }
        _ => unreachable!("fixed integer formats handled before match"),
    }
}

fn format_base64_error(err: &base64::DecodeError) -> String {
    match err {
        base64::DecodeError::InvalidByte(offset, byte) => {
            let ch = *byte as char;
            if ch.is_ascii_graphic() || ch == ' ' {
                format!("unexpected character '{ch}' at position {offset}")
            } else {
                format!("unexpected byte 0x{byte:02x} at position {offset}")
            }
        }
        base64::DecodeError::InvalidLastSymbol(offset, byte) => {
            let ch = *byte as char;
            if ch.is_ascii_graphic() || ch == ' ' {
                format!("invalid trailing character '{ch}' at position {offset}")
            } else {
                format!("invalid trailing byte 0x{byte:02x} at position {offset}")
            }
        }
        other => other.to_string(),
    }
}

fn normalize_separator(raw: &str) -> Result<&str, String> {
    if raw.chars().count() != 1 {
        return Err("--sep expects exactly one character".to_string());
    }
    Ok(raw)
}

fn read_convert_input(input: Option<&str>) -> Result<String, String> {
    if let Some(input) = input {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err("Input cannot be empty".to_string());
        }
        return Ok(trimmed.to_owned());
    }

    if !std::io::stdin().is_terminal() {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("Failed to read input from stdin: {}", e))?;
        let trimmed = buf.trim();
        if trimmed.is_empty() {
            return Err("No input data received from stdin".to_string());
        }
        return Ok(trimmed.to_owned());
    }

    Err("No input provided. Pass INPUT as a positional argument or pipe via stdin".to_string())
}

/// Perform the complete conversion from input to output.
pub fn convert(args: &ConvertArgs) -> Result<String, String> {
    let separator = normalize_separator(&args.sep)?;
    let big_endian = !args.le;
    let input = read_convert_input(args.input.as_deref())?;
    let value = parse_input_with_format(&input, args.from)?;
    format_target(&value, args.to, big_endian, separator, !args.no_prefix, args.escape)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(from: ConvertInputFormat, input: &str, to: ConvertOutputFormat) -> ConvertArgs {
        ConvertArgs {
            from,
            to,
            input: Some(input.to_string()),
            le: false,
            sep: ",".to_string(),
            no_prefix: false,
            escape: false,
        }
    }

    fn parse_convert_args(argv: &[&str]) -> ConvertArgs {
        use clap::Parser;

        let cli = crate::cli::Cli::try_parse_from(argv).unwrap();
        match cli.command {
            Some(crate::cli::Commands::Convert(args)) => args,
            _ => panic!("expected convert command"),
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
    fn convert_hex_to_binary() {
        let output =
            convert(&args(ConvertInputFormat::Hex, "0x48656c6c6f", ConvertOutputFormat::Binary))
                .unwrap();
        assert_eq!(output, "0b0100100001100101011011000110110001101111");
    }

    #[test]
    fn convert_int_to_binary_default_be() {
        let output =
            convert(&args(ConvertInputFormat::Int, "305419896", ConvertOutputFormat::Binary))
                .unwrap();
        assert_eq!(output, "0b00010010001101000101011001111000");
    }

    #[test]
    fn convert_int_to_binary_le() {
        let mut value = args(ConvertInputFormat::Int, "305419896", ConvertOutputFormat::Binary);
        value.le = true;
        let output = convert(&value).unwrap();
        assert_eq!(output, "0b01111000010101100011010000010010");
    }

    #[test]
    fn convert_zero_to_binary() {
        let output =
            convert(&args(ConvertInputFormat::Int, "0", ConvertOutputFormat::Binary)).unwrap();
        assert_eq!(output, "0b00000000");
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
        use clap::Parser;

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
        use clap::Parser;

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
        use clap::Parser;

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

    #[test]
    fn cli_accepts_new_top3_formats() {
        use clap::Parser;

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
            vec!["sonar", "convert", "i8", "hex", "--", "-128"],
            vec!["sonar", "convert", "i16", "hex", "--", "-32768"],
            vec!["sonar", "convert", "i32", "hex", "--", "-2147483648"],
            vec!["sonar", "convert", "i64", "hex", "--", "-9223372036854775808"],
            vec![
                "sonar",
                "convert",
                "i128",
                "hex",
                "--",
                "-170141183460469231731687303715884105728",
            ],
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
    fn convert_pubkey_hex_roundtrip() {
        let pubkey = "11111111111111111111111111111111";
        let to_hex = parse_convert_args(&["sonar", "convert", "pubkey", "hex", pubkey]);
        let hex = convert(&to_hex).unwrap();
        assert_eq!(hex, "0x0000000000000000000000000000000000000000000000000000000000000000");

        let back = parse_convert_args(&["sonar", "convert", "hex", "pubkey", &hex]);
        let pubkey_back = convert(&back).unwrap();
        assert_eq!(pubkey_back, pubkey);
    }

    #[test]
    fn convert_signature_hex_roundtrip() {
        let signature = "3PtGYH77LhhQqTXP4SmDVJ85hmDieWsgXCUbn14v7gYyVYPjZzygUQhTk3bSTYnfA48vCM1rmWY7zWL3j1EVKmEy";
        let to_hex = parse_convert_args(&["sonar", "convert", "signature", "hex", signature]);
        let hex = convert(&to_hex).unwrap();
        let expected_hex =
            format!("0x{}", hex::encode(bs58::decode(signature).into_vec().unwrap()));
        assert_eq!(hex, expected_hex);

        let back = parse_convert_args(&["sonar", "convert", "hex", "signature", &hex]);
        let signature_back = convert(&back).unwrap();
        assert_eq!(signature_back, signature);
    }

    #[test]
    fn convert_pubkey_requires_exactly_32_bytes() {
        let parsed = parse_convert_args(&[
            "sonar",
            "convert",
            "hex",
            "pubkey",
            "0x01010101010101010101010101010101010101010101010101010101010101",
        ]);
        let err = convert(&parsed).unwrap_err();
        assert!(err.contains("pubkey requires exactly 32 bytes"));
    }

    #[test]
    fn convert_signature_requires_exactly_64_bytes() {
        let parsed = parse_convert_args(&[
            "sonar",
            "convert",
            "hex",
            "signature",
            "0x010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101",
        ]);
        let err = convert(&parsed).unwrap_err();
        assert!(err.contains("signature requires exactly 64 bytes"));
    }

    #[test]
    fn convert_pubkey_rejects_invalid_input() {
        let parsed = parse_convert_args(&["sonar", "convert", "pubkey", "hex", "invalid-pubkey"]);
        let err = convert(&parsed).unwrap_err();
        assert!(err.contains("Invalid pubkey"));
    }

    #[test]
    fn convert_signature_rejects_invalid_input() {
        let parsed =
            parse_convert_args(&["sonar", "convert", "signature", "hex", "invalid-signature"]);
        let err = convert(&parsed).unwrap_err();
        assert!(err.contains("Invalid signature"));
    }

    #[test]
    fn convert_keypair_to_pubkey_from_hex() {
        let keypair_hex = format!("0x{}{}", "01".repeat(32), "00".repeat(32));
        let parsed = parse_convert_args(&["sonar", "convert", "keypair", "pubkey", &keypair_hex]);
        let output = convert(&parsed).unwrap();
        assert_eq!(output, "11111111111111111111111111111111");
    }

    #[test]
    fn convert_keypair_to_pubkey_from_hex_bytes_array() {
        let mut elements = vec!["0x01".to_string(); 32];
        elements.extend(vec!["0x00".to_string(); 32]);
        let keypair_hex_bytes = format!("[{}]", elements.join(","));
        let parsed =
            parse_convert_args(&["sonar", "convert", "keypair", "pubkey", &keypair_hex_bytes]);
        let output = convert(&parsed).unwrap();
        assert_eq!(output, "11111111111111111111111111111111");
    }

    #[test]
    fn convert_keypair_to_pubkey_from_decimal_bytes_array() {
        let mut elements = vec!["1".to_string(); 32];
        elements.extend(vec!["0".to_string(); 32]);
        let keypair_bytes = format!("[{}]", elements.join(","));
        let parsed = parse_convert_args(&["sonar", "convert", "keypair", "pubkey", &keypair_bytes]);
        let output = convert(&parsed).unwrap();
        assert_eq!(output, "11111111111111111111111111111111");
    }

    #[test]
    fn convert_keypair_requires_exactly_64_bytes() {
        let invalid_hex = format!("0x{}{}", "01".repeat(31), "00".repeat(32));
        let parsed = parse_convert_args(&["sonar", "convert", "keypair", "pubkey", &invalid_hex]);
        let err = convert(&parsed).unwrap_err();
        assert!(err.contains("keypair requires exactly 64 bytes"));
    }

    #[test]
    fn convert_fixed_unsigned_boundaries() {
        let min = parse_convert_args(&["sonar", "convert", "u8", "hex", "0"]);
        assert_eq!(convert(&min).unwrap(), "0x00");

        let max = parse_convert_args(&["sonar", "convert", "u8", "hex", "255"]);
        assert_eq!(convert(&max).unwrap(), "0xff");

        let overflow = parse_convert_args(&["sonar", "convert", "u8", "hex", "256"]);
        let err = convert(&overflow).unwrap_err();
        assert!(err.contains("u8 value 256 is out of range"));
    }

    #[test]
    fn convert_fixed_signed_boundaries() {
        let min = parse_convert_args(&["sonar", "convert", "i8", "hex", "--", "-128"]);
        assert_eq!(convert(&min).unwrap(), "0x80");

        let max = parse_convert_args(&["sonar", "convert", "i8", "hex", "127"]);
        assert_eq!(convert(&max).unwrap(), "0x7f");

        let overflow = parse_convert_args(&["sonar", "convert", "i8", "hex", "--", "-129"]);
        let err = convert(&overflow).unwrap_err();
        assert!(err.contains("i8 value -129 is out of range"));
    }

    #[test]
    fn convert_fixed_integer_respects_endianness() {
        let be = parse_convert_args(&["sonar", "convert", "u16", "hex", "4660"]);
        assert_eq!(convert(&be).unwrap(), "0x1234");

        let mut le = parse_convert_args(&["sonar", "convert", "u16", "hex", "4660"]);
        le.le = true;
        assert_eq!(convert(&le).unwrap(), "0x3412");
    }

    #[test]
    fn convert_hex_to_fixed_integer_requires_exact_width_for_byte_input() {
        let u16_err = parse_convert_args(&["sonar", "convert", "hex", "u16", "0x01"]);
        let err = convert(&u16_err).unwrap_err();
        assert!(err.contains("u16 requires exactly 2 bytes"));

        let i16_err = parse_convert_args(&["sonar", "convert", "hex", "i16", "0x01"]);
        let err = convert(&i16_err).unwrap_err();
        assert!(err.contains("i16 requires exactly 2 bytes"));
    }

    #[test]
    fn convert_int_to_fixed_integer_uses_range_check() {
        let ok = parse_convert_args(&["sonar", "convert", "int", "u64", "1"]);
        assert_eq!(convert(&ok).unwrap(), "1");

        let overflow =
            parse_convert_args(&["sonar", "convert", "int", "u64", "18446744073709551616"]);
        let err = convert(&overflow).unwrap_err();
        assert!(err.contains("u64 value 18446744073709551616 is out of range"));
    }

    #[test]
    fn convert_hex_to_i16_respects_endianness() {
        let be = parse_convert_args(&["sonar", "convert", "hex", "i16", "0xfffe"]);
        assert_eq!(convert(&be).unwrap(), "-2");

        let mut le = parse_convert_args(&["sonar", "convert", "hex", "i16", "0xfeff"]);
        le.le = true;
        assert_eq!(convert(&le).unwrap(), "-2");
    }
}
