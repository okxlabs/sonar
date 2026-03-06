/// Borsh type descriptor AST and parser.
///
/// Supports: u8/u16/u32/u64/u128, i8/i16/i32/i64/i128, bool, string, pubkey,
/// vec<T>, option<T>, [T;N], (T1,T2,...), (), enum<T0,T1,...>, result<T,E>.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BorshType {
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
    Bool,
    String,
    Pubkey,
    Vec(Box<BorshType>),
    Option(Box<BorshType>),
    Array(Box<BorshType>, usize),
    Tuple(Vec<BorshType>),
    Unit,
    Enum(Vec<BorshType>),
    Result(Box<BorshType>, Box<BorshType>),
}

impl std::fmt::Display for BorshType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BorshType::U8 => write!(f, "u8"),
            BorshType::U16 => write!(f, "u16"),
            BorshType::U32 => write!(f, "u32"),
            BorshType::U64 => write!(f, "u64"),
            BorshType::U128 => write!(f, "u128"),
            BorshType::I8 => write!(f, "i8"),
            BorshType::I16 => write!(f, "i16"),
            BorshType::I32 => write!(f, "i32"),
            BorshType::I64 => write!(f, "i64"),
            BorshType::I128 => write!(f, "i128"),
            BorshType::Bool => write!(f, "bool"),
            BorshType::String => write!(f, "string"),
            BorshType::Pubkey => write!(f, "pubkey"),
            BorshType::Vec(inner) => write!(f, "vec<{inner}>"),
            BorshType::Option(inner) => write!(f, "option<{inner}>"),
            BorshType::Array(inner, n) => write!(f, "[{inner};{n}]"),
            BorshType::Tuple(types) => {
                write!(f, "(")?;
                for (i, ty) in types.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{ty}")?;
                }
                write!(f, ")")
            }
            BorshType::Unit => write!(f, "()"),
            BorshType::Enum(variants) => {
                write!(f, "enum<")?;
                for (i, v) in variants.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, ">")
            }
            BorshType::Result(ok, err) => write!(f, "result<{ok},{err}>"),
        }
    }
}

/// Parse a type descriptor string into a `BorshType`.
pub fn parse_borsh_type(input: &str) -> Result<BorshType, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("empty type descriptor".to_string());
    }
    let mut pos = 0;
    let ty = parse_type(input, &mut pos)?;
    skip_whitespace(input, &mut pos);
    if pos != input.len() {
        return Err(format!(
            "unexpected trailing characters at position {pos}: '{}'",
            &input[pos..]
        ));
    }
    Ok(ty)
}

