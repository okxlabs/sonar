//! Byte and number conversion utilities for B2n and N2b commands.

use clap::{Args, ValueEnum};

#[derive(Args, Debug)]
pub struct B2nArgs {
    /// Input data (auto-detects format: hex string 0x..., hex array [0x12,...], decimal array [12,...] or [12 ...])
    #[arg(value_name = "INPUT")]
    pub input: String,

    /// Byte order for interpreting the input
    #[arg(short = 'e', long, value_enum, default_value_t = Endianness::Little)]
    pub endian: Endianness,
}

#[derive(Args, Debug)]
pub struct N2bArgs {
    /// Input number (decimal or 0x hex)
    #[arg(value_name = "NUMBER")]
    pub number: String,

    /// Byte order for output
    #[arg(short = 'e', long, value_enum, default_value_t = Endianness::Little)]
    pub endian: Endianness,

    /// Output format
    #[arg(short = 'f', long, value_enum, default_value_t = ByteFormat::Hex)]
    pub format: ByteFormat,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Default)]
pub enum Endianness {
    #[default]
    Little,
    Big,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Default)]
pub enum ByteFormat {
    #[default]
    Hex,
    HexArray,
    DecArray,
    DecArraySpace,
}

const MAX_BYTES: usize = 32;

/// Parse various byte input formats into a byte vector.
/// Supported formats:
/// - Hex string: 0x12345678
/// - Hex byte array: [0x12,0x34,0x45,0x78]
/// - Decimal byte array (comma-separated): [18,52,69,120]
/// - Decimal byte array (space-separated): [18 52 69 120]
pub fn parse_bytes_input(input: &str) -> Result<Vec<u8>, String> {
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

        let mut bytes = Vec::new();
        for elem in elements {
            let elem = elem.trim();
            if elem.is_empty() {
                continue;
            }

            let value = if elem.starts_with("0x") || elem.starts_with("0X") {
                // Hex element
                let hex_str = &elem[2..];
                u64::from_str_radix(hex_str, 16)
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
pub fn format_bytes(bytes: &[u8], format: ByteFormat) -> String {
    match format {
        ByteFormat::Hex => {
            if bytes.is_empty() {
                "0x0".to_string()
            } else {
                format!("0x{}", hex::encode(bytes))
            }
        }
        ByteFormat::HexArray => {
            let elements: Vec<String> = bytes.iter().map(|b| format!("0x{:02x}", b)).collect();
            format!("[{}]", elements.join(","))
        }
        ByteFormat::DecArray => {
            let elements: Vec<String> = bytes.iter().map(|b| b.to_string()).collect();
            format!("[{}]", elements.join(","))
        }
        ByteFormat::DecArraySpace => {
            let elements: Vec<String> = bytes.iter().map(|b| b.to_string()).collect();
            format!("[{}]", elements.join(" "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== parse_bytes_input tests =====

    #[test]
    fn parse_bytes_input_hex_string() {
        let result = parse_bytes_input("0x12345678").unwrap();
        assert_eq!(result, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn parse_bytes_input_hex_string_uppercase() {
        let result = parse_bytes_input("0X12AB").unwrap();
        assert_eq!(result, vec![0x12, 0xAB]);
    }

    #[test]
    fn parse_bytes_input_hex_string_with_whitespace() {
        let result = parse_bytes_input("  0x12 34 56  ").unwrap();
        assert_eq!(result, vec![0x12, 0x34, 0x56]);
    }

    #[test]
    fn parse_bytes_input_hex_string_odd_length() {
        // Odd length hex should be padded with leading zero
        let result = parse_bytes_input("0x123").unwrap();
        assert_eq!(result, vec![0x01, 0x23]);
    }

    #[test]
    fn parse_bytes_input_hex_array() {
        let result = parse_bytes_input("[0x12,0x34,0x56,0x78]").unwrap();
        assert_eq!(result, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn parse_bytes_input_hex_array_with_whitespace() {
        let result = parse_bytes_input("[ 0x12 , 0x34 , 0x56 ]").unwrap();
        assert_eq!(result, vec![0x12, 0x34, 0x56]);
    }

    #[test]
    fn parse_bytes_input_decimal_array_comma() {
        let result = parse_bytes_input("[18,52,86,120]").unwrap();
        assert_eq!(result, vec![18, 52, 86, 120]);
    }

    #[test]
    fn parse_bytes_input_decimal_array_space() {
        let result = parse_bytes_input("[18 52 86 120]").unwrap();
        assert_eq!(result, vec![18, 52, 86, 120]);
    }

    #[test]
    fn parse_bytes_input_decimal_array_with_extra_whitespace() {
        let result = parse_bytes_input("[  18,  52,  86  ]").unwrap();
        assert_eq!(result, vec![18, 52, 86]);
    }

    #[test]
    fn parse_bytes_input_empty_array() {
        let result = parse_bytes_input("[]").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_bytes_input_rejects_empty_input() {
        let err = parse_bytes_input("").unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn parse_bytes_input_rejects_invalid_format() {
        let err = parse_bytes_input("invalid").unwrap_err();
        assert!(err.contains("Invalid input format"));
    }

    #[test]
    fn parse_bytes_input_rejects_value_over_255() {
        let err = parse_bytes_input("[256]").unwrap_err();
        assert!(err.contains("exceeds 255"));
    }

    #[test]
    fn parse_bytes_input_rejects_too_many_bytes() {
        let long_input = format!("[{}]", (0..33).map(|_| "1").collect::<Vec<_>>().join(","));
        let err = parse_bytes_input(&long_input).unwrap_err();
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
        let result = format_bytes(&[0x12, 0x34, 0x56, 0x78], ByteFormat::Hex);
        assert_eq!(result, "0x12345678");
    }

    #[test]
    fn format_bytes_hex_empty() {
        let result = format_bytes(&[], ByteFormat::Hex);
        assert_eq!(result, "0x0");
    }

    #[test]
    fn format_bytes_hex_array() {
        let result = format_bytes(&[0x12, 0x34, 0x56], ByteFormat::HexArray);
        assert_eq!(result, "[0x12,0x34,0x56]");
    }

    #[test]
    fn format_bytes_dec_array() {
        let result = format_bytes(&[18, 52, 86], ByteFormat::DecArray);
        assert_eq!(result, "[18,52,86]");
    }

    #[test]
    fn format_bytes_dec_array_space() {
        let result = format_bytes(&[18, 52, 86], ByteFormat::DecArraySpace);
        assert_eq!(result, "[18 52 86]");
    }

    // ===== roundtrip tests =====

    #[test]
    fn roundtrip_hex_to_decimal_array() {
        let bytes = parse_bytes_input("0x12345678").unwrap();
        let formatted = format_bytes(&bytes, ByteFormat::DecArray);
        let parsed_back = parse_bytes_input(&formatted).unwrap();
        assert_eq!(bytes, parsed_back);
    }

    #[test]
    fn roundtrip_decimal_to_hex() {
        let bytes = parse_bytes_input("[18,52,86,120]").unwrap();
        let formatted = format_bytes(&bytes, ByteFormat::Hex);
        let parsed_back = parse_bytes_input(&formatted).unwrap();
        assert_eq!(bytes, parsed_back);
    }
}
