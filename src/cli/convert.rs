//! Byte and number conversion utilities for B2n and N2b commands.

use clap::{Args, ValueEnum};

#[derive(Args, Debug)]
pub struct B2nArgs {
    /// Hex string input (e.g., 0x12345678)
    #[arg(value_name = "HEX", group = "input")]
    pub hex: Option<String>,

    /// Hex byte array input (e.g., [12,34,56,78] or [12 34 56 78])
    #[arg(short = 'x', value_name = "ARRAY", group = "input")]
    pub hex_array: Option<String>,

    /// Decimal byte array input (e.g., [18,52,86,120] or [18 52 86 120])
    #[arg(short = 'd', value_name = "ARRAY", group = "input")]
    pub dec_array: Option<String>,

    /// Use big-endian byte order (default: little-endian)
    #[arg(short = 'b', long)]
    pub be: bool,
}

#[derive(Args, Debug)]
pub struct N2bArgs {
    /// Input number (decimal or 0x hex)
    #[arg(value_name = "NUMBER")]
    pub number: String,

    /// Output as hex byte array (e.g., [12,34,56,78])
    #[arg(short = 'x', group = "format")]
    pub hex_array: bool,

    /// Output as decimal byte array (e.g., [18,52,86,120])
    #[arg(short = 'd', group = "format")]
    pub dec_array: bool,

    /// Use big-endian byte order (default: little-endian)
    #[arg(short = 'b', long)]
    pub be: bool,

    /// Use space as separator for arrays (default: comma)
    #[arg(long)]
    pub space: bool,

    /// Add 0x prefix to each element in hex-array output (e.g., [0x12,0x34])
    #[arg(long)]
    pub prefix: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Default)]
pub enum ByteFormat {
    #[default]
    Hex,
    HexArray,
    DecArray,
}

const MAX_BYTES: usize = 32;

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
        if bytes.len() > MAX_BYTES {
            return Err(format!("Input exceeds maximum {} bytes", MAX_BYTES));
        }
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

        if bytes.len() > MAX_BYTES {
            return Err(format!("Input exceeds maximum {} bytes", MAX_BYTES));
        }
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
    fn parse_bytes_input_rejects_too_many_bytes() {
        let long_input = format!("[{}]", (0..33).map(|_| "1").collect::<Vec<_>>().join(","));
        let err = parse_bytes_input(&long_input, None).unwrap_err();
        assert!(err.contains("exceeds maximum"));
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
}
