//! Unified data format conversion utilities.

use base64::Engine;
use clap::{Args, ValueEnum};

/// Supported conversion formats
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum ConvertFormat {
    /// Big integer: decimal (e.g. 255) or 0x-prefixed hex (aliases: num, n)
    #[value(alias = "num", alias = "n")]
    Number,
    /// Hex string with 0x prefix, e.g. 0x1234abcd (alias: h)
    #[value(alias = "h")]
    Hex,
    /// Hex byte array, e.g. [0x12,0x34,0x56] or [12,34,56] when -f hex-array (aliases: ha, x)
    #[value(alias = "ha", alias = "x")]
    HexArray,
    /// Decimal byte array, e.g. [18,52,86,120] (aliases: da, d)
    #[value(alias = "da", alias = "d")]
    DecArray,
    /// UTF-8 string (aliases: u, utf)
    #[value(alias = "u", alias = "utf")]
    Utf8,
    /// Base64 encoded string (alias: b64)
    #[value(alias = "b64")]
    Base64,
    /// Base58 encoded string, e.g. Solana pubkey (alias: b58)
    #[value(alias = "b58")]
    Base58,
    /// Lamports: raw amount (1 SOL = 1e9 lamports) (alias: lam)
    #[value(alias = "lam")]
    Lamports,
    /// SOL amount as decimal, e.g. 1.5
    Sol,
}

#[derive(Args, Debug)]
pub struct ConvertArgs {
    /// Input value to convert
    #[arg(value_name = "INPUT")]
    pub input: String,

    /// Input format; if omitted, auto-detected (0x→hex, [...]→dec-array, digits→number, +/=/→base64, else base58)
    #[arg(short = 'f', long, value_name = "FORMAT")]
    pub from: Option<ConvertFormat>,

    /// Output format
    #[arg(short = 't', long, value_name = "FORMAT")]
    pub to: ConvertFormat,

    /// Use big-endian byte order for number/hex/lamports; default is little-endian
    #[arg(short = 'b', long)]
    pub be: bool,

    /// Use space as separator for arrays (default: comma)
    #[arg(long)]
    pub space: bool,

    /// Add 0x prefix to hex-array elements
    #[arg(long)]
    pub prefix: bool,

    /// Show invalid UTF-8 bytes as \xNN escape sequences (for utf8 output)
    #[arg(short = 'e', long)]
    pub escape: bool,
}

/// Internal byte format enum for parsing/formatting helpers
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Default)]
pub enum ByteFormat {
    #[default]
    Hex,
    HexArray,
    DecArray,
}

/// Parse various byte input formats into a byte vector.
/// Supported formats:
/// - Hex string: 0x12345678
/// - Hex byte array: [12,34,45,78] (with format hint) or [0x12,0x34,0x45,0x78]
/// - Decimal byte array (comma-separated): [18,52,69,120]
/// - Decimal byte array (space-separated): [18 52 69 120]
///
/// The `format_hint` parameter specifies how to interpret array elements without 0x prefix:
/// - None: auto-detect (0x prefix = hex, otherwise decimal)
/// - Some(HexArray): interpret as hex
/// - Some(DecArray): interpret as decimal
/// - Some(Hex): ignored for arrays, only affects hex string interpretation
pub fn parse_bytes_input(input: &str, format_hint: Option<ByteFormat>) -> Result<Vec<u8>, String> {
    let input = input.trim();

    if input.is_empty() {
        return Err("Input cannot be empty".to_string());
    }

    // Check if it's a hex string (0x...) without brackets
    if input.starts_with("0x") || input.starts_with("0X") {
        // Remove 0x prefix and any whitespace
        let hex_str: String = input[2..].chars().filter(|c| !c.is_whitespace()).collect();
        if hex_str.is_empty() {
            return Err("Hex string cannot be empty after 0x prefix".to_string());
        }
        // Pad with leading zero if odd length
        let hex_str = if hex_str.len() % 2 != 0 { format!("0{}", hex_str) } else { hex_str };
        let bytes = hex::decode(&hex_str).map_err(|e| format!("Invalid hex string: {}", e))?;
        return Ok(bytes);
    }

    // Check if it's an array format [...]
    if input.starts_with('[') && input.ends_with(']') {
        let inner = input[1..input.len() - 1].trim();
        if inner.is_empty() {
            return Ok(Vec::new());
        }

        // Detect separator: comma or space
        let elements: Vec<&str> = if inner.contains(',') {
            inner.split(',').collect()
        } else {
            inner.split_whitespace().collect()
        };

        // Determine if we should interpret elements as hex based on format hint
        let force_hex = matches!(format_hint, Some(ByteFormat::HexArray));

        let mut bytes = Vec::new();
        for elem in elements {
            let elem = elem.trim();
            if elem.is_empty() {
                continue;
            }

            let value = if elem.starts_with("0x") || elem.starts_with("0X") {
                // Explicit hex element
                let hex_str = &elem[2..];
                u64::from_str_radix(hex_str, 16)
                    .map_err(|e| format!("Invalid hex value '{}': {}", elem, e))?
            } else if force_hex {
                // Format hint says treat as hex
                u64::from_str_radix(elem, 16)
                    .map_err(|e| format!("Invalid hex value '{}': {}", elem, e))?
            } else {
                // Decimal element
                elem.parse::<u64>()
                    .map_err(|e| format!("Invalid decimal value '{}': {}", elem, e))?
            };

            if value > 255 {
                return Err(format!("Byte value {} exceeds 255", value));
            }
            bytes.push(value as u8);
        }

        return Ok(bytes);
    }

    // When format hint explicitly says Hex, try parsing as raw hex string without 0x prefix
    if matches!(format_hint, Some(ByteFormat::Hex)) {
        let hex_str: String = input.chars().filter(|c| !c.is_whitespace()).collect();
        if hex_str.is_empty() {
            return Err("Hex string cannot be empty".to_string());
        }
        let hex_str = if hex_str.len() % 2 != 0 { format!("0{}", hex_str) } else { hex_str };
        let bytes = hex::decode(&hex_str).map_err(|e| format!("Invalid hex string: {}", e))?;
        return Ok(bytes);
    }

    Err("Invalid input format. Expected hex string (0x...) or byte array ([...])".to_string())
}

