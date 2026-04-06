//! Metaplex Token Metadata account decoder.
//!
//! This module decodes metadata PDA account data following the Metaplex
//! token-metadata standard account layout.

use anyhow::{Result, anyhow, bail};
use serde_json::{Map, Value, json};
use solana_pubkey::Pubkey;
use std::str::FromStr;

use super::binary_reader::BinaryReader;

const METAPLEX_METADATA_PROGRAM_ID: &str = "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s";
const METADATA_V1_KEY: u8 = 4;

/// Returns Metaplex Token Metadata program id.
pub(crate) fn metadata_program_id() -> Pubkey {
    Pubkey::from_str(METAPLEX_METADATA_PROGRAM_ID).expect("valid metaplex metadata program id")
}

/// Derive metadata PDA for a legacy mint.
pub(crate) fn derive_metadata_pda(mint: &Pubkey) -> Pubkey {
    let program_id = metadata_program_id();
    let (pda, _) = Pubkey::find_program_address(
        &[b"metadata", program_id.as_ref(), mint.as_ref()],
        &program_id,
    );
    pda
}

/// Decode a metaplex metadata account into JSON.
pub(crate) fn decode_metadata_account_data(data: &[u8]) -> Result<Value> {
    let mut parser = Parser::new(data);

    let key = parser.read_u8()?;
    if key != METADATA_V1_KEY {
        bail!("not a MetadataV1 account, key={key}");
    }

    let update_authority = parser.read_pubkey()?;
    let mint = parser.read_pubkey()?;
    let name = parser.read_string()?;
    let symbol = parser.read_string()?;
    let uri = parser.read_string()?;
    let seller_fee_basis_points = parser.read_u16()?;
    let creators = parser.read_option_creators()?;
    let primary_sale_happened = parser.read_bool()?;
    let is_mutable = parser.read_bool()?;

    let mut result = Map::new();
    result.insert("update_authority".into(), json!(update_authority.to_string()));
    result.insert("mint".into(), json!(mint.to_string()));
    result.insert("name".into(), json!(trim_null_padding(&name)));
    result.insert("symbol".into(), json!(trim_null_padding(&symbol)));
    result.insert("uri".into(), json!(trim_null_padding(&uri)));
    result.insert("seller_fee_basis_points".into(), json!(seller_fee_basis_points));
    result.insert("primary_sale_happened".into(), json!(primary_sale_happened));
    result.insert("is_mutable".into(), json!(is_mutable));
    result.insert("creators".into(), creators_to_json(creators));

    // Optional trailing fields in Metadata account.
    if parser.has_remaining() {
        let edition_nonce = parser.read_option_u8()?;
        if let Some(v) = edition_nonce {
            result.insert("edition_nonce".into(), json!(v));
        }
    }

    if parser.has_remaining() {
        let token_standard = parser.read_option_token_standard()?;
        if let Some(v) = token_standard {
            result.insert("token_standard".into(), json!(v));
        }
    }

    if parser.has_remaining() {
        let collection = parser.read_option_collection()?;
        if let Some(v) = collection {
            result.insert("collection".into(), v);
        }
    }

    if parser.has_remaining() {
        let uses = parser.read_option_uses()?;
        if let Some(v) = uses {
            result.insert("uses".into(), v);
        }
    }

    if parser.has_remaining() {
        let collection_details = parser.read_option_collection_details()?;
        if let Some(v) = collection_details {
            result.insert("collection_details".into(), v);
        }
    }

    if parser.has_remaining() {
        let programmable_config = parser.read_option_programmable_config()?;
        if let Some(v) = programmable_config {
            result.insert("programmable_config".into(), v);
        }
    }

    Ok(Value::Object(result))
}

#[derive(Debug)]
struct Creator {
    address: Pubkey,
    verified: bool,
    share: u8,
}

fn creators_to_json(creators: Option<Vec<Creator>>) -> Value {
    match creators {
        Some(creators) => Value::Array(
            creators
                .into_iter()
                .map(|c| {
                    json!({
                        "address": c.address.to_string(),
                        "verified": c.verified,
                        "share": c.share
                    })
                })
                .collect(),
        ),
        None => Value::Null,
    }
}

fn trim_null_padding(input: &str) -> String {
    input.trim_end_matches('\0').to_string()
}

