//! PDA (Program Derived Address) derivation command.

use clap::Args;
use solana_pubkey::Pubkey;
use std::str::FromStr;

#[derive(Args, Debug)]
pub struct PdaArgs {
    /// The program ID to derive the PDA from
    #[arg(value_name = "PROGRAM_ID")]
    pub program_id: String,

    /// Seeds in format: value:type,value:type,...
    /// Types: string, pubkey
    #[arg(value_name = "SEEDS")]
    pub seeds: String,
}

/// Supported seed types for PDA derivation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeedType {
    /// UTF-8 string, converted to bytes
    String,
    /// Base58-encoded Solana pubkey, converted to 32 bytes
    Pubkey,
}

impl FromStr for SeedType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "string" | "str" => Ok(SeedType::String),
            "pubkey" | "publickey" | "pk" => Ok(SeedType::Pubkey),
            _ => Err(format!("Unknown seed type: '{}'. Supported types: string, pubkey", s)),
        }
    }
}

/// A parsed seed with its value and type.
#[derive(Debug, Clone)]
pub struct ParsedSeed {
    pub value: String,
    pub seed_type: SeedType,
}

/// Parse the seeds string into a vector of ParsedSeed.
/// Format: "value:type,value:type,..."
/// Example: "position:string,9msbbNFZaK9hGEBiWvdAXdN7YgHUKVeT5APk2b4r6rR6:pubkey"
pub fn parse_seeds(input: &str) -> Result<Vec<ParsedSeed>, String> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let mut seeds = Vec::new();

    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Find the last colon to split value and type
        // This allows colons in the value part (though unusual)
        let last_colon = part
            .rfind(':')
            .ok_or_else(|| format!("Invalid seed format '{}': expected 'value:type'", part))?;

        let value = part[..last_colon].to_string();
        let type_str = part[last_colon + 1..].trim();

        if value.is_empty() {
            return Err(format!("Empty seed value in '{}'", part));
        }

        let seed_type = SeedType::from_str(type_str)?;

        seeds.push(ParsedSeed { value, seed_type });
    }

    Ok(seeds)
}

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
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_seeds_single_string() {
        let seeds = parse_seeds("position:string").unwrap();
        assert_eq!(seeds.len(), 1);
        assert_eq!(seeds[0].value, "position");
        assert_eq!(seeds[0].seed_type, SeedType::String);
    }

    #[test]
    fn parse_seeds_multiple() {
        let seeds =
            parse_seeds("position:string,9msbbNFZaK9hGEBiWvdAXdN7YgHUKVeT5APk2b4r6rR6:pubkey")
                .unwrap();
        assert_eq!(seeds.len(), 2);
        assert_eq!(seeds[0].value, "position");
        assert_eq!(seeds[0].seed_type, SeedType::String);
        assert_eq!(seeds[1].value, "9msbbNFZaK9hGEBiWvdAXdN7YgHUKVeT5APk2b4r6rR6");
        assert_eq!(seeds[1].seed_type, SeedType::Pubkey);
    }

    #[test]
    fn parse_seeds_with_whitespace() {
        let seeds = parse_seeds("  position : string , key : pubkey  ").unwrap();
        assert_eq!(seeds.len(), 2);
        assert_eq!(seeds[0].value, "position ");
        assert_eq!(seeds[1].value, "key ");
    }

    #[test]
    fn parse_seeds_empty() {
        let seeds = parse_seeds("").unwrap();
        assert!(seeds.is_empty());
    }

    #[test]
    fn parse_seeds_invalid_format() {
        let result = parse_seeds("position");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected 'value:type'"));
    }

    #[test]
    fn parse_seeds_unknown_type() {
        let result = parse_seeds("position:unknown");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown seed type"));
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
}