/// Parse a number from string (decimal or 0x hex format).
pub fn parse_number(input: &str) -> Result<num_bigint::BigUint, String> {
    use num_bigint::BigUint;

    let input = input.trim();
    if input.is_empty() {
        return Err("Number cannot be empty".to_string());
    }

    if input.starts_with("0x") || input.starts_with("0X") {
        let hex_str: String = input[2..].chars().filter(|c| !c.is_whitespace()).collect();
        if hex_str.is_empty() {
            return Err("Hex number cannot be empty after 0x prefix".to_string());
        }
        BigUint::parse_bytes(hex_str.as_bytes(), 16)
            .ok_or_else(|| format!("Invalid hex number: {}", input))
    } else {
        let dec_str: String = input.chars().filter(|c| !c.is_whitespace()).collect();
        BigUint::parse_bytes(dec_str.as_bytes(), 10)
            .ok_or_else(|| format!("Invalid decimal number: {}", input))
    }
}

/// Format bytes according to the specified format.
/// - `use_space`: use space instead of comma as separator for arrays
/// - `use_prefix`: add 0x prefix to each element in hex-array output
pub fn format_bytes(bytes: &[u8], format: ByteFormat, use_space: bool, use_prefix: bool) -> String {
    let separator = if use_space { " " } else { "," };
    match format {
        ByteFormat::Hex => {
            if bytes.is_empty() {
                "0x0".to_string()
            } else {
                format!("0x{}", hex::encode(bytes))
            }
        }
        ByteFormat::HexArray => {
            let elements: Vec<String> = if use_prefix {
                bytes.iter().map(|b| format!("0x{:02x}", b)).collect()
            } else {
                bytes.iter().map(|b| format!("{:02x}", b)).collect()
            };
            format!("[{}]", elements.join(separator))
        }
        ByteFormat::DecArray => {
            let elements: Vec<String> = bytes.iter().map(|b| b.to_string()).collect();
            format!("[{}]", elements.join(separator))
        }
    }
}