struct Parser<'a> {
    reader: BinaryReader<'a>,
}

impl<'a> Parser<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { reader: BinaryReader::new(bytes) }
    }

    fn has_remaining(&self) -> bool {
        self.reader.has_remaining()
    }

    fn read_u8(&mut self) -> Result<u8> {
        self.reader.read_u8()
    }

    fn read_bool(&mut self) -> Result<bool> {
        self.reader.read_bool()
    }

    fn read_u16(&mut self) -> Result<u16> {
        self.reader.read_u16()
    }

    fn read_u64(&mut self) -> Result<u64> {
        self.reader.read_u64()
    }

    fn read_pubkey(&mut self) -> Result<Pubkey> {
        self.reader.read_pubkey()
    }

    fn read_string(&mut self) -> Result<String> {
        self.reader.read_string()
    }

    fn read_option_tag(&mut self) -> Result<bool> {
        self.reader.read_option_tag()
    }

    fn read_option_u8(&mut self) -> Result<Option<u8>> {
        if !self.read_option_tag()? {
            return Ok(None);
        }
        Ok(Some(self.read_u8()?))
    }

    fn read_option_creators(&mut self) -> Result<Option<Vec<Creator>>> {
        if !self.read_option_tag()? {
            return Ok(None);
        }

        let len = self.reader.read_u32()? as usize;
        let mut creators = Vec::with_capacity(len);
        for _ in 0..len {
            creators.push(Creator {
                address: self.read_pubkey()?,
                verified: self.read_bool()?,
                share: self.read_u8()?,
            });
        }
        Ok(Some(creators))
    }

    fn read_option_token_standard(&mut self) -> Result<Option<&'static str>> {
        if !self.read_option_tag()? {
            return Ok(None);
        }
        let standard = match self.read_u8()? {
            0 => "NonFungible",
            1 => "FungibleAsset",
            2 => "Fungible",
            3 => "NonFungibleEdition",
            4 => "ProgrammableNonFungible",
            5 => "ProgrammableNonFungibleEdition",
            v => return Err(anyhow!("unsupported token_standard variant {v}")),
        };
        Ok(Some(standard))
    }

    fn read_option_collection(&mut self) -> Result<Option<Value>> {
        if !self.read_option_tag()? {
            return Ok(None);
        }
        let verified = self.read_bool()?;
        let key = self.read_pubkey()?;
        Ok(Some(json!({
            "verified": verified,
            "key": key.to_string()
        })))
    }

    fn read_option_uses(&mut self) -> Result<Option<Value>> {
        if !self.read_option_tag()? {
            return Ok(None);
        }
        let use_method = match self.read_u8()? {
            0 => "Burn",
            1 => "Multiple",
            2 => "Single",
            v => return Err(anyhow!("unsupported use_method variant {v}")),
        };
        let remaining = self.read_u64()?;
        let total = self.read_u64()?;
        Ok(Some(json!({
            "use_method": use_method,
            "remaining": remaining.to_string(),
            "total": total.to_string()
        })))
    }

    fn read_option_collection_details(&mut self) -> Result<Option<Value>> {
        if !self.read_option_tag()? {
            return Ok(None);
        }
        let variant = self.read_u8()?;
        match variant {
            0 => {
                let size = self.read_u64()?;
                Ok(Some(json!({
                    "kind": "V1",
                    "size": size.to_string()
                })))
            }
            _ => Err(anyhow!("unsupported collection_details variant {variant}")),
        }
    }

    fn read_option_programmable_config(&mut self) -> Result<Option<Value>> {
        if !self.read_option_tag()? {
            return Ok(None);
        }
        let variant = self.read_u8()?;
        match variant {
            0 => {
                let rule_set = if self.read_option_tag()? {
                    Some(self.read_pubkey()?.to_string())
                } else {
                    None
                };
                Ok(Some(json!({
                    "kind": "V1",
                    "rule_set": rule_set
                })))
            }
            _ => Err(anyhow!("unsupported programmable_config variant {variant}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_u8(out: &mut Vec<u8>, v: u8) {
        out.push(v);
    }

    fn push_bool(out: &mut Vec<u8>, v: bool) {
        out.push(u8::from(v));
    }

    fn push_u16(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn push_u32(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn push_u64(out: &mut Vec<u8>, v: u64) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn push_pubkey(out: &mut Vec<u8>, pk: &Pubkey) {
        out.extend_from_slice(pk.as_ref());
    }

    fn push_string(out: &mut Vec<u8>, s: &str) {
        push_u32(out, s.len() as u32);
        out.extend_from_slice(s.as_bytes());
    }

    #[test]
    fn decode_minimal_metadata_account() {
        let update_authority = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        let mut data = Vec::new();
        push_u8(&mut data, METADATA_V1_KEY);
        push_pubkey(&mut data, &update_authority);
        push_pubkey(&mut data, &mint);
        push_string(&mut data, "Test Token\0\0");
        push_string(&mut data, "TST\0");
        push_string(&mut data, "https://example.com/meta.json\0");
        push_u16(&mut data, 500);
        push_u8(&mut data, 0); // creators None
        push_bool(&mut data, false);
        push_bool(&mut data, true);

        let parsed = decode_metadata_account_data(&data).unwrap();
        assert_eq!(parsed["update_authority"], update_authority.to_string());
        assert_eq!(parsed["mint"], mint.to_string());
        assert_eq!(parsed["name"], "Test Token");
        assert_eq!(parsed["symbol"], "TST");
        assert_eq!(parsed["uri"], "https://example.com/meta.json");
        assert_eq!(parsed["seller_fee_basis_points"], 500);
        assert_eq!(parsed["creators"], Value::Null);
        assert_eq!(parsed["primary_sale_happened"], false);
        assert_eq!(parsed["is_mutable"], true);
    }

    #[test]
    fn decode_metadata_with_optional_fields() {
        let update_authority = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let collection_key = Pubkey::new_unique();
        let rule_set = Pubkey::new_unique();

        let mut data = Vec::new();
        push_u8(&mut data, METADATA_V1_KEY);
        push_pubkey(&mut data, &update_authority);
        push_pubkey(&mut data, &mint);
        push_string(&mut data, "Token");
        push_string(&mut data, "TK");
        push_string(&mut data, "https://example.com");
        push_u16(&mut data, 250);

        // creators Some(vec![...])
        push_u8(&mut data, 1);
        push_u32(&mut data, 1);
        push_pubkey(&mut data, &creator);
        push_bool(&mut data, true);
        push_u8(&mut data, 100);

        push_bool(&mut data, true); // primary_sale_happened
        push_bool(&mut data, false); // is_mutable

        // edition_nonce Some(9)
        push_u8(&mut data, 1);
        push_u8(&mut data, 9);

        // token_standard Some(ProgrammableNonFungible)
        push_u8(&mut data, 1);
        push_u8(&mut data, 4);

        // collection Some
        push_u8(&mut data, 1);
        push_bool(&mut data, true);
        push_pubkey(&mut data, &collection_key);

        // uses Some(Single, 3, 10)
        push_u8(&mut data, 1);
        push_u8(&mut data, 2);
        push_u64(&mut data, 3);
        push_u64(&mut data, 10);

        // collection_details Some(V1 { size: 777 })
        push_u8(&mut data, 1);
        push_u8(&mut data, 0);
        push_u64(&mut data, 777);

        // programmable_config Some(V1 { rule_set: Some(rule_set) })
        push_u8(&mut data, 1);
        push_u8(&mut data, 0);
        push_u8(&mut data, 1);
        push_pubkey(&mut data, &rule_set);

        let parsed = decode_metadata_account_data(&data).unwrap();
        assert_eq!(parsed["edition_nonce"], 9);
        assert_eq!(parsed["token_standard"], "ProgrammableNonFungible");
        assert_eq!(parsed["collection"]["verified"], true);
        assert_eq!(parsed["collection"]["key"], collection_key.to_string());
        assert_eq!(parsed["uses"]["use_method"], "Single");
        assert_eq!(parsed["uses"]["remaining"], "3");
        assert_eq!(parsed["uses"]["total"], "10");
        assert_eq!(parsed["collection_details"]["kind"], "V1");
        assert_eq!(parsed["collection_details"]["size"], "777");
        assert_eq!(parsed["programmable_config"]["kind"], "V1");
        assert_eq!(parsed["programmable_config"]["rule_set"], rule_set.to_string());
    }
}
