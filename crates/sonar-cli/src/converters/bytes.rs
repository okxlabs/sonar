use super::types::ByteFormat;

/// Parse byte-oriented input.
pub(crate) fn parse_bytes_input(
    input: &str,
    format_hint: Option<ByteFormat>,
) -> Result<Vec<u8>, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Input cannot be empty".to_string());
    }

    if input.starts_with("0x") || input.starts_with("0X") {
        let hex_str: String = input[2..].chars().filter(|c| !c.is_whitespace()).collect();
        if hex_str.is_empty() {
            return Err("Hex string cannot be empty after 0x prefix".to_string());
        }
        let hex_str = if hex_str.len() % 2 != 0 { format!("0{}", hex_str) } else { hex_str };
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
        let hex_str = if hex_str.len() % 2 != 0 { format!("0{}", hex_str) } else { hex_str };
        return hex::decode(hex_str).map_err(|e| format!("Invalid hex string: {}", e));
    }

    Err("Invalid input format. Expected hex string (0x...) or byte array ([...])".to_string())
}

pub(crate) fn parse_binary_input(input: &str) -> Result<Vec<u8>, String> {
    let trimmed = input.trim();
    let body = if trimmed.starts_with("0b") || trimmed.starts_with("0B") {
        &trimmed[2..]
    } else {
        trimmed
    };

    let bits: String = body.chars().filter(|c| !c.is_whitespace() && *c != '_').collect();
    if bits.is_empty() {
        return Err("Binary string cannot be empty".to_string());
    }
    if !bits.chars().all(|c| c == '0' || c == '1') {
        return Err("Binary string must contain only 0 and 1".to_string());
    }

    let padded_len = bits.len().next_multiple_of(8);
    let padded = format!("{:0>width$}", bits, width = padded_len);

    let mut bytes = Vec::with_capacity(padded_len / 8);
    for chunk in padded.as_bytes().chunks(8) {
        let byte_str = std::str::from_utf8(chunk).unwrap();
        bytes.push(u8::from_str_radix(byte_str, 2).unwrap());
    }
    Ok(bytes)
}

pub(crate) fn format_bytes(
    bytes: &[u8],
    format: ByteFormat,
    separator: &str,
    with_prefix: bool,
) -> String {
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

pub(crate) fn format_binary(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "0b0".to_string();
    }

    let bits: String = bytes.iter().map(|b| format!("{:08b}", b)).collect();
    format!("0b{}", bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_string_with_prefix() {
        let result = parse_bytes_input("0x48656c6c6f", Some(ByteFormat::Hex)).unwrap();
        assert_eq!(result, b"Hello");
    }

    #[test]
    fn parse_hex_string_without_prefix() {
        let result = parse_bytes_input("48656c6c6f", Some(ByteFormat::Hex)).unwrap();
        assert_eq!(result, b"Hello");
    }

    #[test]
    fn parse_hex_odd_length_pads_leading_zero() {
        let result = parse_bytes_input("0xf", Some(ByteFormat::Hex)).unwrap();
        assert_eq!(result, vec![0x0f]);
    }

    #[test]
    fn parse_byte_array_decimal() {
        let result =
            parse_bytes_input("[72, 101, 108, 108, 111]", Some(ByteFormat::Bytes)).unwrap();
        assert_eq!(result, b"Hello");
    }

    #[test]
    fn parse_byte_array_hex_elements() {
        let result =
            parse_bytes_input("[0x48, 0x65, 0x6c, 0x6c, 0x6f]", Some(ByteFormat::HexBytes))
                .unwrap();
        assert_eq!(result, b"Hello");
    }

    #[test]
    fn parse_byte_array_empty() {
        let result = parse_bytes_input("[]", Some(ByteFormat::Bytes)).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_byte_value_exceeds_255() {
        let err = parse_bytes_input("[256]", Some(ByteFormat::Bytes)).unwrap_err();
        assert!(err.contains("exceeds 255"));
    }

    #[test]
    fn parse_empty_input_rejected() {
        let err = parse_bytes_input("", Some(ByteFormat::Hex)).unwrap_err();
        assert!(err.contains("cannot be empty"));
    }

    #[test]
    fn parse_binary_with_prefix() {
        let result = parse_binary_input("0b01001000").unwrap();
        assert_eq!(result, vec![0x48]);
    }

    #[test]
    fn parse_binary_without_prefix() {
        let result = parse_binary_input("01001000").unwrap();
        assert_eq!(result, vec![0x48]);
    }

    #[test]
    fn parse_binary_with_underscores() {
        let result = parse_binary_input("0b0100_1000").unwrap();
        assert_eq!(result, vec![0x48]);
    }

    #[test]
    fn parse_binary_partial_byte_padded() {
        let result = parse_binary_input("0b1111").unwrap();
        assert_eq!(result, vec![0x0f]);
    }

    #[test]
    fn parse_binary_rejects_non_binary_chars() {
        let err = parse_binary_input("0b012").unwrap_err();
        assert!(err.contains("must contain only 0 and 1"));
    }

    #[test]
    fn format_hex_nonempty() {
        let out = format_bytes(&[0x12, 0x34, 0x56], ByteFormat::Hex, ",", true);
        assert_eq!(out, "0x123456");
    }

    #[test]
    fn format_hex_empty() {
        let out = format_bytes(&[], ByteFormat::Hex, ",", true);
        assert_eq!(out, "0x0");
    }

    #[test]
    fn format_hex_bytes_with_prefix() {
        let out = format_bytes(&[0x12, 0x34], ByteFormat::HexBytes, ",", true);
        assert_eq!(out, "[0x12,0x34]");
    }

    #[test]
    fn format_hex_bytes_without_prefix() {
        let out = format_bytes(&[0x12, 0x34], ByteFormat::HexBytes, ",", false);
        assert_eq!(out, "[12,34]");
    }

    #[test]
    fn format_bytes_decimal() {
        let out = format_bytes(&[18, 52, 86], ByteFormat::Bytes, " ", true);
        assert_eq!(out, "[18 52 86]");
    }

    #[test]
    fn format_binary_nonempty() {
        let out = format_binary(&[0x48]);
        assert_eq!(out, "0b01001000");
    }

    #[test]
    fn format_binary_empty() {
        let out = format_binary(&[]);
        assert_eq!(out, "0b0");
    }
}
