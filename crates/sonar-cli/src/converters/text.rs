pub(crate) fn bytes_to_utf8(bytes: &[u8], escape_invalid: bool) -> String {
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

pub(crate) fn format_base64_error(err: &base64::DecodeError) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_utf8_passthrough() {
        assert_eq!(bytes_to_utf8(b"Hello", false), "Hello");
        assert_eq!(bytes_to_utf8(b"Hello", true), "Hello");
    }

    #[test]
    fn invalid_utf8_lossy() {
        let bytes = &[0xff, 0xfe];
        let result = bytes_to_utf8(bytes, false);
        assert!(result.contains('\u{FFFD}'));
    }

    #[test]
    fn invalid_utf8_escaped() {
        let bytes = &[0xff];
        let result = bytes_to_utf8(bytes, true);
        assert_eq!(result, "\\xff");
    }

    #[test]
    fn mixed_valid_invalid_escaped() {
        let bytes = &[b'H', b'i', 0xff, b'!'];
        let result = bytes_to_utf8(bytes, true);
        assert_eq!(result, "Hi\\xff!");
    }

    #[test]
    fn empty_input() {
        assert_eq!(bytes_to_utf8(&[], false), "");
        assert_eq!(bytes_to_utf8(&[], true), "");
    }

    #[test]
    fn format_base64_invalid_byte_printable() {
        let err = base64::DecodeError::InvalidByte(5, b'!');
        let msg = format_base64_error(&err);
        assert!(msg.contains("'!'"));
        assert!(msg.contains("position 5"));
    }

    #[test]
    fn format_base64_invalid_byte_non_printable() {
        let err = base64::DecodeError::InvalidByte(3, 0x01);
        let msg = format_base64_error(&err);
        assert!(msg.contains("0x01"));
    }
}