/// Convert bytes to UTF-8 string.
/// - Valid UTF-8 sequences are decoded normally (supports Chinese, emoji, etc.).
/// - Invalid bytes are replaced with U+FFFD or '\xNN' escape sequences.
pub fn bytes_to_utf8(bytes: &[u8], escape_invalid: bool) -> String {
    if !escape_invalid {
        // Use lossy conversion: invalid bytes become U+FFFD (replacement character)
        return String::from_utf8_lossy(bytes).into_owned();
    }

    // With escape: show invalid bytes as \xNN
    let mut result = String::new();
    let mut i = 0;
    while i < bytes.len() {
        // Try to decode a valid UTF-8 sequence starting at position i
        let remaining = &bytes[i..];
        match std::str::from_utf8(remaining) {
            Ok(valid_str) => {
                // Rest of the bytes are valid UTF-8
                result.push_str(valid_str);
                break;
            }
            Err(e) => {
                // Push valid bytes before the error
                let valid_up_to = e.valid_up_to();
                if valid_up_to > 0 {
                    // Safety: from_utf8 confirmed these bytes are valid
                    result.push_str(unsafe {
                        std::str::from_utf8_unchecked(&remaining[..valid_up_to])
                    });
                    i += valid_up_to;
                } else {
                    // The byte at position i is invalid, escape it
                    result.push_str(&format!("\\x{:02x}", bytes[i]));
                    i += 1;
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== parse_bytes_input tests =====

    #[test]
    fn parse_bytes_input_hex_string() {
        let result = parse_bytes_input("0x12345678", None).unwrap();
        assert_eq!(result, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn parse_bytes_input_hex_string_uppercase() {
        let result = parse_bytes_input("0X12AB", None).unwrap();
        assert_eq!(result, vec![0x12, 0xAB]);
    }

    #[test]
    fn parse_bytes_input_hex_string_with_whitespace() {
        let result = parse_bytes_input("  0x12 34 56  ", None).unwrap();
        assert_eq!(result, vec![0x12, 0x34, 0x56]);
    }

    #[test]
    fn parse_bytes_input_hex_string_odd_length() {
        // Odd length hex should be padded with leading zero
        let result = parse_bytes_input("0x123", None).unwrap();
        assert_eq!(result, vec![0x01, 0x23]);
    }

    #[test]
    fn parse_bytes_input_hex_array_with_0x_prefix() {
        let result = parse_bytes_input("[0x12,0x34,0x56,0x78]", None).unwrap();
        assert_eq!(result, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn parse_bytes_input_hex_array_with_whitespace() {
        let result = parse_bytes_input("[ 0x12 , 0x34 , 0x56 ]", None).unwrap();
        assert_eq!(result, vec![0x12, 0x34, 0x56]);
    }

    #[test]
    fn parse_bytes_input_hex_array_with_format_hint() {
        // With hex-array format hint, interpret as hex without 0x prefix
        let result = parse_bytes_input("[12,34,56,78]", Some(ByteFormat::HexArray)).unwrap();
        assert_eq!(result, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn parse_bytes_input_hex_array_space_with_format_hint() {
        // HexArray format hint works for both comma and space separated
        let result = parse_bytes_input("[12 34 56 78]", Some(ByteFormat::HexArray)).unwrap();
        assert_eq!(result, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn parse_bytes_input_decimal_array_comma() {
        let result = parse_bytes_input("[18,52,86,120]", None).unwrap();
        assert_eq!(result, vec![18, 52, 86, 120]);
    }

    #[test]
    fn parse_bytes_input_decimal_array_space() {
        let result = parse_bytes_input("[18 52 86 120]", None).unwrap();
        assert_eq!(result, vec![18, 52, 86, 120]);
    }

    #[test]
    fn parse_bytes_input_decimal_array_with_extra_whitespace() {
        let result = parse_bytes_input("[  18,  52,  86  ]", None).unwrap();
        assert_eq!(result, vec![18, 52, 86]);
    }

    #[test]
    fn parse_bytes_input_empty_array() {
        let result = parse_bytes_input("[]", None).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_bytes_input_rejects_empty_input() {
        let err = parse_bytes_input("", None).unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn parse_bytes_input_rejects_invalid_format() {
        let err = parse_bytes_input("invalid", None).unwrap_err();
        assert!(err.contains("Invalid input format"));
    }

    #[test]
    fn parse_bytes_input_rejects_value_over_255() {
        let err = parse_bytes_input("[256]", None).unwrap_err();
        assert!(err.contains("exceeds 255"));
    }

    #[test]
    fn parse_bytes_input_raw_hex_with_format_hint() {
        // Raw hex string without 0x prefix, with explicit Hex format hint
        let result = parse_bytes_input("48656c6c6f", Some(ByteFormat::Hex)).unwrap();
        assert_eq!(result, vec![0x48, 0x65, 0x6c, 0x6c, 0x6f]);
    }

    #[test]
    fn parse_bytes_input_raw_hex_odd_length() {
        // Odd-length raw hex should be padded with leading zero
        let result = parse_bytes_input("123", Some(ByteFormat::Hex)).unwrap();
        assert_eq!(result, vec![0x01, 0x23]);
    }

    #[test]
    fn parse_bytes_input_raw_hex_with_whitespace() {
        let result = parse_bytes_input("  48 65 6c  ", Some(ByteFormat::Hex)).unwrap();
        assert_eq!(result, vec![0x48, 0x65, 0x6c]);
    }

    #[test]
    fn parse_bytes_input_raw_hex_still_works_with_0x_prefix() {
        // 0x-prefixed hex should still work when format hint is Hex
        let result = parse_bytes_input("0x48656c6c6f", Some(ByteFormat::Hex)).unwrap();
        assert_eq!(result, vec![0x48, 0x65, 0x6c, 0x6c, 0x6f]);
    }

    #[test]
    fn parse_bytes_input_accepts_long_arrays() {
        // Test that we can parse arrays longer than 32 bytes
        let long_input =
            format!("[{}]", (0..100).map(|i| (i % 256).to_string()).collect::<Vec<_>>().join(","));
        let result = parse_bytes_input(&long_input, None).unwrap();
        assert_eq!(result.len(), 100);
    }

    // ===== parse_number tests =====

    #[test]
    fn parse_number_decimal() {
        let result = parse_number("305419896").unwrap();
        assert_eq!(result.to_string(), "305419896");
    }

    #[test]
    fn parse_number_hex() {
        let result = parse_number("0x12345678").unwrap();
        assert_eq!(result.to_string(), "305419896");
    }

    #[test]
    fn parse_number_with_whitespace() {
        let result = parse_number("  123  ").unwrap();
        assert_eq!(result.to_string(), "123");
    }

    #[test]
    fn parse_number_rejects_empty() {
        let err = parse_number("").unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn parse_number_rejects_invalid() {
        let err = parse_number("abc").unwrap_err();
        assert!(err.contains("Invalid"));
    }

    // ===== format_bytes tests =====

    #[test]
    fn format_bytes_hex() {
        let result = format_bytes(&[0x12, 0x34, 0x56, 0x78], ByteFormat::Hex, false, false);
        assert_eq!(result, "0x12345678");
    }

    #[test]
    fn format_bytes_hex_empty() {
        let result = format_bytes(&[], ByteFormat::Hex, false, false);
        assert_eq!(result, "0x0");
    }

    #[test]
    fn format_bytes_hex_array_comma() {
        let result = format_bytes(&[0x12, 0x34, 0x56], ByteFormat::HexArray, false, false);
        assert_eq!(result, "[12,34,56]");
    }

    #[test]
    fn format_bytes_hex_array_space() {
        let result = format_bytes(&[0x12, 0x34, 0x56], ByteFormat::HexArray, true, false);
        assert_eq!(result, "[12 34 56]");
    }

    #[test]
    fn format_bytes_hex_array_with_prefix() {
        let result = format_bytes(&[0x12, 0x34, 0x56], ByteFormat::HexArray, false, true);
        assert_eq!(result, "[0x12,0x34,0x56]");
    }

    #[test]
    fn format_bytes_hex_array_space_with_prefix() {
        let result = format_bytes(&[0x12, 0x34, 0x56], ByteFormat::HexArray, true, true);
        assert_eq!(result, "[0x12 0x34 0x56]");
    }

    #[test]
    fn format_bytes_dec_array_comma() {
        let result = format_bytes(&[18, 52, 86], ByteFormat::DecArray, false, false);
        assert_eq!(result, "[18,52,86]");
    }

    #[test]
    fn format_bytes_dec_array_space() {
        let result = format_bytes(&[18, 52, 86], ByteFormat::DecArray, true, false);
        assert_eq!(result, "[18 52 86]");
    }

    // ===== roundtrip tests =====

    #[test]
    fn roundtrip_hex_to_decimal_array() {
        let bytes = parse_bytes_input("0x12345678", None).unwrap();
        let formatted = format_bytes(&bytes, ByteFormat::DecArray, false, false);
        let parsed_back = parse_bytes_input(&formatted, None).unwrap();
        assert_eq!(bytes, parsed_back);
    }

    #[test]
    fn roundtrip_decimal_to_hex() {
        let bytes = parse_bytes_input("[18,52,86,120]", None).unwrap();
        let formatted = format_bytes(&bytes, ByteFormat::Hex, false, false);
        let parsed_back = parse_bytes_input(&formatted, None).unwrap();
        assert_eq!(bytes, parsed_back);
    }

    #[test]
    fn roundtrip_hex_array_with_format() {
        let bytes = parse_bytes_input("0x12345678", None).unwrap();
        let formatted = format_bytes(&bytes, ByteFormat::HexArray, false, false);
        // When parsing back, need to specify hex format
        let parsed_back = parse_bytes_input(&formatted, Some(ByteFormat::HexArray)).unwrap();
        assert_eq!(bytes, parsed_back);
    }

    #[test]
    fn roundtrip_hex_array_with_prefix() {
        let bytes = parse_bytes_input("0x12345678", None).unwrap();
        let formatted = format_bytes(&bytes, ByteFormat::HexArray, false, true);
        // With prefix, can parse back without format hint (0x prefix auto-detected)
        let parsed_back = parse_bytes_input(&formatted, None).unwrap();
        assert_eq!(bytes, parsed_back);
    }

    // ===== bytes_to_utf8 tests =====

    #[test]
    fn bytes_to_utf8_ascii() {
        // "Hello" = [72, 101, 108, 108, 111]
        let result = bytes_to_utf8(&[72, 101, 108, 108, 111], false);
        assert_eq!(result, "Hello");
    }

    #[test]
    fn bytes_to_utf8_chinese() {
        // "你好" in UTF-8 = [228, 189, 160, 229, 165, 189]
        let result = bytes_to_utf8(&[228, 189, 160, 229, 165, 189], false);
        assert_eq!(result, "你好");
    }

    #[test]
    fn bytes_to_utf8_emoji() {
        // "😀" in UTF-8 = [240, 159, 152, 128]
        let result = bytes_to_utf8(&[240, 159, 152, 128], false);
        assert_eq!(result, "😀");
    }

    #[test]
    fn bytes_to_utf8_with_control_chars() {
        // "Hi" with NUL and newline: [72, 0, 105, 10] - these are valid UTF-8
        let result = bytes_to_utf8(&[72, 0, 105, 10], false);
        assert_eq!(result, "H\0i\n");
    }

    #[test]
    fn bytes_to_utf8_invalid_lossy() {
        // Invalid UTF-8 byte 0xFF should become replacement character
        let result = bytes_to_utf8(&[72, 255, 105], false);
        assert_eq!(result, "H\u{FFFD}i");
    }

    #[test]
    fn bytes_to_utf8_invalid_escape() {
        // Invalid UTF-8 byte 0xFF should be escaped
        let result = bytes_to_utf8(&[72, 255, 105], true);
        assert_eq!(result, "H\\xffi");
    }

    #[test]
    fn bytes_to_utf8_empty() {
        let result = bytes_to_utf8(&[], false);
        assert_eq!(result, "");
    }

    #[test]
    fn bytes_to_utf8_all_invalid_escape() {
        // All invalid bytes
        let result = bytes_to_utf8(&[255, 254, 253], true);
        assert_eq!(result, "\\xff\\xfe\\xfd");
    }

    #[test]
    fn bytes_to_utf8_mixed_valid_invalid() {
        // Mix of valid ASCII, valid multibyte UTF-8, and invalid bytes
        // "A" + invalid + "你" + invalid
        let result = bytes_to_utf8(&[65, 255, 228, 189, 160, 254], true);
        assert_eq!(result, "A\\xff你\\xfe");
    }

    // ===== detect_format tests =====

    #[test]
    fn detect_format_hex() {
        assert_eq!(detect_format("0x12345678"), ConvertFormat::Hex);
        assert_eq!(detect_format("0X12AB"), ConvertFormat::Hex);
    }

    #[test]
    fn detect_format_array() {
        assert_eq!(detect_format("[18,52,86,120]"), ConvertFormat::DecArray);
        assert_eq!(detect_format("[1 2 3]"), ConvertFormat::DecArray);
    }

    #[test]
    fn detect_format_base64() {
        assert_eq!(detect_format("SGVsbG8="), ConvertFormat::Base64);
        assert_eq!(detect_format("a+b/c"), ConvertFormat::Base64);
    }

    #[test]
    fn detect_format_number() {
        assert_eq!(detect_format("12345678"), ConvertFormat::Number);
        assert_eq!(detect_format("0"), ConvertFormat::Number);
    }

    #[test]
    fn detect_format_base58() {
        // Alphanumeric without special chars defaults to base58
        assert_eq!(detect_format("9Ajdvz"), ConvertFormat::Base58);
    }

    // ===== convert function tests =====

    #[test]
    fn convert_hex_to_number_le() {
        let args = ConvertArgs {
            input: "0x12345678".to_string(),
            from: Some(ConvertFormat::Hex),
            to: ConvertFormat::Number,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        // Little-endian: [0x12, 0x34, 0x56, 0x78] -> 0x78563412 = 2018915346
        assert_eq!(result, "2018915346");
    }

    #[test]
    fn convert_hex_to_number_be() {
        let args = ConvertArgs {
            input: "0x12345678".to_string(),
            from: Some(ConvertFormat::Hex),
            to: ConvertFormat::Number,
            be: true,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        // Big-endian: [0x12, 0x34, 0x56, 0x78] -> 0x12345678 = 305419896
        assert_eq!(result, "305419896");
    }

    #[test]
    fn convert_number_to_hex_le() {
        let args = ConvertArgs {
            input: "305419896".to_string(),
            from: Some(ConvertFormat::Number),
            to: ConvertFormat::Hex,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        // 305419896 = 0x12345678, little-endian bytes: [0x78, 0x56, 0x34, 0x12]
        assert_eq!(result, "0x78563412");
    }

    #[test]
    fn convert_number_to_hex_be() {
        let args = ConvertArgs {
            input: "305419896".to_string(),
            from: Some(ConvertFormat::Number),
            to: ConvertFormat::Hex,
            be: true,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "0x12345678");
    }

    #[test]
    fn convert_hex_to_dec_array() {
        let args = ConvertArgs {
            input: "0x48656c6c6f".to_string(),
            from: Some(ConvertFormat::Hex),
            to: ConvertFormat::DecArray,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "[72,101,108,108,111]");
    }

    #[test]
    fn convert_hex_to_utf8() {
        let args = ConvertArgs {
            input: "0x48656c6c6f".to_string(),
            from: Some(ConvertFormat::Hex),
            to: ConvertFormat::Utf8,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "Hello");
    }

    #[test]
    fn convert_hex_to_utf8_chinese() {
        // "你好" in UTF-8
        let args = ConvertArgs {
            input: "0xe4bda0e5a5bd".to_string(),
            from: Some(ConvertFormat::Hex),
            to: ConvertFormat::Utf8,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "你好");
    }

    #[test]
    fn convert_base64_to_base58() {
        let args = ConvertArgs {
            input: "SGVsbG8=".to_string(),
            from: Some(ConvertFormat::Base64),
            to: ConvertFormat::Base58,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "9Ajdvzr");
    }

    #[test]
    fn convert_base58_to_base64() {
        let args = ConvertArgs {
            input: "9Ajdvzr".to_string(),
            from: Some(ConvertFormat::Base58),
            to: ConvertFormat::Base64,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "SGVsbG8=");
    }

    #[test]
    fn convert_auto_detect_hex() {
        let args = ConvertArgs {
            input: "0x48656c6c6f".to_string(),
            from: None, // auto-detect
            to: ConvertFormat::Utf8,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "Hello");
    }

    #[test]
    fn convert_raw_hex_to_utf8() {
        // Raw hex without 0x prefix, with explicit --from hex
        let args = ConvertArgs {
            input: "48656c6c6f".to_string(),
            from: Some(ConvertFormat::Hex),
            to: ConvertFormat::Utf8,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "Hello");
    }

    #[test]
    fn convert_auto_detect_number() {
        let args = ConvertArgs {
            input: "255".to_string(),
            from: None, // auto-detect
            to: ConvertFormat::Hex,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "0xff");
    }

    #[test]
    fn convert_hex_array_with_space_separator() {
        let args = ConvertArgs {
            input: "0x12345678".to_string(),
            from: Some(ConvertFormat::Hex),
            to: ConvertFormat::HexArray,
            be: false,
            space: true,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "[12 34 56 78]");
    }

    #[test]
    fn convert_hex_array_with_prefix() {
        let args = ConvertArgs {
            input: "0x12345678".to_string(),
            from: Some(ConvertFormat::Hex),
            to: ConvertFormat::HexArray,
            be: false,
            space: false,
            prefix: true,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "[0x12,0x34,0x56,0x78]");
    }

    // ===== lamports/sol conversion tests =====

    #[test]
    fn convert_lamports_to_sol_whole() {
        let args = ConvertArgs {
            input: "1000000000".to_string(),
            from: Some(ConvertFormat::Lamports),
            to: ConvertFormat::Sol,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "1");
    }

    #[test]
    fn convert_lamports_to_sol_decimal() {
        let args = ConvertArgs {
            input: "1500000000".to_string(),
            from: Some(ConvertFormat::Lamports),
            to: ConvertFormat::Sol,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "1.500");
    }

    #[test]
    fn convert_lamports_to_sol_small() {
        let args = ConvertArgs {
            input: "1".to_string(),
            from: Some(ConvertFormat::Lamports),
            to: ConvertFormat::Sol,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "0.000000001");
    }

    #[test]
    fn convert_sol_to_lamports_whole() {
        let args = ConvertArgs {
            input: "1".to_string(),
            from: Some(ConvertFormat::Sol),
            to: ConvertFormat::Lamports,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "1000000000");
    }

    #[test]
    fn convert_sol_to_lamports_decimal() {
        let args = ConvertArgs {
            input: "1.5".to_string(),
            from: Some(ConvertFormat::Sol),
            to: ConvertFormat::Lamports,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "1500000000");
    }

    #[test]
    fn convert_sol_to_lamports_small() {
        let args = ConvertArgs {
            input: "0.000000001".to_string(),
            from: Some(ConvertFormat::Sol),
            to: ConvertFormat::Lamports,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "1");
    }

    #[test]
    fn convert_lamports_to_sol_zero() {
        let args = ConvertArgs {
            input: "0".to_string(),
            from: Some(ConvertFormat::Lamports),
            to: ConvertFormat::Sol,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        assert_eq!(result, "0");
    }

    #[test]
    fn convert_lamports_to_hex() {
        let args = ConvertArgs {
            input: "1000000000".to_string(),
            from: Some(ConvertFormat::Lamports),
            to: ConvertFormat::Hex,
            be: false,
            space: false,
            prefix: false,
            escape: false,
        };
        let result = convert(&args).unwrap();
        // 1000000000 = 0x3B9ACA00, little-endian
        assert_eq!(result, "0x00ca9a3b00000000");
    }

    #[test]
    fn format_sol_precision() {
        // Test format_sol directly
        assert_eq!(format_sol(0), "0");
        assert_eq!(format_sol(1_000_000_000), "1");
        assert_eq!(format_sol(2_500_000_000), "2.500");
        assert_eq!(format_sol(1_234_567_000), "1.234567");
        assert_eq!(format_sol(1_234_567_890), "1.23456789");
        assert_eq!(format_sol(123), "0.000000123");
    }
}

/// Auto-detect input format based on content patterns.
pub fn detect_format(input: &str) -> ConvertFormat {
    let input = input.trim();

    // Check for hex string (0x...)
    if input.starts_with("0x") || input.starts_with("0X") {
        return ConvertFormat::Hex;
    }

    // Check for array format [...]
    if input.starts_with('[') && input.ends_with(']') {
        // Default to decimal array (most common case)
        return ConvertFormat::DecArray;
    }

    // Check if it looks like base64 (contains +, /, or ends with =)
    if input.contains('+') || input.contains('/') || input.ends_with('=') {
        return ConvertFormat::Base64;
    }

    // Check if it's a pure decimal number
    if input.chars().all(|c| c.is_ascii_digit()) {
        return ConvertFormat::Number;
    }

    // Default to base58 for alphanumeric strings (common for Solana)
    ConvertFormat::Base58
}

/// Intermediate representation for conversions
pub enum ConvertValue {
    Bytes(Vec<u8>),
    Number(num_bigint::BigUint),
    /// Lamports amount (for SOL/lamports conversion)
    Lamports(u64),
}

/// 1 SOL = 1,000,000,000 lamports
const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

/// Parse input string according to the specified format into intermediate representation.
pub fn parse_input(input: &str, format: ConvertFormat) -> Result<ConvertValue, String> {
    match format {
        ConvertFormat::Number => {
            let num = parse_number(input)?;
            Ok(ConvertValue::Number(num))
        }
        ConvertFormat::Hex => {
            let bytes = parse_bytes_input(input, Some(ByteFormat::Hex))?;
            Ok(ConvertValue::Bytes(bytes))
        }
        ConvertFormat::HexArray => {
            let bytes = parse_bytes_input(input, Some(ByteFormat::HexArray))?;
            Ok(ConvertValue::Bytes(bytes))
        }
        ConvertFormat::DecArray => {
            let bytes = parse_bytes_input(input, Some(ByteFormat::DecArray))?;
            Ok(ConvertValue::Bytes(bytes))
        }
        ConvertFormat::Utf8 => {
            // UTF-8 input: Rust strings are already UTF-8
            Ok(ConvertValue::Bytes(input.as_bytes().to_vec()))
        }
        ConvertFormat::Base64 => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(input)
                .map_err(|e| format!("Invalid base64 input: {}", e))?;
            Ok(ConvertValue::Bytes(bytes))
        }
        ConvertFormat::Base58 => {
            let bytes = bs58::decode(input)
                .into_vec()
                .map_err(|e| format!("Invalid base58 input: {}", e))?;
            Ok(ConvertValue::Bytes(bytes))
        }
        ConvertFormat::Lamports => {
            let input = input.trim();
            let lamports: u64 =
                input.parse().map_err(|e| format!("Invalid lamports value '{}': {}", input, e))?;
            Ok(ConvertValue::Lamports(lamports))
        }
        ConvertFormat::Sol => {
            let input = input.trim();
            let sol: f64 =
                input.parse().map_err(|e| format!("Invalid SOL value '{}': {}", input, e))?;
            if sol < 0.0 {
                return Err("SOL amount cannot be negative".to_string());
            }
            // Convert SOL to lamports (1 SOL = 10^9 lamports)
            let lamports = (sol * LAMPORTS_PER_SOL as f64).round() as u64;
            Ok(ConvertValue::Lamports(lamports))
        }
    }
}

/// Convert intermediate value to bytes, considering endianness for numbers.
pub fn value_to_bytes(value: ConvertValue, big_endian: bool) -> Vec<u8> {
    match value {
        ConvertValue::Bytes(bytes) => bytes,
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

/// Format bytes according to the target format.
pub fn format_output(
    bytes: &[u8],
    format: ConvertFormat,
    big_endian: bool,
    use_space: bool,
    use_prefix: bool,
    escape: bool,
) -> String {
    match format {
        ConvertFormat::Number => {
            use num_bigint::BigUint;
            let num = if big_endian {
                BigUint::from_bytes_be(bytes)
            } else {
                BigUint::from_bytes_le(bytes)
            };
            num.to_string()
        }
        ConvertFormat::Hex => format_bytes(bytes, ByteFormat::Hex, use_space, use_prefix),
        ConvertFormat::HexArray => format_bytes(bytes, ByteFormat::HexArray, use_space, use_prefix),
        ConvertFormat::DecArray => format_bytes(bytes, ByteFormat::DecArray, use_space, use_prefix),
        ConvertFormat::Utf8 => bytes_to_utf8(bytes, escape),
        ConvertFormat::Base64 => base64::engine::general_purpose::STANDARD.encode(bytes),
        ConvertFormat::Base58 => bs58::encode(bytes).into_string(),
        ConvertFormat::Lamports => {
            // Interpret bytes as u64 lamports
            let lamports = bytes_to_u64(bytes, big_endian);
            lamports.to_string()
        }
        ConvertFormat::Sol => {
            // Interpret bytes as u64 lamports, convert to SOL
            let lamports = bytes_to_u64(bytes, big_endian);
            format_sol(lamports)
        }
    }
}

/// Convert bytes to u64, handling variable-length input.
fn bytes_to_u64(bytes: &[u8], big_endian: bool) -> u64 {
    if bytes.is_empty() {
        return 0;
    }

    // Pad or truncate to 8 bytes
    let mut buf = [0u8; 8];
    let len = bytes.len().min(8);

    if big_endian {
        // For big-endian, align to the right
        buf[8 - len..].copy_from_slice(&bytes[..len]);
        u64::from_be_bytes(buf)
    } else {
        // For little-endian, align to the left
        buf[..len].copy_from_slice(&bytes[..len]);
        u64::from_le_bytes(buf)
    }
}

/// Format lamports as SOL with appropriate precision.
fn format_sol(lamports: u64) -> String {
    let sol = lamports as f64 / LAMPORTS_PER_SOL as f64;

    // Use appropriate precision based on the amount
    if lamports == 0 {
        "0".to_string()
    } else if lamports % LAMPORTS_PER_SOL == 0 {
        // Whole SOL amount
        format!("{}", lamports / LAMPORTS_PER_SOL)
    } else if lamports % 1_000_000 == 0 {
        // Millis precision (3 decimals)
        format!("{:.3}", sol)
    } else if lamports % 1_000 == 0 {
        // Micros precision (6 decimals)
        format!("{:.6}", sol)
    } else {
        // Full precision (9 decimals), trim trailing zeros
        let formatted = format!("{:.9}", sol);
        formatted.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Perform the complete conversion from input to output.
pub fn convert(args: &ConvertArgs) -> Result<String, String> {
    // Determine input format (auto-detect if not specified)
    let from_format = args.from.unwrap_or_else(|| detect_format(&args.input));

    // Parse input
    let value = parse_input(&args.input, from_format)?;

    // Special case: direct lamports <-> SOL conversion without bytes intermediate
    if let ConvertValue::Lamports(lamports) = &value {
        match args.to {
            ConvertFormat::Sol => {
                return Ok(format_sol(*lamports));
            }
            ConvertFormat::Lamports => {
                return Ok(lamports.to_string());
            }
            _ => {} // Fall through to bytes conversion
        }
    }

    // Convert to bytes
    let bytes = value_to_bytes(value, args.be);

    // Format output
    let output = format_output(&bytes, args.to, args.be, args.space, args.prefix, args.escape);

    Ok(output)
}
