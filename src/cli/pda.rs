//! PDA (Program Derived Address) derivation command.

use crate::utils::parse_hex_data;
use clap::Args;
use solana_pubkey::Pubkey;
use std::str::FromStr;

#[derive(Args, Debug)]
pub struct PdaArgs {
    /// The program ID to derive the PDA from
    #[arg(value_name = "PROGRAM_ID")]
    pub program_id: String,

    /// Seeds in format: type:value (repeatable), e.g. string:hello pubkey:<PUBKEY>
    ///
    /// Seed types with aliases in parentheses:
    ///   string (str) · pubkey (pk) · bool · u8 · u16 · u32 · u64 · u128
    ///   i8 · i16 · i32 · i64 · i128 · bytes (hex)
    #[arg(value_name = "SEED", num_args = 1.., required = true)]
    pub seeds: Vec<String>,
}

/// Supported seed types for PDA derivation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeedType {
    /// UTF-8 string, converted to bytes
    String,
    /// Base58-encoded Solana pubkey, converted to 32 bytes
    Pubkey,
    /// Boolean, encoded as a single byte (true=1, false=0)
    Bool,
    /// Unsigned 8-bit integer
    U8,
    /// Unsigned 16-bit integer, little-endian
    U16,
    /// Unsigned 32-bit integer, little-endian
    U32,
    /// Unsigned 64-bit integer, little-endian
    U64,
    /// Unsigned 128-bit integer, little-endian
    U128,
    /// Signed 8-bit integer
    I8,
    /// Signed 16-bit integer, little-endian
    I16,
    /// Signed 32-bit integer, little-endian
    I32,
    /// Signed 64-bit integer, little-endian
    I64,
    /// Signed 128-bit integer, little-endian
    I128,
    /// Hex-encoded raw bytes
    Bytes,
}

impl FromStr for SeedType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "string" | "str" => Ok(SeedType::String),
            "pubkey" | "publickey" | "pk" => Ok(SeedType::Pubkey),
            "bool" => Ok(SeedType::Bool),
            "u8" => Ok(SeedType::U8),
            "u16" => Ok(SeedType::U16),
            "u32" => Ok(SeedType::U32),
            "u64" => Ok(SeedType::U64),
            "u128" => Ok(SeedType::U128),
            "i8" => Ok(SeedType::I8),
            "i16" => Ok(SeedType::I16),
            "i32" => Ok(SeedType::I32),
            "i64" => Ok(SeedType::I64),
            "i128" => Ok(SeedType::I128),
            "bytes" | "hex" => Ok(SeedType::Bytes),
            _ => Err(format!(
                "Unknown seed type: '{}'. Supported: string (str), pubkey (pk, publickey), bool, u8, u16, u32, u64, u128, i8, i16, i32, i64, i128, bytes (hex)",
                s
            )),
        }
    }
}

/// A parsed seed with its value and type.
#[derive(Debug, Clone)]
pub struct ParsedSeed {
    pub value: String,
    pub seed_type: SeedType,
}

/// Parse seed arguments into a vector of ParsedSeed.
/// Format: "type:value", provided as repeatable positional args.
/// Example: ["string:position", "pubkey:9msbbNFZaK9hGEBiWvdAXdN7YgHUKVeT5APk2b4r6rR6"]
pub fn parse_seeds(inputs: &[String]) -> Result<Vec<ParsedSeed>, String> {
    if inputs.is_empty() {
        return Err("At least one seed is required. Use format 'type:value'".to_string());
    }

    let mut seeds = Vec::new();

    for raw in inputs {
        let part = raw.trim();
        let (type_str, value) = part
            .split_once(':')
            .ok_or_else(|| format!("Invalid seed format '{}': expected 'type:value'", part))?;
        let type_str = type_str.trim();
        let value = value.trim();

        if type_str.is_empty() {
            return Err(format!("Empty seed type in '{}'", part));
        }

        if value.is_empty() {
            return Err(format!("Empty seed value in '{}'", part));
        }

        let seed_type = SeedType::from_str(type_str)?;

        seeds.push(ParsedSeed { value: value.to_string(), seed_type });
    }

    Ok(seeds)
}

/// Parse an integer from a string and return its little-endian bytes.
fn parse_int<T>(value: &str) -> Result<Vec<u8>, String>
where
    T: std::str::FromStr + ToLeBytes,
    T::Err: std::fmt::Display,
{
    let v = value
        .parse::<T>()
        .map_err(|e| format!("Invalid {} '{}': {}", std::any::type_name::<T>(), value, e))?;
    Ok(v.to_le_bytes_vec())
}

trait ToLeBytes {
    fn to_le_bytes_vec(self) -> Vec<u8>;
}

macro_rules! impl_to_le_bytes {
    ($($t:ty),*) => {
        $(impl ToLeBytes for $t {
            fn to_le_bytes_vec(self) -> Vec<u8> { self.to_le_bytes().to_vec() }
        })*
    };
}

impl_to_le_bytes!(u8, u16, u32, u64, u128, i8, i16, i32, i64, i128);

