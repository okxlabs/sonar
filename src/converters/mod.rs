//! Pure conversion logic for `sonar convert`.

mod bytes;
mod integers;
pub mod sol;
mod text;
pub mod types;

pub use types::{ConvertRequest, InputFormat, OutputFormat};

use std::{
    io::{IsTerminal, Read},
    str::FromStr,
};

use base64::Engine;
use num_bigint::{BigUint, Sign};
use solana_pubkey::Pubkey;
use solana_signature::Signature;

use bytes::{format_binary, format_bytes, parse_binary_input, parse_bytes_input};
use integers::{format_fixed_integer, parse_fixed_integer, parse_number, value_to_bytes};
use sol::{format_sol, parse_sol_to_lamports};
use text::{bytes_to_utf8, format_base64_error};
use types::{ByteFormat, ConvertValue, input_fixed_int_spec, output_fixed_int_spec};

fn parse_input_with_format(input: &str, format: InputFormat) -> Result<ConvertValue, String> {
    if let Some(spec) = input_fixed_int_spec(format) {
        return parse_fixed_integer(input, spec);
    }

    match format {
        InputFormat::Int => Ok(ConvertValue::Number(parse_number(input)?)),
        InputFormat::Hex => {
            Ok(ConvertValue::Bytes(parse_bytes_input(input, Some(ByteFormat::Hex))?))
        }
        InputFormat::HexBytes => {
            Ok(ConvertValue::Bytes(parse_bytes_input(input, Some(ByteFormat::HexBytes))?))
        }
        InputFormat::Bytes => {
            Ok(ConvertValue::Bytes(parse_bytes_input(input, Some(ByteFormat::Bytes))?))
        }
        InputFormat::Text => Ok(ConvertValue::Bytes(input.as_bytes().to_vec())),
        InputFormat::Binary => Ok(ConvertValue::Bytes(parse_binary_input(input)?)),
        InputFormat::Base64 => {
            let value = base64::engine::general_purpose::STANDARD
                .decode(input)
                .map_err(|e| format!("Invalid base64 input: {}", format_base64_error(&e)))?;
            Ok(ConvertValue::Bytes(value))
        }
        InputFormat::Base58 => {
            let value = bs58::decode(input)
                .into_vec()
                .map_err(|e| format!("Invalid base58 input: {}", e))?;
            Ok(ConvertValue::Bytes(value))
        }
        InputFormat::Pubkey => {
            let trimmed = input.trim();
            let pubkey = Pubkey::from_str(trimmed)
                .map_err(|e| format!("Invalid pubkey '{}': {}", trimmed, e))?;
            Ok(ConvertValue::Bytes(pubkey.to_bytes().to_vec()))
        }
        InputFormat::Signature => {
            let trimmed = input.trim();
            let signature = Signature::from_str(trimmed)
                .map_err(|e| format!("Invalid signature '{}': {}", trimmed, e))?;
            Ok(ConvertValue::Bytes(signature.as_ref().to_vec()))
        }
        InputFormat::Keypair => {
            let bytes = parse_bytes_input(input, Some(ByteFormat::Bytes))?;
            if bytes.len() != 64 {
                return Err(format!("keypair requires exactly 64 bytes, got {}", bytes.len()));
            }
            Ok(ConvertValue::Bytes(bytes[32..].to_vec()))
        }
        InputFormat::Lamports => {
            let trimmed = input.trim();
            let lamports = trimmed
                .parse::<u64>()
                .map_err(|e| format!("Invalid lamports value '{}': {}", trimmed, e))?;
            Ok(ConvertValue::Lamports(lamports))
        }
        InputFormat::Sol => Ok(ConvertValue::Lamports(parse_sol_to_lamports(input)?)),
        _ => unreachable!("fixed integer formats handled before match"),
    }
}

