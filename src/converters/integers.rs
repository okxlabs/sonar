use super::types::{ConvertValue, FixedIntSpec};
use num_bigint::{BigInt, BigUint, Sign};

pub(crate) fn parse_number(input: &str) -> Result<num_bigint::BigUint, String> {
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

pub(crate) fn parse_fixed_integer(input: &str, spec: FixedIntSpec) -> Result<ConvertValue, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(format!("{} cannot be empty", spec.name()));
    }

    if spec.is_big() {
        if spec.is_signed() {
            let value = trimmed
                .parse::<BigInt>()
                .map_err(|e| format!("Invalid {} value '{}': {}", spec.name(), trimmed, e))?;
            let (min, max) = big_signed_bounds(spec.bits());
            if value < min || value > max {
                return Err(format!("{} value {} is out of range", spec.name(), value));
            }
            return Ok(ConvertValue::FixedBigSigned { value, bits: spec.bits() });
        } else {
            let value = trimmed
                .parse::<BigUint>()
                .map_err(|e| format!("Invalid {} value '{}': {}", spec.name(), trimmed, e))?;
            if value.bits() > u64::from(spec.bits()) {
                return Err(format!("{} value {} is out of range", spec.name(), value));
            }
            return Ok(ConvertValue::FixedBigUnsigned { value, bits: spec.bits() });
        }
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

pub(crate) fn format_fixed_integer(
    value: &ConvertValue,
    spec: FixedIntSpec,
    big_endian: bool,
) -> Result<String, String> {
    let out_of_range = |value: &str| format!("{} value {} is out of range", spec.name(), value);

    if spec.is_big() && spec.is_signed() {
        let (min, max) = big_signed_bounds(spec.bits());
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
                decode_big_signed(bytes, big_endian)
            }
            ConvertValue::Number(num) => BigInt::from(num.clone()),
            ConvertValue::FixedUnsigned { value, .. } => BigInt::from(*value),
            ConvertValue::FixedBigUnsigned { value, .. } => BigInt::from(value.clone()),
            ConvertValue::FixedBigSigned { value, .. } => value.clone(),
            ConvertValue::FixedSigned { value, .. } => BigInt::from(*value),
            ConvertValue::Lamports(value) => BigInt::from(*value),
        };

        if signed_value < min || signed_value > max {
            return Err(out_of_range(&signed_value.to_string()));
        }
        return Ok(signed_value.to_string());
    }

    if spec.is_big() {
        let max_bits = u64::from(spec.bits());
        let big_value = match value {
            ConvertValue::Bytes(bytes) => {
                if bytes.len() != spec.bytes() {
                    return Err(format!(
                        "{} requires exactly {} bytes, got {}",
                        spec.name(),
                        spec.bytes(),
                        bytes.len()
                    ));
                }
                if big_endian {
                    BigUint::from_bytes_be(bytes)
                } else {
                    BigUint::from_bytes_le(bytes)
                }
            }
            ConvertValue::Number(num) => num.clone(),
            ConvertValue::FixedUnsigned { value, .. } => BigUint::from(*value),
            ConvertValue::FixedBigUnsigned { value, .. } => value.clone(),
            ConvertValue::FixedBigSigned { value, .. } => {
                if value.sign() == Sign::Minus {
                    return Err(out_of_range(&value.to_string()));
                }
                value.to_biguint().unwrap()
            }
            ConvertValue::FixedSigned { value, .. } => {
                if *value < 0 {
                    return Err(out_of_range(&value.to_string()));
                }
                BigUint::from(*value as u128)
            }
            ConvertValue::Lamports(value) => BigUint::from(*value),
        };

        if big_value.bits() > max_bits {
            return Err(out_of_range(&big_value.to_string()));
        }
        return Ok(big_value.to_string());
    }

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
            ConvertValue::FixedBigUnsigned { value, .. } => {
                let raw = value.to_string();
                let as_u128 = biguint_to_u128(value).ok_or_else(|| out_of_range(&raw))?;
                let max_u128 =
                    u128::try_from(max).expect("signed max should always be non-negative");
                if as_u128 > max_u128 {
                    return Err(out_of_range(&raw));
                }
                as_u128 as i128
            }
            ConvertValue::FixedBigSigned { value, .. } => {
                let raw = value.to_string();
                let as_i128 = i128::try_from(value).map_err(|_| out_of_range(&raw))?;
                as_i128
            }
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
            ConvertValue::FixedBigUnsigned { value, .. } => {
                let raw = value.to_string();
                biguint_to_u128(value).ok_or_else(|| out_of_range(&raw))?
            }
            ConvertValue::FixedBigSigned { value, .. } => {
                let raw = value.to_string();
                if value.sign() == Sign::Minus {
                    return Err(out_of_range(&raw));
                }
                let as_u128 = u128::try_from(value).map_err(|_| out_of_range(&raw))?;
                as_u128
            }
            ConvertValue::Lamports(value) => u128::from(*value),
        };

        if unsigned_value > max {
            return Err(out_of_range(&unsigned_value.to_string()));
        }
        Ok(unsigned_value.to_string())
    }
}

