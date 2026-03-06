use std::io::{IsTerminal, Read};

use anyhow::{Context, Result};

use crate::cli::{BorshArgs, BorshCommands, BorshDeArgs, BorshSerArgs};
use crate::converters::borsh_decode::decode_borsh;
use crate::converters::borsh_encode::encode_borsh;
use crate::converters::borsh_type::parse_borsh_type;

pub(crate) fn handle(args: BorshArgs) -> Result<()> {
    match args.command {
        BorshCommands::De(args) => handle_de(args),
        BorshCommands::Ser(args) => handle_ser(args),
    }
}

fn handle_de(args: BorshDeArgs) -> Result<()> {
    let ty = parse_borsh_type(&args.type_str)
        .map_err(|e| anyhow::anyhow!("invalid type descriptor: {e}"))?;

    let raw = read_input(args.input.as_deref(), "bytes")?;
    let data = parse_input_bytes(&raw)?;

    if args.skip_bytes > data.len() {
        anyhow::bail!(
            "skip-bytes ({}) exceeds input length ({} bytes)",
            args.skip_bytes,
            data.len()
        );
    }

    let mut offset = args.skip_bytes;
    let value =
        decode_borsh(&ty, &data, &mut offset).context("failed to deserialize borsh data")?;

    if offset < data.len() {
        log::warn!(
            "{} unconsumed byte(s) remaining after deserialization (consumed {offset} of {})",
            data.len() - offset,
            data.len()
        );
    }

    let output = serde_json::to_string_pretty(&value)?;
    println!("{output}");
    Ok(())
}

fn handle_ser(args: BorshSerArgs) -> Result<()> {
    let ty = parse_borsh_type(&args.type_str)
        .map_err(|e| anyhow::anyhow!("invalid type descriptor: {e}"))?;

    let raw = read_input(args.input.as_deref(), "JSON")?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).context("failed to parse JSON input")?;

    let bytes = encode_borsh(&ty, &value).context("failed to serialize to borsh")?;

    println!("0x{}", hex::encode(&bytes));
    Ok(())
}

fn read_input(input: Option<&str>, kind: &str) -> Result<String> {
    if let Some(input) = input {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            anyhow::bail!("input cannot be empty");
        }
        return Ok(trimmed.to_owned());
    }

    if !std::io::stdin().is_terminal() {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf).context("failed to read from stdin")?;
        let trimmed = buf.trim();
        if trimmed.is_empty() {
            anyhow::bail!("no {kind} data received from stdin");
        }
        return Ok(trimmed.to_owned());
    }

    anyhow::bail!("no input provided. Pass {kind} as a positional argument or pipe via stdin")
}

fn parse_input_bytes(input: &str) -> Result<Vec<u8>> {
    // Try hex with 0x prefix
    if input.starts_with("0x") || input.starts_with("0X") {
        let hex_str: String = input[2..].chars().filter(|c| !c.is_whitespace()).collect();
        let hex_str = if hex_str.len() % 2 != 0 { format!("0{hex_str}") } else { hex_str };
        return hex::decode(&hex_str).context("invalid hex input");
    }

    // Try byte array [1,2,3,...]
    if input.starts_with('[') && input.ends_with(']') {
        let inner = input[1..input.len() - 1].trim();
        if inner.is_empty() {
            return Ok(Vec::new());
        }
        let bytes: Result<Vec<u8>> = inner
            .split(',')
            .map(|s| {
                let s = s.trim();
                if s.starts_with("0x") || s.starts_with("0X") {
                    u8::from_str_radix(&s[2..], 16)
                        .with_context(|| format!("invalid hex byte: {s}"))
                } else {
                    s.parse::<u8>().with_context(|| format!("invalid byte value: {s}"))
                }
            })
            .collect();
        return bytes;
    }

    // Try base64
    use base64::Engine;
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(input) {
        return Ok(bytes);
    }

    anyhow::bail!("unrecognized input format. Expected hex (0x...), byte array ([...]), or base64")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_input() {
        assert_eq!(parse_input_bytes("0x0100000000000000").unwrap(), 1u64.to_le_bytes());
    }

    #[test]
    fn parse_byte_array_input() {
        assert_eq!(parse_input_bytes("[1,0,0,0,0,0,0,0]").unwrap(), 1u64.to_le_bytes());
    }

    #[test]
    fn parse_base64_input() {
        let bytes = parse_input_bytes("AQAAAAAAAAA=").unwrap();
        assert_eq!(bytes, 1u64.to_le_bytes());
    }

    #[test]
    fn parse_invalid_input() {
        assert!(parse_input_bytes("not-valid-anything").is_err());
    }
}
