use anyhow::{Result, anyhow};
use serde::Serialize;
use serde_json::Value;

use crate::models::*;

mod decode;
mod indexed;

#[cfg(test)]
mod tests;

#[cfg(test)]
use self::decode::{parse_array_type, parse_option_type, parse_simple_type, parse_vec_type};
use self::decode::{
    parse_idl_fields_as_parsed_fields, parse_instruction_args, parse_type_definition,
    raw_unparsed_value,
};
pub use self::indexed::IndexedIdl;
use self::indexed::{IdlLookup, scan_event_by_discriminator, scan_instruction_by_discriminator};

/// A fully parsed IDL instruction with resolved argument fields.
#[derive(Debug, Clone, Serialize)]
pub struct IdlParsedInstruction {
    pub name: String,
    pub fields: Vec<IdlParsedField>,
    /// The account items from the matched IDL instruction definition.
    pub accounts: Vec<IdlAccountItem>,
}

/// A single parsed field from IDL binary data.
#[derive(Debug, Clone, Serialize)]
pub struct IdlParsedField {
    pub name: String,
    pub value: Value,
}

const EMIT_CPI_DISCRIMINATOR: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];

/// Parse an instruction's binary data using an IDL.
///
/// Matches the data's discriminator against the IDL's instructions, then
/// deserializes the remaining bytes according to the matched instruction's args.
///
/// Returns `Ok(None)` if no instruction discriminator matches.
pub fn parse_instruction(idl: &Idl, data: &[u8]) -> Result<Option<IdlParsedInstruction>> {
    parse_instruction_with_lookup(idl, data)
}

fn parse_instruction_with_lookup<L: IdlLookup>(
    lookup: &L,
    data: &[u8],
) -> Result<Option<IdlParsedInstruction>> {
    let Some(idl_instruction) = lookup.find_instruction_by_discriminator(data) else {
        return Ok(None);
    };

    let mut offset = idl_instruction.discriminator.as_ref().map_or(0, |d| d.len());
    let fields = parse_instruction_args(data, &mut offset, &idl_instruction.args, lookup)?;

    Ok(Some(IdlParsedInstruction {
        name: idl_instruction.name.clone(),
        fields,
        accounts: idl_instruction.accounts.clone(),
    }))
}

/// Parse Anchor account data using the IDL.
///
/// Returns `Ok(Some((type_name, parsed_data)))` if the account type is found and parsed,
/// `Ok(None)` if no matching account type is found in the IDL,
/// or an error if parsing fails.
pub fn parse_account_data(idl: &Idl, account_data: &[u8]) -> Result<Option<(String, Value)>> {
    parse_account_data_with_lookup(idl, account_data)
}

fn parse_account_data_with_lookup<L: IdlLookup>(
    lookup: &L,
    account_data: &[u8],
) -> Result<Option<(String, Value)>> {
    if account_data.len() < 8 {
        return Err(anyhow!(
            "Account data too short: {} bytes (expected at least 8 for discriminator)",
            account_data.len()
        ));
    }

    let discriminator = &account_data[..8];

    let Some(type_def) = lookup.find_account_type_by_discriminator(discriminator) else {
        return Ok(None);
    };

    let mut offset = 8;
    let parsed_value = parse_type_definition(account_data, &mut offset, type_def, lookup)?;

    Ok(Some((type_def.name.clone(), parsed_value)))
}

/// Check if raw instruction data represents an Anchor CPI event.
pub fn is_cpi_event_data(data: &[u8]) -> bool {
    data.len() >= 16 && data[..8] == EMIT_CPI_DISCRIMINATOR
}

/// Parse an Anchor CPI event from raw instruction data.
///
/// The caller provides the instruction `data` and the emitting program's IDL.
pub fn parse_cpi_event_data(idl: &Idl, data: &[u8]) -> Result<Option<IdlParsedInstruction>> {
    parse_cpi_event_data_with_lookup(idl, data)
}

fn parse_cpi_event_data_with_lookup<L: IdlLookup>(
    lookup: &L,
    data: &[u8],
) -> Result<Option<IdlParsedInstruction>> {
    if data.len() < 16 {
        return Ok(None);
    }

    if data[..8] != EMIT_CPI_DISCRIMINATOR {
        return Ok(None);
    }

    let event_discriminator = &data[8..16];

    let Some(event_def) = lookup.find_event_by_discriminator(event_discriminator) else {
        return Ok(None);
    };

    let type_fields = if let Some(fields) = &event_def.fields {
        Some(fields.clone())
    } else {
        lookup
            .find_type_definition(&event_def.name)
            .and_then(|type_def| type_def.type_.fields.clone())
    };

    let mut offset = 16;
    let fields = match type_fields.as_ref() {
        Some(fields) => parse_idl_fields_as_parsed_fields(data, &mut offset, fields, lookup)?,
        None => {
            let mut raw_fields = Vec::new();
            if offset < data.len() {
                let raw_data = &data[offset..];
                raw_fields.push(IdlParsedField {
                    name: "raw_data".into(),
                    value: raw_unparsed_value("event_data", &event_def.name, raw_data),
                });
            }
            raw_fields
        }
    };

    Ok(Some(IdlParsedInstruction { name: event_def.name.clone(), fields, accounts: Vec::new() }))
}

/// Find the instruction whose discriminator matches the given data.
///
/// When multiple instructions match (e.g. one with an explicit empty discriminator),
/// the most specific (longest) match wins.
pub fn find_instruction_by_discriminator<'a>(
    idl: &'a Idl,
    data: &[u8],
) -> Option<&'a IdlInstruction> {
    scan_instruction_by_discriminator(idl, data)
}

/// Find an event by discriminator.
pub fn find_event_by_discriminator<'a>(idl: &'a Idl, discriminator: &[u8]) -> Option<&'a IdlEvent> {
    scan_event_by_discriminator(idl, discriminator)
}