fn skip_whitespace(input: &str, pos: &mut usize) {
    while *pos < input.len() && input.as_bytes()[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
}

fn parse_type(input: &str, pos: &mut usize) -> Result<BorshType, String> {
    skip_whitespace(input, pos);
    if *pos >= input.len() {
        return Err(format!("unexpected end of type descriptor at position {pos}"));
    }

    let ch = input.as_bytes()[*pos];

    // Tuple: (T1, T2, ...)
    if ch == b'(' {
        return parse_tuple(input, pos);
    }

    // Fixed-size array: [T;N]
    if ch == b'[' {
        return parse_array(input, pos);
    }

    // Keyword-based types
    parse_keyword_type(input, pos)
}

fn parse_tuple(input: &str, pos: &mut usize) -> Result<BorshType, String> {
    *pos += 1; // skip '('
    let mut types = Vec::new();
    loop {
        skip_whitespace(input, pos);
        if *pos >= input.len() {
            return Err("unclosed tuple: expected ')'".to_string());
        }
        if input.as_bytes()[*pos] == b')' {
            *pos += 1;
            break;
        }
        if !types.is_empty() {
            if input.as_bytes()[*pos] != b',' {
                return Err(format!(
                    "expected ',' or ')' in tuple at position {pos}, got '{}'",
                    input.as_bytes()[*pos] as char
                ));
            }
            *pos += 1; // skip ','
        }
        types.push(parse_type(input, pos)?);
    }
    if types.is_empty() {
        return Ok(BorshType::Unit);
    }
    Ok(BorshType::Tuple(types))
}

fn parse_array(input: &str, pos: &mut usize) -> Result<BorshType, String> {
    *pos += 1; // skip '['
    let inner = parse_type(input, pos)?;
    skip_whitespace(input, pos);
    if *pos >= input.len() || input.as_bytes()[*pos] != b';' {
        return Err(format!("expected ';' in array type at position {pos}"));
    }
    *pos += 1; // skip ';'
    skip_whitespace(input, pos);

    // Parse the size number
    let start = *pos;
    while *pos < input.len() && input.as_bytes()[*pos].is_ascii_digit() {
        *pos += 1;
    }
    if start == *pos {
        return Err(format!("expected array size number at position {pos}"));
    }
    let size: usize = input[start..*pos]
        .parse()
        .map_err(|e| format!("invalid array size: {e}"))?;
    if size == 0 {
        return Err("array size must be > 0".to_string());
    }

    skip_whitespace(input, pos);
    if *pos >= input.len() || input.as_bytes()[*pos] != b']' {
        return Err(format!("expected ']' in array type at position {pos}"));
    }
    *pos += 1; // skip ']'
    Ok(BorshType::Array(Box::new(inner), size))
}

fn parse_keyword_type(input: &str, pos: &mut usize) -> Result<BorshType, String> {
    skip_whitespace(input, pos);

    // Collect keyword characters (alphanumeric)
    let start = *pos;
    while *pos < input.len() {
        let ch = input.as_bytes()[*pos];
        if ch.is_ascii_alphanumeric() || ch == b'_' {
            *pos += 1;
        } else {
            break;
        }
    }

    if start == *pos {
        return Err(format!(
            "expected type name at position {pos}, got '{}'",
            input.as_bytes()[*pos] as char
        ));
    }

    let keyword = &input[start..*pos];
    let keyword_lower = keyword.to_ascii_lowercase();

    match keyword_lower.as_str() {
        "u8" => Ok(BorshType::U8),
        "u16" => Ok(BorshType::U16),
        "u32" => Ok(BorshType::U32),
        "u64" => Ok(BorshType::U64),
        "u128" => Ok(BorshType::U128),
        "i8" => Ok(BorshType::I8),
        "i16" => Ok(BorshType::I16),
        "i32" => Ok(BorshType::I32),
        "i64" => Ok(BorshType::I64),
        "i128" => Ok(BorshType::I128),
        "bool" => Ok(BorshType::Bool),
        "string" | "str" => Ok(BorshType::String),
        "pubkey" => Ok(BorshType::Pubkey),
        "vec" => {
            skip_whitespace(input, pos);
            expect_char(input, pos, '<', "vec")?;
            let inner = parse_type(input, pos)?;
            skip_whitespace(input, pos);
            expect_char(input, pos, '>', "vec")?;
            Ok(BorshType::Vec(Box::new(inner)))
        }
        "option" => {
            skip_whitespace(input, pos);
            expect_char(input, pos, '<', "option")?;
            let inner = parse_type(input, pos)?;
            skip_whitespace(input, pos);
            expect_char(input, pos, '>', "option")?;
            Ok(BorshType::Option(Box::new(inner)))
        }
        "enum" => {
            skip_whitespace(input, pos);
            expect_char(input, pos, '<', "enum")?;
            let mut variants = Vec::new();
            loop {
                skip_whitespace(input, pos);
                if *pos < input.len() && input.as_bytes()[*pos] == b'>' {
                    *pos += 1;
                    break;
                }
                if !variants.is_empty() {
                    if *pos >= input.len() || input.as_bytes()[*pos] != b',' {
                        return Err(format!(
                            "expected ',' or '>' in enum at position {pos}, got '{}'",
                            if *pos < input.len() { input.as_bytes()[*pos] as char } else { '?' }
                        ));
                    }
                    *pos += 1; // skip ','
                }
                variants.push(parse_type(input, pos)?);
            }
            if variants.is_empty() {
                return Err("enum must have at least one variant".to_string());
            }
            Ok(BorshType::Enum(variants))
        }
        "result" => {
            skip_whitespace(input, pos);
            expect_char(input, pos, '<', "result")?;
            let ok_type = parse_type(input, pos)?;
            skip_whitespace(input, pos);
            if *pos >= input.len() || input.as_bytes()[*pos] != b',' {
                return Err(format!("expected ',' in result type at position {pos}"));
            }
            *pos += 1; // skip ','
            let err_type = parse_type(input, pos)?;
            skip_whitespace(input, pos);
            expect_char(input, pos, '>', "result")?;
            Ok(BorshType::Result(Box::new(ok_type), Box::new(err_type)))
        }
        _ => Err(format!("unknown type '{keyword}' at position {start}")),
    }
}

fn expect_char(input: &str, pos: &mut usize, expected: char, context: &str) -> Result<(), String> {
    if *pos >= input.len() || input.as_bytes()[*pos] != expected as u8 {
        let got = if *pos < input.len() {
            format!("'{}'", input.as_bytes()[*pos] as char)
        } else {
            "end of input".to_string()
        };
        return Err(format!(
            "expected '{expected}' for {context} at position {pos}, got {got}"
        ));
    }
    *pos += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_primitives() {
        assert_eq!(parse_borsh_type("u8").unwrap(), BorshType::U8);
        assert_eq!(parse_borsh_type("u16").unwrap(), BorshType::U16);
        assert_eq!(parse_borsh_type("u32").unwrap(), BorshType::U32);
        assert_eq!(parse_borsh_type("u64").unwrap(), BorshType::U64);
        assert_eq!(parse_borsh_type("u128").unwrap(), BorshType::U128);
        assert_eq!(parse_borsh_type("i8").unwrap(), BorshType::I8);
        assert_eq!(parse_borsh_type("i16").unwrap(), BorshType::I16);
        assert_eq!(parse_borsh_type("i32").unwrap(), BorshType::I32);
        assert_eq!(parse_borsh_type("i64").unwrap(), BorshType::I64);
        assert_eq!(parse_borsh_type("i128").unwrap(), BorshType::I128);
        assert_eq!(parse_borsh_type("bool").unwrap(), BorshType::Bool);
        assert_eq!(parse_borsh_type("string").unwrap(), BorshType::String);
        assert_eq!(parse_borsh_type("pubkey").unwrap(), BorshType::Pubkey);
    }

    #[test]
    fn parse_case_insensitive() {
        assert_eq!(parse_borsh_type("U64").unwrap(), BorshType::U64);
        assert_eq!(parse_borsh_type("Bool").unwrap(), BorshType::Bool);
        assert_eq!(parse_borsh_type("Pubkey").unwrap(), BorshType::Pubkey);
    }

    #[test]
    fn parse_vec() {
        assert_eq!(
            parse_borsh_type("vec<u32>").unwrap(),
            BorshType::Vec(Box::new(BorshType::U32))
        );
    }

    #[test]
    fn parse_option() {
        assert_eq!(
            parse_borsh_type("option<u64>").unwrap(),
            BorshType::Option(Box::new(BorshType::U64))
        );
    }

    #[test]
    fn parse_nested_vec() {
        assert_eq!(
            parse_borsh_type("vec<vec<u8>>").unwrap(),
            BorshType::Vec(Box::new(BorshType::Vec(Box::new(BorshType::U8))))
        );
    }

    #[test]
    fn parse_array() {
        assert_eq!(
            parse_borsh_type("[u8;32]").unwrap(),
            BorshType::Array(Box::new(BorshType::U8), 32)
        );
    }

    #[test]
    fn parse_tuple() {
        assert_eq!(
            parse_borsh_type("(u64,bool)").unwrap(),
            BorshType::Tuple(vec![BorshType::U64, BorshType::Bool])
        );
    }

    #[test]
    fn parse_complex_tuple() {
        assert_eq!(
            parse_borsh_type("(u64, bool, vec<u32>)").unwrap(),
            BorshType::Tuple(vec![
                BorshType::U64,
                BorshType::Bool,
                BorshType::Vec(Box::new(BorshType::U32)),
            ])
        );
    }

    #[test]
    fn parse_whitespace_tolerance() {
        assert_eq!(
            parse_borsh_type("  vec < u32 >  ").unwrap(),
            BorshType::Vec(Box::new(BorshType::U32))
        );
        assert_eq!(
            parse_borsh_type("[ u8 ; 4 ]").unwrap(),
            BorshType::Array(Box::new(BorshType::U8), 4)
        );
    }

    #[test]
    fn parse_str_alias() {
        assert_eq!(parse_borsh_type("str").unwrap(), BorshType::String);
    }

    #[test]
    fn parse_empty_rejected() {
        assert!(parse_borsh_type("").is_err());
    }

    #[test]
    fn parse_unknown_type() {
        assert!(parse_borsh_type("float32").is_err());
    }

    #[test]
    fn parse_trailing_chars() {
        assert!(parse_borsh_type("u64 extra").is_err());
    }

    #[test]
    fn parse_zero_size_array_rejected() {
        assert!(parse_borsh_type("[u8;0]").is_err());
    }

    #[test]
    fn display_roundtrip() {
        let ty = parse_borsh_type("(u64,bool,vec<u32>)").unwrap();
        let s = ty.to_string();
        let ty2 = parse_borsh_type(&s).unwrap();
        assert_eq!(ty, ty2);
    }

    #[test]
    fn parse_unit() {
        assert_eq!(parse_borsh_type("()").unwrap(), BorshType::Unit);
    }

    #[test]
    fn parse_enum() {
        assert_eq!(
            parse_borsh_type("enum<(),u64,(u32,bool)>").unwrap(),
            BorshType::Enum(vec![
                BorshType::Unit,
                BorshType::U64,
                BorshType::Tuple(vec![BorshType::U32, BorshType::Bool]),
            ])
        );
    }

    #[test]
    fn parse_enum_single_variant() {
        assert_eq!(
            parse_borsh_type("enum<u64>").unwrap(),
            BorshType::Enum(vec![BorshType::U64])
        );
    }

    #[test]
    fn parse_result() {
        assert_eq!(
            parse_borsh_type("result<u64, string>").unwrap(),
            BorshType::Result(Box::new(BorshType::U64), Box::new(BorshType::String))
        );
    }

    #[test]
    fn display_unit() {
        assert_eq!(BorshType::Unit.to_string(), "()");
    }

    #[test]
    fn display_enum_roundtrip() {
        let ty = parse_borsh_type("enum<(),u64>").unwrap();
        let s = ty.to_string();
        let ty2 = parse_borsh_type(&s).unwrap();
        assert_eq!(ty, ty2);
    }

    #[test]
    fn display_result_roundtrip() {
        let ty = parse_borsh_type("result<u64,string>").unwrap();
        let s = ty.to_string();
        let ty2 = parse_borsh_type(&s).unwrap();
        assert_eq!(ty, ty2);
    }
}