fn format_target(
    value: &ConvertValue,
    target: OutputFormat,
    big_endian: bool,
    separator: &str,
    hex_array_with_prefix: bool,
    escape_text: bool,
) -> Result<String, String> {
    if let Some(spec) = output_fixed_int_spec(target) {
        return format_fixed_integer(value, spec, big_endian);
    }

    match target {
        OutputFormat::Lamports => {
            let lamports = value_to_lamports(value, big_endian)?;
            Ok(lamports.to_string())
        }
        OutputFormat::Sol => {
            let lamports = value_to_lamports(value, big_endian)?;
            Ok(format_sol(lamports))
        }
        OutputFormat::Int => {
            let bytes = value_to_bytes(value, big_endian);
            let num = if big_endian {
                BigUint::from_bytes_be(&bytes)
            } else {
                BigUint::from_bytes_le(&bytes)
            };
            Ok(num.to_string())
        }
        OutputFormat::Hex => {
            let bytes = value_to_bytes(value, big_endian);
            Ok(format_bytes(&bytes, ByteFormat::Hex, separator, hex_array_with_prefix))
        }
        OutputFormat::HexBytes => {
            let bytes = value_to_bytes(value, big_endian);
            Ok(format_bytes(&bytes, ByteFormat::HexBytes, separator, hex_array_with_prefix))
        }
        OutputFormat::Bytes => {
            let bytes = value_to_bytes(value, big_endian);
            Ok(format_bytes(&bytes, ByteFormat::Bytes, separator, hex_array_with_prefix))
        }
        OutputFormat::Text => {
            let bytes = value_to_bytes(value, big_endian);
            Ok(bytes_to_utf8(&bytes, escape_text))
        }
        OutputFormat::Binary => {
            let bytes = value_to_bytes(value, big_endian);
            Ok(format_binary(&bytes))
        }
        OutputFormat::Base64 => {
            let bytes = value_to_bytes(value, big_endian);
            Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
        }
        OutputFormat::Base58 => {
            let bytes = value_to_bytes(value, big_endian);
            Ok(bs58::encode(&bytes).into_string())
        }
        OutputFormat::Pubkey => {
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
        OutputFormat::Signature => {
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

fn biguint_to_u64(value: &BigUint) -> Option<u64> {
    let bytes = value.to_bytes_be();
    if bytes.len() > 8 {
        return None;
    }
    let mut buf = [0u8; 8];
    buf[8 - bytes.len()..].copy_from_slice(&bytes);
    Some(u64::from_be_bytes(buf))
}

fn value_to_lamports(value: &ConvertValue, big_endian: bool) -> Result<u64, String> {
    let out_of_range = |raw: &str| format!("Lamports value {} is out of range for u64", raw);

    match value {
        ConvertValue::Lamports(v) => Ok(*v),
        ConvertValue::Number(num) => {
            let raw = num.to_string();
            biguint_to_u64(num).ok_or_else(|| out_of_range(&raw))
        }
        ConvertValue::FixedUnsigned { value, .. } => {
            u64::try_from(*value).map_err(|_| out_of_range(&value.to_string()))
        }
        ConvertValue::FixedSigned { value, .. } => {
            if *value < 0 {
                return Err(out_of_range(&value.to_string()));
            }
            u64::try_from(*value).map_err(|_| out_of_range(&value.to_string()))
        }
        ConvertValue::FixedBigUnsigned { value, .. } => {
            let raw = value.to_string();
            biguint_to_u64(value).ok_or_else(|| out_of_range(&raw))
        }
        ConvertValue::FixedBigSigned { value, .. } => {
            if value.sign() == Sign::Minus {
                return Err(out_of_range(&value.to_string()));
            }
            let (_, magnitude) = value.to_bytes_be();
            let num = BigUint::from_bytes_be(&magnitude);
            let raw = value.to_string();
            biguint_to_u64(&num).ok_or_else(|| out_of_range(&raw))
        }
        ConvertValue::Bytes(bytes) => {
            let num = if big_endian {
                BigUint::from_bytes_be(bytes)
            } else {
                BigUint::from_bytes_le(bytes)
            };
            let raw = num.to_string();
            biguint_to_u64(&num).ok_or_else(|| out_of_range(&raw))
        }
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
pub fn convert(req: &ConvertRequest) -> Result<String, String> {
    let separator = normalize_separator(&req.sep)?;
    let big_endian = !req.le;
    let input = read_convert_input(req.input.as_deref())?;
    let value = parse_input_with_format(&input, req.from)?;
    format_target(&value, req.to, big_endian, separator, !req.no_prefix, req.escape)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(from: InputFormat, input: &str, to: OutputFormat) -> ConvertRequest {
        ConvertRequest {
            from,
            to,
            input: Some(input.to_string()),
            le: false,
            sep: ",".to_string(),
            no_prefix: false,
            escape: false,
        }
    }

    #[test]
    fn convert_hex_to_text() {
        let output = convert(&req(InputFormat::Hex, "0x48656c6c6f", OutputFormat::Text)).unwrap();
        assert_eq!(output, "Hello");
    }

    #[test]
    fn convert_int_to_hex_default_be() {
        let output = convert(&req(InputFormat::Int, "305419896", OutputFormat::Hex)).unwrap();
        assert_eq!(output, "0x12345678");
    }

    #[test]
    fn convert_int_to_hex_le() {
        let mut value = req(InputFormat::Int, "305419896", OutputFormat::Hex);
        value.le = true;
        let output = convert(&value).unwrap();
        assert_eq!(output, "0x78563412");
    }

    #[test]
    fn convert_sol_to_lamports() {
        let output = convert(&req(InputFormat::Sol, "1.5", OutputFormat::Lamports)).unwrap();
        assert_eq!(output, "1500000000");
    }

    #[test]
    fn convert_lamports_to_sol() {
        let output = convert(&req(InputFormat::Lamports, "1500000000", OutputFormat::Sol)).unwrap();
        assert_eq!(output, "1.5");
    }

    #[test]
    fn convert_u128_to_lamports_small_value() {
        let output = convert(&req(InputFormat::U128, "1", OutputFormat::Lamports)).unwrap();
        assert_eq!(output, "1");
    }

    #[test]
    fn convert_u256_to_lamports_small_value() {
        let output = convert(&req(InputFormat::U256, "1", OutputFormat::Lamports)).unwrap();
        assert_eq!(output, "1");
    }

    #[test]
    fn convert_u128_to_lamports_rejects_overflow() {
        let err = convert(&req(InputFormat::U128, "18446744073709551616", OutputFormat::Lamports))
            .unwrap_err();
        assert!(err.contains("out of range"));
    }

    #[test]
    fn convert_hex_to_lamports_rejects_overflow() {
        let err = convert(&req(InputFormat::Hex, "0x010000000000000000", OutputFormat::Lamports))
            .unwrap_err();
        assert!(err.contains("out of range"));
    }

    #[test]
    fn convert_hex_bytes_with_no_prefix() {
        let mut value = req(InputFormat::Hex, "0x123456", OutputFormat::HexBytes);
        value.no_prefix = true;
        let output = convert(&value).unwrap();
        assert_eq!(output, "[12,34,56]");
    }

    #[test]
    fn convert_bytes_separator_space() {
        let mut value = req(InputFormat::Hex, "0x123456", OutputFormat::Bytes);
        value.sep = " ".to_string();
        let output = convert(&value).unwrap();
        assert_eq!(output, "[18 52 86]");
    }

    #[test]
    fn convert_rejects_invalid_separator() {
        let mut value = req(InputFormat::Hex, "0x1234", OutputFormat::Bytes);
        value.sep = "::".to_string();
        let err = convert(&value).unwrap_err();
        assert!(err.contains("exactly one character"));
    }

    #[test]
    fn convert_text_escape_invalid() {
        let mut value = req(InputFormat::Hex, "0xff", OutputFormat::Text);
        value.escape = true;
        let output = convert(&value).unwrap();
        assert_eq!(output, "\\xff");
    }

    #[test]
    fn convert_hex_to_binary() {
        let output = convert(&req(InputFormat::Hex, "0x48656c6c6f", OutputFormat::Binary)).unwrap();
        assert_eq!(output, "0b0100100001100101011011000110110001101111");
    }

    #[test]
    fn convert_int_to_binary_default_be() {
        let output = convert(&req(InputFormat::Int, "305419896", OutputFormat::Binary)).unwrap();
        assert_eq!(output, "0b00010010001101000101011001111000");
    }

    #[test]
    fn convert_int_to_binary_le() {
        let mut value = req(InputFormat::Int, "305419896", OutputFormat::Binary);
        value.le = true;
        let output = convert(&value).unwrap();
        assert_eq!(output, "0b01111000010101100011010000010010");
    }

    #[test]
    fn convert_zero_to_binary() {
        let output = convert(&req(InputFormat::Int, "0", OutputFormat::Binary)).unwrap();
        assert_eq!(output, "0b00000000");
    }

    #[test]
    fn convert_pubkey_hex_roundtrip() {
        let pubkey = "11111111111111111111111111111111";
        let to_hex = req(InputFormat::Pubkey, pubkey, OutputFormat::Hex);
        let hex = convert(&to_hex).unwrap();
        assert_eq!(hex, "0x0000000000000000000000000000000000000000000000000000000000000000");

        let back = req(InputFormat::Hex, &hex, OutputFormat::Pubkey);
        let pubkey_back = convert(&back).unwrap();
        assert_eq!(pubkey_back, pubkey);
    }

    #[test]
    fn convert_signature_hex_roundtrip() {
        let signature = "3PtGYH77LhhQqTXP4SmDVJ85hmDieWsgXCUbn14v7gYyVYPjZzygUQhTk3bSTYnfA48vCM1rmWY7zWL3j1EVKmEy";
        let to_hex = req(InputFormat::Signature, signature, OutputFormat::Hex);
        let hex = convert(&to_hex).unwrap();
        let expected_hex =
            format!("0x{}", hex::encode(bs58::decode(signature).into_vec().unwrap()));
        assert_eq!(hex, expected_hex);

        let back = req(InputFormat::Hex, &hex, OutputFormat::Signature);
        let signature_back = convert(&back).unwrap();
        assert_eq!(signature_back, signature);
    }

    #[test]
    fn convert_pubkey_requires_exactly_32_bytes() {
        let parsed = req(
            InputFormat::Hex,
            "0x01010101010101010101010101010101010101010101010101010101010101",
            OutputFormat::Pubkey,
        );
        let err = convert(&parsed).unwrap_err();
        assert!(err.contains("pubkey requires exactly 32 bytes"));
    }

    #[test]
    fn convert_signature_requires_exactly_64_bytes() {
        let parsed = req(
            InputFormat::Hex,
            "0x010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101",
            OutputFormat::Signature,
        );
        let err = convert(&parsed).unwrap_err();
        assert!(err.contains("signature requires exactly 64 bytes"));
    }

    #[test]
    fn convert_pubkey_rejects_invalid_input() {
        let parsed = req(InputFormat::Pubkey, "invalid-pubkey", OutputFormat::Hex);
        let err = convert(&parsed).unwrap_err();
        assert!(err.contains("Invalid pubkey"));
    }

    #[test]
    fn convert_signature_rejects_invalid_input() {
        let parsed = req(InputFormat::Signature, "invalid-signature", OutputFormat::Hex);
        let err = convert(&parsed).unwrap_err();
        assert!(err.contains("Invalid signature"));
    }

    #[test]
    fn convert_keypair_to_pubkey_from_hex() {
        let keypair_hex = format!("0x{}{}", "01".repeat(32), "00".repeat(32));
        let parsed = req(InputFormat::Keypair, &keypair_hex, OutputFormat::Pubkey);
        let output = convert(&parsed).unwrap();
        assert_eq!(output, "11111111111111111111111111111111");
    }

    #[test]
    fn convert_keypair_to_pubkey_from_hex_bytes_array() {
        let mut elements = vec!["0x01".to_string(); 32];
        elements.extend(vec!["0x00".to_string(); 32]);
        let keypair_hex_bytes = format!("[{}]", elements.join(","));
        let parsed = req(InputFormat::Keypair, &keypair_hex_bytes, OutputFormat::Pubkey);
        let output = convert(&parsed).unwrap();
        assert_eq!(output, "11111111111111111111111111111111");
    }

    #[test]
    fn convert_keypair_to_pubkey_from_decimal_bytes_array() {
        let mut elements = vec!["1".to_string(); 32];
        elements.extend(vec!["0".to_string(); 32]);
        let keypair_bytes = format!("[{}]", elements.join(","));
        let parsed = req(InputFormat::Keypair, &keypair_bytes, OutputFormat::Pubkey);
        let output = convert(&parsed).unwrap();
        assert_eq!(output, "11111111111111111111111111111111");
    }

    #[test]
    fn convert_keypair_requires_exactly_64_bytes() {
        let invalid_hex = format!("0x{}{}", "01".repeat(31), "00".repeat(32));
        let parsed = req(InputFormat::Keypair, &invalid_hex, OutputFormat::Pubkey);
        let err = convert(&parsed).unwrap_err();
        assert!(err.contains("keypair requires exactly 64 bytes"));
    }

    #[test]
    fn convert_fixed_unsigned_boundaries() {
        let min = req(InputFormat::U8, "0", OutputFormat::Hex);
        assert_eq!(convert(&min).unwrap(), "0x00");

        let max = req(InputFormat::U8, "255", OutputFormat::Hex);
        assert_eq!(convert(&max).unwrap(), "0xff");

        let overflow = req(InputFormat::U8, "256", OutputFormat::Hex);
        let err = convert(&overflow).unwrap_err();
        assert!(err.contains("u8 value 256 is out of range"));
    }

    #[test]
    fn convert_fixed_signed_boundaries() {
        let min = req(InputFormat::I8, "-128", OutputFormat::Hex);
        assert_eq!(convert(&min).unwrap(), "0x80");

        let max = req(InputFormat::I8, "127", OutputFormat::Hex);
        assert_eq!(convert(&max).unwrap(), "0x7f");

        let overflow = req(InputFormat::I8, "-129", OutputFormat::Hex);
        let err = convert(&overflow).unwrap_err();
        assert!(err.contains("i8 value -129 is out of range"));
    }

    #[test]
    fn convert_fixed_integer_respects_endianness() {
        let be = req(InputFormat::U16, "4660", OutputFormat::Hex);
        assert_eq!(convert(&be).unwrap(), "0x1234");

        let mut le = req(InputFormat::U16, "4660", OutputFormat::Hex);
        le.le = true;
        assert_eq!(convert(&le).unwrap(), "0x3412");
    }

    #[test]
    fn convert_hex_to_fixed_integer_requires_exact_width_for_byte_input() {
        let u16_err = req(InputFormat::Hex, "0x01", OutputFormat::U16);
        let err = convert(&u16_err).unwrap_err();
        assert!(err.contains("u16 requires exactly 2 bytes"));

        let i16_err = req(InputFormat::Hex, "0x01", OutputFormat::I16);
        let err = convert(&i16_err).unwrap_err();
        assert!(err.contains("i16 requires exactly 2 bytes"));
    }

    #[test]
    fn convert_int_to_fixed_integer_uses_range_check() {
        let ok = req(InputFormat::Int, "1", OutputFormat::U64);
        assert_eq!(convert(&ok).unwrap(), "1");

        let overflow = req(InputFormat::Int, "18446744073709551616", OutputFormat::U64);
        let err = convert(&overflow).unwrap_err();
        assert!(err.contains("u64 value 18446744073709551616 is out of range"));
    }

    #[test]
    fn convert_hex_to_i16_respects_endianness() {
        let be = req(InputFormat::Hex, "0xfffe", OutputFormat::I16);
        assert_eq!(convert(&be).unwrap(), "-2");

        let mut le = req(InputFormat::Hex, "0xfeff", OutputFormat::I16);
        le.le = true;
        assert_eq!(convert(&le).unwrap(), "-2");
    }

    #[test]
    fn convert_binary_to_hex() {
        let output = convert(&req(InputFormat::Binary, "0b01001000", OutputFormat::Hex)).unwrap();
        assert_eq!(output, "0x48");
    }

    #[test]
    fn convert_binary_to_text() {
        let output = convert(&req(
            InputFormat::Binary,
            "0b0100100001100101011011000110110001101111",
            OutputFormat::Text,
        ))
        .unwrap();
        assert_eq!(output, "Hello");
    }

    #[test]
    fn convert_binary_input_with_underscores() {
        let output = convert(&req(InputFormat::Binary, "0b0100_1000", OutputFormat::Hex)).unwrap();
        assert_eq!(output, "0x48");
    }

    #[test]
    fn convert_binary_input_without_prefix() {
        let output = convert(&req(InputFormat::Binary, "01001000", OutputFormat::Hex)).unwrap();
        assert_eq!(output, "0x48");
    }

    #[test]
    fn convert_binary_input_partial_byte() {
        let output = convert(&req(InputFormat::Binary, "0b1111", OutputFormat::Hex)).unwrap();
        assert_eq!(output, "0x0f");
    }

    #[test]
    fn convert_binary_rejects_invalid_chars() {
        let err = convert(&req(InputFormat::Binary, "0b012", OutputFormat::Hex)).unwrap_err();
        assert!(err.contains("must contain only 0 and 1"));
    }

    #[test]
    fn convert_binary_roundtrip() {
        let to_binary = convert(&req(InputFormat::Hex, "0xff", OutputFormat::Binary)).unwrap();
        assert_eq!(to_binary, "0b11111111");

        let back = convert(&req(InputFormat::Binary, &to_binary, OutputFormat::Hex)).unwrap();
        assert_eq!(back, "0xff");
    }
}