/// Convert parsed seeds to byte vectors for PDA derivation.
pub fn seeds_to_bytes(seeds: &[ParsedSeed]) -> Result<Vec<Vec<u8>>, String> {
    seeds
        .iter()
        .map(|seed| match seed.seed_type {
            SeedType::String => Ok(seed.value.as_bytes().to_vec()),
            SeedType::Pubkey => {
                let pubkey = Pubkey::from_str(&seed.value)
                    .map_err(|e| format!("Invalid pubkey '{}': {}", seed.value, e))?;
                Ok(pubkey.to_bytes().to_vec())
            }
            SeedType::Bool => match seed.value.as_str() {
                "true" | "1" => Ok(vec![1]),
                "false" | "0" => Ok(vec![0]),
                _ => Err(format!("Invalid bool '{}': expected true/false or 1/0", seed.value)),
            },
            SeedType::U8 => parse_int::<u8>(&seed.value),
            SeedType::U16 => parse_int::<u16>(&seed.value),
            SeedType::U32 => parse_int::<u32>(&seed.value),
            SeedType::U64 => parse_int::<u64>(&seed.value),
            SeedType::U128 => parse_int::<u128>(&seed.value),
            SeedType::I8 => parse_int::<i8>(&seed.value),
            SeedType::I16 => parse_int::<i16>(&seed.value),
            SeedType::I32 => parse_int::<i32>(&seed.value),
            SeedType::I64 => parse_int::<i64>(&seed.value),
            SeedType::I128 => parse_int::<i128>(&seed.value),
            SeedType::Bytes => parse_hex_data(&seed.value),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_seeds_single_string() {
        let seeds = parse_seeds(&["string:position".to_string()]).unwrap();
        assert_eq!(seeds.len(), 1);
        assert_eq!(seeds[0].value, "position");
        assert_eq!(seeds[0].seed_type, SeedType::String);
    }

    #[test]
    fn parse_seeds_multiple() {
        let seeds = parse_seeds(&[
            "string:position".to_string(),
            "pubkey:9msbbNFZaK9hGEBiWvdAXdN7YgHUKVeT5APk2b4r6rR6".to_string(),
        ])
        .unwrap();
        assert_eq!(seeds.len(), 2);
        assert_eq!(seeds[0].value, "position");
        assert_eq!(seeds[0].seed_type, SeedType::String);
        assert_eq!(seeds[1].value, "9msbbNFZaK9hGEBiWvdAXdN7YgHUKVeT5APk2b4r6rR6");
        assert_eq!(seeds[1].seed_type, SeedType::Pubkey);
    }

    #[test]
    fn parse_seeds_with_whitespace() {
        let seeds =
            parse_seeds(&["  string : position  ".to_string(), " pubkey : key ".to_string()])
                .unwrap();
        assert_eq!(seeds.len(), 2);
        assert_eq!(seeds[0].value, "position");
        assert_eq!(seeds[1].value, "key");
    }

    #[test]
    fn parse_seeds_alias_type() {
        let seeds = parse_seeds(&[
            "str:hello".to_string(),
            "pk:11111111111111111111111111111111".to_string(),
        ])
        .unwrap();
        assert_eq!(seeds[0].seed_type, SeedType::String);
        assert_eq!(seeds[1].seed_type, SeedType::Pubkey);
    }

    #[test]
    fn parse_seeds_u64_and_u8_values() {
        let seeds = parse_seeds(&["u64:42".to_string(), "u8:7".to_string()]).unwrap();

        assert_eq!(seeds.len(), 2);
        assert_eq!(seeds[0].value, "42");
        assert_eq!(seeds[0].seed_type, SeedType::U64);
        assert_eq!(seeds[1].value, "7");
        assert_eq!(seeds[1].seed_type, SeedType::U8);
    }

    #[test]
    fn parse_seeds_empty_input() {
        let result = parse_seeds(&[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("At least one seed is required"));
    }

    #[test]
    fn parse_seeds_invalid_format() {
        let result = parse_seeds(&["position".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected 'type:value'"));
    }

    #[test]
    fn parse_seeds_empty_type() {
        let result = parse_seeds(&[":position".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty seed type"));
    }

    #[test]
    fn parse_seeds_unknown_type() {
        let result = parse_seeds(&["unknown:position".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown seed type"));
    }

    #[test]
    fn parse_seeds_all_types() {
        let seeds = parse_seeds(&[
            "bool:true".to_string(),
            "u16:1000".to_string(),
            "u32:100000".to_string(),
            "u128:99".to_string(),
            "i8:-1".to_string(),
            "i16:-100".to_string(),
            "i32:-100000".to_string(),
            "i64:-42".to_string(),
            "i128:-99".to_string(),
            "bytes:deadbeef".to_string(),
            "hex:0xcafe".to_string(),
        ])
        .unwrap();
        assert_eq!(seeds[0].seed_type, SeedType::Bool);
        assert_eq!(seeds[1].seed_type, SeedType::U16);
        assert_eq!(seeds[2].seed_type, SeedType::U32);
        assert_eq!(seeds[3].seed_type, SeedType::U128);
        assert_eq!(seeds[4].seed_type, SeedType::I8);
        assert_eq!(seeds[5].seed_type, SeedType::I16);
        assert_eq!(seeds[6].seed_type, SeedType::I32);
        assert_eq!(seeds[7].seed_type, SeedType::I64);
        assert_eq!(seeds[8].seed_type, SeedType::I128);
        assert_eq!(seeds[9].seed_type, SeedType::Bytes);
        assert_eq!(seeds[10].seed_type, SeedType::Bytes);
    }

    #[test]
    fn parse_seeds_removed_types_are_rejected() {
        let u64be_result = parse_seeds(&["u64be:42".to_string()]);
        assert!(u64be_result.is_err());
        assert!(u64be_result.unwrap_err().contains("Unknown seed type"));
    }

    #[test]
    fn seeds_to_bytes_string() {
        let seeds = vec![ParsedSeed { value: "hello".to_string(), seed_type: SeedType::String }];
        let bytes = seeds_to_bytes(&seeds).unwrap();
        assert_eq!(bytes.len(), 1);
        assert_eq!(bytes[0], b"hello".to_vec());
    }

    #[test]
    fn seeds_to_bytes_pubkey() {
        let pubkey_str = "11111111111111111111111111111111";
        let seeds = vec![ParsedSeed { value: pubkey_str.to_string(), seed_type: SeedType::Pubkey }];
        let bytes = seeds_to_bytes(&seeds).unwrap();
        assert_eq!(bytes.len(), 1);
        assert_eq!(bytes[0].len(), 32);
    }

    #[test]
    fn seeds_to_bytes_invalid_pubkey() {
        let seeds = vec![ParsedSeed { value: "invalid".to_string(), seed_type: SeedType::Pubkey }];
        let result = seeds_to_bytes(&seeds);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid pubkey"));
    }

    #[test]
    fn seeds_to_bytes_bool() {
        let t = seeds_to_bytes(&parse_seeds(&["bool:true".to_string()]).unwrap()).unwrap();
        let f = seeds_to_bytes(&parse_seeds(&["bool:0".to_string()]).unwrap()).unwrap();
        assert_eq!(t[0], vec![1]);
        assert_eq!(f[0], vec![0]);
    }

    #[test]
    fn seeds_to_bytes_bool_invalid() {
        let seeds = parse_seeds(&["bool:yes".to_string()]).unwrap();
        assert!(seeds_to_bytes(&seeds).is_err());
    }

    #[test]
    fn seeds_to_bytes_integers() {
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("u8:42", vec![42]),
            ("u16:1000", 1000_u16.to_le_bytes().to_vec()),
            ("u32:100000", 100000_u32.to_le_bytes().to_vec()),
            ("u64:42", 42_u64.to_le_bytes().to_vec()),
            ("u128:99", 99_u128.to_le_bytes().to_vec()),
            ("i8:-1", (-1_i8).to_le_bytes().to_vec()),
            ("i16:-100", (-100_i16).to_le_bytes().to_vec()),
            ("i32:-100000", (-100000_i32).to_le_bytes().to_vec()),
            ("i64:-42", (-42_i64).to_le_bytes().to_vec()),
            ("i128:-99", (-99_i128).to_le_bytes().to_vec()),
        ];
        for (input, expected) in cases {
            let seeds = parse_seeds(&[input.to_string()]).unwrap();
            let bytes = seeds_to_bytes(&seeds).unwrap();
            assert_eq!(bytes[0], expected, "failed for {}", input);
        }
    }

    #[test]
    fn seeds_to_bytes_integer_overflow() {
        let seeds = parse_seeds(&["u8:999".to_string()]).unwrap();
        assert!(seeds_to_bytes(&seeds).is_err());
    }

    #[test]
    fn seeds_to_bytes_integer_invalid() {
        let seeds = parse_seeds(&["u64:not_a_number".to_string()]).unwrap();
        assert!(seeds_to_bytes(&seeds).is_err());
    }

    #[test]
    fn seeds_to_bytes_hex() {
        let seeds = parse_seeds(&["bytes:deadbeef".to_string()]).unwrap();
        let bytes = seeds_to_bytes(&seeds).unwrap();
        assert_eq!(bytes[0], vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn seeds_to_bytes_hex_0x_prefix() {
        let seeds = parse_seeds(&["hex:0xcafe".to_string()]).unwrap();
        let bytes = seeds_to_bytes(&seeds).unwrap();
        assert_eq!(bytes[0], vec![0xca, 0xfe]);
    }

    #[test]
    fn seeds_to_bytes_hex_odd_length() {
        let seeds = parse_seeds(&["bytes:abc".to_string()]).unwrap();
        let result = seeds_to_bytes(&seeds);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("odd length"));
    }

    #[test]
    fn seeds_to_bytes_hex_invalid() {
        let seeds = parse_seeds(&["bytes:zzzz".to_string()]).unwrap();
        let result = seeds_to_bytes(&seeds);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid hex"));
    }
}