pub(crate) fn value_to_bytes(value: &ConvertValue, big_endian: bool) -> Vec<u8> {
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
        ConvertValue::FixedBigUnsigned { value, bits } => {
            let width = usize::from(bits / 8);
            let be_bytes = value.to_bytes_be();
            let mut buf = vec![0u8; width];
            let offset = width.saturating_sub(be_bytes.len());
            buf[offset..].copy_from_slice(&be_bytes[be_bytes.len().saturating_sub(width)..]);
            if !big_endian {
                buf.reverse();
            }
            buf
        }
        ConvertValue::FixedBigSigned { value, bits } => {
            let width = usize::from(bits / 8);
            encode_big_signed(value, width, big_endian)
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

pub(crate) fn biguint_to_u128(value: &num_bigint::BigUint) -> Option<u128> {
    let bytes = value.to_bytes_be();
    if bytes.len() > 16 {
        return None;
    }
    let mut buf = [0u8; 16];
    buf[16 - bytes.len()..].copy_from_slice(&bytes);
    Some(u128::from_be_bytes(buf))
}

pub(crate) fn unsigned_max(bits: u16) -> u128 {
    if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 }
}

pub(crate) fn signed_bounds(bits: u16) -> (i128, i128) {
    if bits == 128 {
        (i128::MIN, i128::MAX)
    } else {
        let max = (1i128 << (bits - 1)) - 1;
        let min = -(1i128 << (bits - 1));
        (min, max)
    }
}

pub(crate) fn big_signed_bounds(bits: u16) -> (BigInt, BigInt) {
    let max = (BigInt::from(1) << (bits - 1)) - 1;
    let min = -(BigInt::from(1) << (bits - 1));
    (min, max)
}

fn encode_big_signed(value: &BigInt, width: usize, big_endian: bool) -> Vec<u8> {
    let mut buf = match value.sign() {
        Sign::Minus => vec![0xFFu8; width],
        _ => vec![0u8; width],
    };

    let (sign, magnitude) = value.to_bytes_be();
    if sign == Sign::Minus {
        // Two's complement: invert all bits and add 1
        let mut twos = vec![0u8; width];
        let offset = width.saturating_sub(magnitude.len());
        twos[offset..].copy_from_slice(&magnitude[magnitude.len().saturating_sub(width)..]);
        // Negate: flip bits then add 1
        for b in &mut twos {
            *b = !*b;
        }
        let mut carry = 1u16;
        for b in twos.iter_mut().rev() {
            let sum = u16::from(*b) + carry;
            *b = sum as u8;
            carry = sum >> 8;
        }
        buf = twos;
    } else {
        let be_bytes = magnitude;
        let offset = width.saturating_sub(be_bytes.len());
        buf[offset..].copy_from_slice(&be_bytes[be_bytes.len().saturating_sub(width)..]);
    }

    if !big_endian {
        buf.reverse();
    }
    buf
}

fn decode_big_signed(bytes: &[u8], big_endian: bool) -> BigInt {
    let ordered: Vec<u8> = if big_endian {
        bytes.to_vec()
    } else {
        let mut v = bytes.to_vec();
        v.reverse();
        v
    };

    // Check sign bit (MSB of first byte in big-endian)
    let negative = (ordered[0] & 0x80) != 0;
    if negative {
        // Two's complement: flip bits, add 1, negate
        let mut mag = ordered;
        for b in &mut mag {
            *b = !*b;
        }
        let mut carry = 1u16;
        for b in mag.iter_mut().rev() {
            let sum = u16::from(*b) + carry;
            *b = sum as u8;
            carry = sum >> 8;
        }
        -BigInt::from_bytes_be(Sign::Plus, &mag)
    } else {
        BigInt::from_bytes_be(Sign::Plus, &ordered)
    }
}

pub(crate) fn decode_fixed_unsigned(bytes: &[u8], big_endian: bool) -> u128 {
    let mut buf = [0u8; 16];
    if big_endian {
        buf[16 - bytes.len()..].copy_from_slice(bytes);
        u128::from_be_bytes(buf)
    } else {
        buf[..bytes.len()].copy_from_slice(bytes);
        u128::from_le_bytes(buf)
    }
}

pub(crate) fn decode_fixed_signed(bytes: &[u8], big_endian: bool) -> i128 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_decimal_number() {
        let num = parse_number("305419896").unwrap();
        assert_eq!(num.to_string(), "305419896");
    }

    #[test]
    fn parse_hex_number() {
        let num = parse_number("0x12345678").unwrap();
        assert_eq!(num.to_string(), "305419896");
    }

    #[test]
    fn parse_number_rejects_empty() {
        assert!(parse_number("").is_err());
    }

    #[test]
    fn parse_fixed_u8_boundary() {
        assert!(parse_fixed_integer("0", FixedIntSpec::U8).is_ok());
        assert!(parse_fixed_integer("255", FixedIntSpec::U8).is_ok());
        assert!(parse_fixed_integer("256", FixedIntSpec::U8).is_err());
    }

    #[test]
    fn parse_fixed_i8_boundary() {
        assert!(parse_fixed_integer("-128", FixedIntSpec::I8).is_ok());
        assert!(parse_fixed_integer("127", FixedIntSpec::I8).is_ok());
        assert!(parse_fixed_integer("-129", FixedIntSpec::I8).is_err());
        assert!(parse_fixed_integer("128", FixedIntSpec::I8).is_err());
    }

    #[test]
    fn unsigned_max_values() {
        assert_eq!(unsigned_max(8), 255);
        assert_eq!(unsigned_max(16), 65535);
        assert_eq!(unsigned_max(128), u128::MAX);
    }

    #[test]
    fn signed_bounds_values() {
        assert_eq!(signed_bounds(8), (-128, 127));
        assert_eq!(signed_bounds(16), (-32768, 32767));
    }

    #[test]
    fn value_to_bytes_fixed_unsigned_be() {
        let val = ConvertValue::FixedUnsigned { value: 0x1234, bits: 16 };
        assert_eq!(value_to_bytes(&val, true), vec![0x12, 0x34]);
    }

    #[test]
    fn value_to_bytes_fixed_unsigned_le() {
        let val = ConvertValue::FixedUnsigned { value: 0x1234, bits: 16 };
        assert_eq!(value_to_bytes(&val, false), vec![0x34, 0x12]);
    }

    #[test]
    fn decode_unsigned_be() {
        assert_eq!(decode_fixed_unsigned(&[0x12, 0x34], true), 0x1234);
    }

    #[test]
    fn decode_unsigned_le() {
        assert_eq!(decode_fixed_unsigned(&[0x34, 0x12], false), 0x1234);
    }

    #[test]
    fn decode_signed_negative_be() {
        assert_eq!(decode_fixed_signed(&[0xff, 0xfe], true), -2);
    }

    #[test]
    fn decode_signed_negative_le() {
        assert_eq!(decode_fixed_signed(&[0xfe, 0xff], false), -2);
    }

    #[test]
    fn biguint_to_u128_small() {
        let num = num_bigint::BigUint::from(42u64);
        assert_eq!(biguint_to_u128(&num), Some(42));
    }

    #[test]
    fn biguint_to_u128_too_large() {
        let max_plus_one = BigUint::from(u128::MAX) + BigUint::from(1u32);
        assert_eq!(biguint_to_u128(&max_plus_one), None);
    }

    #[test]
    fn format_fixed_unsigned_from_bytes_exact_width() {
        let val = ConvertValue::Bytes(vec![0x12, 0x34]);
        let result = format_fixed_integer(&val, FixedIntSpec::U16, true).unwrap();
        assert_eq!(result, "4660");
    }

    #[test]
    fn format_fixed_unsigned_rejects_wrong_width() {
        let val = ConvertValue::Bytes(vec![0x12]);
        let err = format_fixed_integer(&val, FixedIntSpec::U16, true).unwrap_err();
        assert!(err.contains("requires exactly 2 bytes"));
    }

    #[test]
    fn format_fixed_signed_from_bytes() {
        let val = ConvertValue::Bytes(vec![0xff, 0xfe]);
        let result = format_fixed_integer(&val, FixedIntSpec::I16, true).unwrap();
        assert_eq!(result, "-2");
    }
}
