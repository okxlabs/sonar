use anyhow::{Result, anyhow};
use serde::Serialize;
use serde_json::{Number as JsonNumber, Value as JsonValue};
use solana_pubkey::Pubkey;

use crate::discriminator::sighash;
use crate::models::*;
use crate::registry::IdlRegistry;
use crate::value::OrderedJsonValue;

// ── Return types ──

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
    pub value: OrderedJsonValue,
}

// ── High-level parse entry points ──

/// Parse an instruction's binary data using an IDL.
///
/// Matches the data's discriminator against the IDL's instructions, then
/// deserializes the remaining bytes according to the matched instruction's args.
///
/// Returns `Ok(None)` if no instruction discriminator matches.
pub fn parse_instruction(
    idl: &Idl,
    data: &[u8],
    registry: &IdlRegistry,
) -> Result<Option<IdlParsedInstruction>> {
    let Some(idl_instruction) = find_instruction_by_discriminator(idl, data) else {
        return Ok(None);
    };

    let mut offset = idl_instruction.discriminator.as_ref().map_or(0, |d| d.len());
    let fields = parse_instruction_args(data, &mut offset, &idl_instruction.args, registry, idl)?;

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
pub fn parse_account_data(
    idl: &Idl,
    account_data: &[u8],
    registry: &IdlRegistry,
) -> Result<Option<(String, OrderedJsonValue)>> {
    if account_data.len() < 8 {
        return Err(anyhow!(
            "Account data too short: {} bytes (expected at least 8 for discriminator)",
            account_data.len()
        ));
    }

    let discriminator = &account_data[..8];

    let Some(type_def) = find_account_type_by_discriminator(idl, discriminator) else {
        return Ok(None);
    };

    let mut offset = 8;
    let parsed_value = parse_type_definition(account_data, &mut offset, type_def, registry, idl)?;

    Ok(Some((type_def.name.clone(), parsed_value)))
}

const EMIT_CPI_DISCRIMINATOR: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];

/// Check if raw instruction data represents an Anchor CPI event.
pub fn is_cpi_event_data(data: &[u8]) -> bool {
    data.len() >= 16 && data[..8] == EMIT_CPI_DISCRIMINATOR
}

/// Parse an Anchor CPI event from raw instruction data.
///
/// The caller provides the instruction `data`, the IDL `registry`, and the
/// `program_id` that emitted the event.
pub fn parse_cpi_event_data(
    data: &[u8],
    idl_registry: &IdlRegistry,
    program_id: &Pubkey,
) -> Result<Option<IdlParsedInstruction>> {
    if data.len() < 16 {
        return Ok(None);
    }

    if data[..8] != EMIT_CPI_DISCRIMINATOR {
        return Ok(None);
    }

    let event_discriminator = &data[8..16];

    let Some(idl) = idl_registry.get(program_id) else {
        return Ok(None);
    };

    let Some(event_def) = find_event_by_discriminator(idl, event_discriminator) else {
        return Ok(None);
    };

    let type_fields = if let Some(fields) = &event_def.fields {
        Some(IdlFields::Named(fields.clone()))
    } else if let Some(types) = &idl.types {
        types.iter().find(|t| t.name == event_def.name).and_then(|t| t.type_.fields.clone())
    } else {
        idl_registry
            .get_type_by_program(program_id, &event_def.name)
            .and_then(|t| t.type_.fields.clone())
    };

    let mut offset = 16;
    let mut fields = Vec::new();

    match type_fields {
        Some(IdlFields::Named(named_fields)) => {
            for field in named_fields {
                if offset >= data.len() {
                    break;
                }
                let value = parse_type(data, &mut offset, &field.type_, idl_registry, idl)?;
                fields.push(IdlParsedField { name: field.name.clone(), value });
            }
        }
        Some(IdlFields::Tuple(type_names)) => {
            for (idx, type_name) in type_names.iter().enumerate() {
                if offset >= data.len() {
                    break;
                }
                let value = parse_simple_type(data, &mut offset, type_name)?;
                fields.push(IdlParsedField { name: format!("field_{}", idx), value });
            }
        }
        None => {
            if offset < data.len() {
                let raw_data = &data[offset..];
                fields.push(IdlParsedField {
                    name: "raw_data".into(),
                    value: raw_unparsed_value("event_data", &event_def.name, raw_data),
                });
            }
        }
    }

    Ok(Some(IdlParsedInstruction { name: event_def.name.clone(), fields, accounts: Vec::new() }))
}

// ── Discriminator matching ──

/// Find the instruction whose discriminator matches the given data.
///
/// When multiple instructions match (e.g. one with an empty discriminator),
/// the most specific (longest) match wins.
pub fn find_instruction_by_discriminator<'a>(
    idl: &'a Idl,
    data: &[u8],
) -> Option<&'a IdlInstruction> {
    idl.instructions
        .iter()
        .filter(|inst| {
            let disc = inst.discriminator.as_deref().unwrap_or(&[]);
            let disc_len = disc.len();
            data.len() >= disc_len && &data[..disc_len] == disc
        })
        .max_by_key(|inst| inst.discriminator.as_ref().map_or(0, |d| d.len()))
}

/// Find an event by discriminator.
pub fn find_event_by_discriminator<'a>(idl: &'a Idl, discriminator: &[u8]) -> Option<&'a IdlEvent> {
    if let Some(events) = &idl.events {
        events.iter().find(|event| {
            event
                .discriminator
                .as_ref()
                .is_some_and(|d| d.len() == 8 && d.as_slice() == discriminator)
        })
    } else {
        None
    }
}

// ── Internal: account type matching ──

fn find_account_type_by_discriminator<'a>(
    idl: &'a Idl,
    discriminator: &[u8],
) -> Option<&'a IdlTypeDefinition> {
    let types = idl.types.as_ref()?;

    types.iter().find(|type_def| {
        if type_def.type_.kind != "struct" {
            return false;
        }
        let expected_discriminator = sighash("account", &type_def.name);
        discriminator == expected_discriminator
    })
}

// ── Internal: binary deserialization ──

fn parse_instruction_args(
    data: &[u8],
    offset: &mut usize,
    args: &[IdlArg],
    registry: &IdlRegistry,
    idl: &Idl,
) -> Result<Vec<IdlParsedField>> {
    let mut fields = Vec::new();

    for arg in args {
        if *offset >= data.len() {
            break;
        }

        let value = parse_type(data, offset, &arg.type_, registry, idl)?;
        fields.push(IdlParsedField { name: arg.name.clone(), value });
    }

    Ok(fields)
}

fn raw_unparsed_value(context: &str, type_name: &str, raw_data: &[u8]) -> OrderedJsonValue {
    OrderedJsonValue::Object(vec![
        ("context".into(), OrderedJsonValue::String(context.to_string())),
        ("type_hint".into(), OrderedJsonValue::String(type_name.to_string())),
        ("raw_hex".into(), OrderedJsonValue::String(hex::encode(raw_data))),
    ])
}

fn parse_type(
    data: &[u8],
    offset: &mut usize,
    idl_type: &IdlType,
    registry: &IdlRegistry,
    idl: &Idl,
) -> Result<OrderedJsonValue> {
    match idl_type {
        IdlType::Simple(type_name) => parse_simple_type(data, offset, type_name),
        IdlType::Vec { vec } => parse_vec_type(data, offset, vec, registry, idl),
        IdlType::Option { option } => parse_option_type(data, offset, option, registry, idl),
        IdlType::Array { array } => parse_array_type(data, offset, array, registry, idl),
        IdlType::Defined { defined } => parse_defined_type(data, offset, defined, registry, idl),
    }
}

fn parse_simple_type(data: &[u8], offset: &mut usize, type_name: &str) -> Result<OrderedJsonValue> {
    let start = *offset;

    let (value, bytes_read) = match type_name {
        "u8" => {
            check_data_len(data, start, 1)?;
            (OrderedJsonValue::Number(JsonNumber::from(u64::from(data[start]))), 1)
        }
        "i8" => {
            check_data_len(data, start, 1)?;
            (OrderedJsonValue::Number(JsonNumber::from(data[start] as i8 as i64)), 1)
        }
        "u16" => {
            check_data_len(data, start, 2)?;
            let value = u16::from_le_bytes([data[start], data[start + 1]]);
            (OrderedJsonValue::Number(JsonNumber::from(u64::from(value))), 2)
        }
        "i16" => {
            check_data_len(data, start, 2)?;
            let value = i16::from_le_bytes([data[start], data[start + 1]]);
            (OrderedJsonValue::Number(JsonNumber::from(value as i64)), 2)
        }
        "u32" => {
            check_data_len(data, start, 4)?;
            let value = u32::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
            ]);
            (OrderedJsonValue::Number(JsonNumber::from(u64::from(value))), 4)
        }
        "i32" => {
            check_data_len(data, start, 4)?;
            let value = i32::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
            ]);
            (OrderedJsonValue::Number(JsonNumber::from(value as i64)), 4)
        }
        "u64" => {
            check_data_len(data, start, 8)?;
            let value = u64::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
                data[start + 4],
                data[start + 5],
                data[start + 6],
                data[start + 7],
            ]);
            (OrderedJsonValue::Number(JsonNumber::from(value)), 8)
        }
        "i64" => {
            check_data_len(data, start, 8)?;
            let value = i64::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
                data[start + 4],
                data[start + 5],
                data[start + 6],
                data[start + 7],
            ]);
            (OrderedJsonValue::Number(JsonNumber::from(value)), 8)
        }
        "u128" => {
            check_data_len(data, start, 16)?;
            let value = u128::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
                data[start + 4],
                data[start + 5],
                data[start + 6],
                data[start + 7],
                data[start + 8],
                data[start + 9],
                data[start + 10],
                data[start + 11],
                data[start + 12],
                data[start + 13],
                data[start + 14],
                data[start + 15],
            ]);
            (OrderedJsonValue::String(value.to_string()), 16)
        }
        "i128" => {
            check_data_len(data, start, 16)?;
            let value = i128::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
                data[start + 4],
                data[start + 5],
                data[start + 6],
                data[start + 7],
                data[start + 8],
                data[start + 9],
                data[start + 10],
                data[start + 11],
                data[start + 12],
                data[start + 13],
                data[start + 14],
                data[start + 15],
            ]);
            (OrderedJsonValue::String(value.to_string()), 16)
        }
        "pubkey" | "publicKey" => {
            check_data_len(data, start, 32)?;
            let pubkey = Pubkey::try_from(&data[start..start + 32])
                .map_err(|_| anyhow!("Invalid pubkey data"))?;
            (OrderedJsonValue::String(pubkey.to_string()), 32)
        }
        "bool" => {
            check_data_len(data, start, 1)?;
            let value = data[start] != 0;
            (OrderedJsonValue::Bool(value), 1)
        }
        "string" => {
            check_data_len(data, start, 4)?;
            let length = u32::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
            ]) as usize;
            let content_start = start + 4;
            check_data_len(data, content_start, length)?;
            let string_data = &data[content_start..content_start + length];
            let value = String::from_utf8_lossy(string_data).to_string();
            (OrderedJsonValue::String(value), 4 + length)
        }
        "bytes" => {
            check_data_len(data, start, 4)?;
            let length = u32::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
            ]) as usize;
            let content_start = start + 4;
            check_data_len(data, content_start, length)?;
            let mut array = Vec::with_capacity(length);
            for byte in &data[content_start..content_start + length] {
                array.push(OrderedJsonValue::Number(JsonNumber::from(u64::from(*byte))));
            }
            (OrderedJsonValue::Array(array), 4 + length)
        }
        _ => {
            let remaining = &data[start..];
            let bytes_read = remaining.len().min(32);
            (raw_unparsed_value("simple_type", type_name, &remaining[..bytes_read]), bytes_read)
        }
    };

    *offset += bytes_read;
    Ok(value)
}

fn parse_vec_type(
    data: &[u8],
    offset: &mut usize,
    element_type: &IdlType,
    registry: &IdlRegistry,
    idl: &Idl,
) -> Result<OrderedJsonValue> {
    let start = *offset;
    check_data_len(data, start, 4)?;

    let length =
        u32::from_le_bytes([data[start], data[start + 1], data[start + 2], data[start + 3]])
            as usize;
    *offset += 4;

    let mut elements = Vec::with_capacity(length);
    for _ in 0..length {
        if *offset >= data.len() {
            break;
        }
        let element = parse_type(data, offset, element_type, registry, idl)?;
        elements.push(element);
    }

    Ok(OrderedJsonValue::Array(elements))
}

fn parse_option_type(
    data: &[u8],
    offset: &mut usize,
    inner_type: &IdlType,
    registry: &IdlRegistry,
    idl: &Idl,
) -> Result<OrderedJsonValue> {
    let start = *offset;
    check_data_len(data, start, 1)?;

    let is_some = data[start] != 0;
    *offset += 1;

    if !is_some {
        Ok(OrderedJsonValue::Null)
    } else {
        parse_type(data, offset, inner_type, registry, idl)
    }
}

fn parse_array_type(
    data: &[u8],
    offset: &mut usize,
    array_def: &[JsonValue; 2],
    registry: &IdlRegistry,
    idl: &Idl,
) -> Result<OrderedJsonValue> {
    let element_type = array_def[0].clone();
    let length = array_def[1].as_u64().ok_or_else(|| anyhow!("Invalid array length"))? as usize;

    let idl_type = serde_json::from_value(element_type)
        .map_err(|_| anyhow!("Failed to parse array element type"))?;

    let mut elements = Vec::with_capacity(length);
    for _ in 0..length {
        if *offset >= data.len() {
            break;
        }
        let element = parse_type(data, offset, &idl_type, registry, idl)?;
        elements.push(element);
    }

    Ok(OrderedJsonValue::Array(elements))
}

fn parse_defined_type(
    data: &[u8],
    offset: &mut usize,
    defined: &DefinedType,
    registry: &IdlRegistry,
    idl: &Idl,
) -> Result<OrderedJsonValue> {
    let program_id = Pubkey::try_from(idl.address.as_str())
        .map_err(|_| anyhow!("Invalid program ID in IDL: {}", idl.address))?;

    if let Some(types) = &idl.types {
        if let Some(type_def) = types.iter().find(|t| t.name == defined.name()) {
            return parse_type_definition(data, offset, type_def, registry, idl);
        }
    }

    if let Some(type_def) = registry.get_type_by_program(&program_id, defined.name()) {
        return parse_type_definition(data, offset, type_def, registry, idl);
    }

    let start = *offset;
    let remaining = &data[start..];
    let bytes_read = remaining.len().min(32);
    *offset += bytes_read;
    Ok(raw_unparsed_value("defined_type", defined.name(), &remaining[..bytes_read]))
}

fn parse_type_definition(
    data: &[u8],
    offset: &mut usize,
    type_def: &IdlTypeDefinition,
    registry: &IdlRegistry,
    idl: &Idl,
) -> Result<OrderedJsonValue> {
    match type_def.type_.kind.as_str() {
        "struct" => {
            if let Some(fields) = &type_def.type_.fields {
                return match fields {
                    IdlFields::Named(named_fields) => {
                        let mut entries = Vec::new();
                        for field in named_fields {
                            if *offset >= data.len() {
                                break;
                            }
                            let value = parse_type(data, offset, &field.type_, registry, idl)?;
                            entries.push((field.name.clone(), value));
                        }
                        Ok(OrderedJsonValue::Object(entries))
                    }
                    IdlFields::Tuple(type_names) => {
                        let mut values = Vec::new();
                        for type_name in type_names {
                            if *offset >= data.len() {
                                break;
                            }
                            let value = parse_simple_type(data, offset, type_name)?;
                            values.push(value);
                        }
                        Ok(OrderedJsonValue::Array(values))
                    }
                };
            }
        }
        "enum" => {
            if let Some(variants) = &type_def.type_.variants {
                check_data_len(data, *offset, 1)?;
                let variant_index = data[*offset] as usize;
                *offset += 1;

                if variant_index < variants.len() {
                    let variant = &variants[variant_index];
                    let payload = if let Some(fields) = &variant.fields {
                        if let Some(fields_array) = fields.as_array() {
                            if fields_array.is_empty() {
                                OrderedJsonValue::Null
                            } else if fields_array[0].get("name").is_some() {
                                let mut entries = Vec::new();
                                for field in fields_array {
                                    if let (Some(name), Some(type_value)) =
                                        (field.get("name"), field.get("type"))
                                    {
                                        let type_ = serde_json::from_value(type_value.clone())
                                            .map_err(|_| {
                                                anyhow!("Failed to parse enum field type")
                                            })?;
                                        let value =
                                            parse_type(data, offset, &type_, registry, idl)?;
                                        entries
                                            .push((name.as_str().unwrap_or("").to_string(), value));
                                    }
                                }
                                OrderedJsonValue::Object(entries)
                            } else {
                                let mut values = Vec::new();
                                for type_value in fields_array {
                                    let type_ = serde_json::from_value(type_value.clone())
                                        .map_err(|_| anyhow!("Failed to parse tuple field type"))?;
                                    let value = parse_type(data, offset, &type_, registry, idl)?;
                                    values.push(value);
                                }
                                OrderedJsonValue::Array(values)
                            }
                        } else {
                            OrderedJsonValue::Null
                        }
                    } else {
                        OrderedJsonValue::Null
                    };

                    return Ok(OrderedJsonValue::Object(vec![(variant.name.clone(), payload)]));
                }
            }
        }
        _ => {}
    }

    let start = *offset;
    let remaining = &data[start..];
    let bytes_read = remaining.len().min(32);
    *offset += bytes_read;
    Ok(raw_unparsed_value("type_definition", &type_def.name, &remaining[..bytes_read]))
}

fn check_data_len(data: &[u8], offset: usize, required: usize) -> Result<()> {
    if offset + required > data.len() {
        Err(anyhow!(
            "Insufficient data: need {} bytes at offset {}, have {} bytes",
            required,
            offset,
            data.len()
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discriminator::sighash;
    use std::str::FromStr;

    fn hello_anchor_idl() -> Idl {
        serde_json::from_str(
            r#"{
                "address": "BYFW1vhC1ohxwRbYoLbAWs86STa25i9sD5uEusVjTYNd",
                "metadata": { "name": "hello_anchor", "version": "0.1.0", "spec": "0.1.0" },
                "instructions": [{
                    "name": "initialize",
                    "discriminator": [175, 175, 109, 31, 13, 152, 155, 237],
                    "accounts": [
                        { "name": "new_account", "writable": true, "signer": true },
                        { "name": "signer", "writable": true, "signer": true },
                        { "name": "system_program", "address": "11111111111111111111111111111111" }
                    ],
                    "args": [{ "name": "data", "type": "u64" }]
                }],
                "types": [{
                    "name": "NewAccount",
                    "type": { "kind": "struct", "fields": [{ "name": "data", "type": "u64" }] }
                }]
            }"#,
        )
        .unwrap()
    }

    // ── parse_instruction ──

    #[test]
    fn parse_instruction_matches_discriminator_and_reads_u64_arg() {
        let idl = hello_anchor_idl();
        let registry = IdlRegistry::new();

        let mut data = vec![175, 175, 109, 31, 13, 152, 155, 237];
        data.extend_from_slice(&42u64.to_le_bytes());

        let result = parse_instruction(&idl, &data, &registry).unwrap();
        let parsed = result.expect("should match");

        assert_eq!(parsed.name, "initialize");
        assert_eq!(parsed.fields.len(), 1);
        assert_eq!(parsed.fields[0].name, "data");
        assert_eq!(parsed.fields[0].value, OrderedJsonValue::Number(42u64.into()));
    }

    #[test]
    fn parse_instruction_returns_none_for_unknown_discriminator() {
        let idl = hello_anchor_idl();
        let registry = IdlRegistry::new();
        let data = vec![0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];

        let result = parse_instruction(&idl, &data, &registry).unwrap();
        assert!(result.is_none());
    }

    // ── parse_account_data ──

    #[test]
    fn parse_account_data_matches_struct_by_discriminator() {
        let idl = hello_anchor_idl();
        let registry = IdlRegistry::new();

        let disc = sighash("account", "NewAccount");
        let mut data = disc.to_vec();
        data.extend_from_slice(&99u64.to_le_bytes());

        let result = parse_account_data(&idl, &data, &registry).unwrap();
        let (type_name, value) = result.expect("should match NewAccount");

        assert_eq!(type_name, "NewAccount");
        assert_eq!(
            value,
            OrderedJsonValue::Object(vec![
                ("data".into(), OrderedJsonValue::Number(99u64.into()),)
            ])
        );
    }

    #[test]
    fn parse_account_data_returns_none_for_unknown_discriminator() {
        let idl = hello_anchor_idl();
        let registry = IdlRegistry::new();
        let data = [0u8; 16];

        let result = parse_account_data(&idl, &data, &registry).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn parse_account_data_rejects_short_data() {
        let idl = hello_anchor_idl();
        let registry = IdlRegistry::new();

        let result = parse_account_data(&idl, &[0u8; 4], &registry);
        assert!(result.is_err());
    }

    // ── CPI event helpers ──

    #[test]
    fn is_cpi_event_data_detects_emit_cpi() {
        let mut data = vec![0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
        data.extend_from_slice(&[0; 8]); // event discriminator placeholder
        assert!(is_cpi_event_data(&data));
    }

    #[test]
    fn is_cpi_event_data_rejects_short() {
        assert!(!is_cpi_event_data(&[0xe4, 0x45, 0xa5]));
    }

    #[test]
    fn is_cpi_event_data_rejects_wrong_prefix() {
        assert!(!is_cpi_event_data(&[0; 16]));
    }

    #[test]
    fn parse_cpi_event_data_returns_none_when_no_idl_registered() {
        let registry = IdlRegistry::new();
        let program_id = Pubkey::new_unique();

        let mut data = vec![0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
        data.extend_from_slice(&[0; 8]);

        let result = parse_cpi_event_data(&data, &registry, &program_id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn parse_cpi_event_data_parses_event_fields() {
        let program_id = Pubkey::from_str("BYFW1vhC1ohxwRbYoLbAWs86STa25i9sD5uEusVjTYNd").unwrap();
        let event_disc = sighash("event", "TransferDone");

        let idl: Idl = serde_json::from_str(&format!(
            r#"{{
                "address": "BYFW1vhC1ohxwRbYoLbAWs86STa25i9sD5uEusVjTYNd",
                "metadata": {{ "name": "ev", "version": "0.1.0", "spec": "0.1.0" }},
                "instructions": [],
                "events": [{{
                    "name": "TransferDone",
                    "discriminator": {:?},
                    "fields": [{{ "name": "amount", "type": "u64" }}]
                }}]
            }}"#,
            event_disc.to_vec()
        ))
        .unwrap();

        let registry = IdlRegistry::with_idl(program_id, &idl);

        let mut data = vec![0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
        data.extend_from_slice(&event_disc);
        data.extend_from_slice(&500u64.to_le_bytes());

        let result = parse_cpi_event_data(&data, &registry, &program_id).unwrap();
        let parsed = result.expect("should parse event");

        assert_eq!(parsed.name, "TransferDone");
        assert_eq!(parsed.fields.len(), 1);
        assert_eq!(parsed.fields[0].name, "amount");
        assert_eq!(parsed.fields[0].value, OrderedJsonValue::Number(500u64.into()));
    }

    // ── Primitive type parsing ──

    #[test]
    fn parse_instruction_multiple_primitive_args() {
        let idl: Idl = serde_json::from_str(
            r#"{
                "address": "11111111111111111111111111111111",
                "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
                "instructions": [{
                    "name": "multi",
                    "discriminator": [1,2,3,4,5,6,7,8],
                    "accounts": [],
                    "args": [
                        { "name": "a", "type": "u8" },
                        { "name": "b", "type": "bool" },
                        { "name": "c", "type": "i16" },
                        { "name": "d", "type": "string" }
                    ]
                }],
                "types": []
            }"#,
        )
        .unwrap();

        let registry = IdlRegistry::new();
        let mut data = vec![1, 2, 3, 4, 5, 6, 7, 8]; // discriminator
        data.push(42); // u8
        data.push(1); // bool true
        data.extend_from_slice(&(-5i16).to_le_bytes()); // i16
        let s = b"hello";
        data.extend_from_slice(&(s.len() as u32).to_le_bytes());
        data.extend_from_slice(s);

        let parsed = parse_instruction(&idl, &data, &registry).unwrap().unwrap();
        assert_eq!(parsed.fields[0].value, OrderedJsonValue::Number(42u64.into()));
        assert_eq!(parsed.fields[1].value, OrderedJsonValue::Bool(true));
        assert_eq!(parsed.fields[2].value, OrderedJsonValue::Number((-5i64).into()));
        assert_eq!(parsed.fields[3].value, OrderedJsonValue::String("hello".into()));
    }

    // ── Defined / struct / enum types ──

    #[test]
    fn parse_instruction_with_defined_struct_arg() {
        let idl: Idl = serde_json::from_str(
            r#"{
                "address": "11111111111111111111111111111111",
                "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
                "instructions": [{
                    "name": "create",
                    "discriminator": [10,20,30,40,50,60,70,80],
                    "accounts": [],
                    "args": [{ "name": "params", "type": { "defined": "Params" } }]
                }],
                "types": [{
                    "name": "Params",
                    "type": {
                        "kind": "struct",
                        "fields": [
                            { "name": "x", "type": "u32" },
                            { "name": "y", "type": "u32" }
                        ]
                    }
                }]
            }"#,
        )
        .unwrap();

        let registry = IdlRegistry::new();
        let mut data = vec![10, 20, 30, 40, 50, 60, 70, 80];
        data.extend_from_slice(&100u32.to_le_bytes());
        data.extend_from_slice(&200u32.to_le_bytes());

        let parsed = parse_instruction(&idl, &data, &registry).unwrap().unwrap();
        assert_eq!(parsed.fields[0].name, "params");
        assert_eq!(
            parsed.fields[0].value,
            OrderedJsonValue::Object(vec![
                ("x".into(), OrderedJsonValue::Number(100u64.into())),
                ("y".into(), OrderedJsonValue::Number(200u64.into())),
            ])
        );
    }

    #[test]
    fn parse_instruction_with_enum_arg() {
        let idl: Idl = serde_json::from_str(
            r#"{
                "address": "11111111111111111111111111111111",
                "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
                "instructions": [{
                    "name": "act",
                    "discriminator": [1,1,1,1,1,1,1,1],
                    "accounts": [],
                    "args": [{ "name": "action", "type": { "defined": "Action" } }]
                }],
                "types": [{
                    "name": "Action",
                    "type": {
                        "kind": "enum",
                        "variants": [
                            { "name": "Start" },
                            { "name": "Stop" }
                        ]
                    }
                }]
            }"#,
        )
        .unwrap();

        let registry = IdlRegistry::new();

        // variant index 0 = Start
        let mut data = vec![1, 1, 1, 1, 1, 1, 1, 1, 0];
        let parsed = parse_instruction(&idl, &data, &registry).unwrap().unwrap();
        assert_eq!(
            parsed.fields[0].value,
            OrderedJsonValue::Object(vec![("Start".into(), OrderedJsonValue::Null)])
        );

        // variant index 1 = Stop
        data[8] = 1;
        let parsed = parse_instruction(&idl, &data, &registry).unwrap().unwrap();
        assert_eq!(
            parsed.fields[0].value,
            OrderedJsonValue::Object(vec![("Stop".into(), OrderedJsonValue::Null)])
        );
    }

    // ── Vec / Option types ──

    #[test]
    fn parse_instruction_with_vec_arg() {
        let idl: Idl = serde_json::from_str(
            r#"{
                "address": "11111111111111111111111111111111",
                "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
                "instructions": [{
                    "name": "bulk",
                    "discriminator": [2,2,2,2,2,2,2,2],
                    "accounts": [],
                    "args": [{ "name": "vals", "type": { "vec": "u16" } }]
                }],
                "types": []
            }"#,
        )
        .unwrap();

        let registry = IdlRegistry::new();
        let mut data = vec![2, 2, 2, 2, 2, 2, 2, 2]; // disc
        data.extend_from_slice(&3u32.to_le_bytes()); // vec length = 3
        data.extend_from_slice(&10u16.to_le_bytes());
        data.extend_from_slice(&20u16.to_le_bytes());
        data.extend_from_slice(&30u16.to_le_bytes());

        let parsed = parse_instruction(&idl, &data, &registry).unwrap().unwrap();
        assert_eq!(
            parsed.fields[0].value,
            OrderedJsonValue::Array(vec![
                OrderedJsonValue::Number(10u64.into()),
                OrderedJsonValue::Number(20u64.into()),
                OrderedJsonValue::Number(30u64.into()),
            ])
        );
    }

    #[test]
    fn parse_instruction_with_option_arg() {
        let idl: Idl = serde_json::from_str(
            r#"{
                "address": "11111111111111111111111111111111",
                "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
                "instructions": [{
                    "name": "opt",
                    "discriminator": [3,3,3,3,3,3,3,3],
                    "accounts": [],
                    "args": [{ "name": "maybe", "type": { "option": "u32" } }]
                }],
                "types": []
            }"#,
        )
        .unwrap();

        let registry = IdlRegistry::new();

        // Some(777)
        let mut data = vec![3, 3, 3, 3, 3, 3, 3, 3, 1];
        data.extend_from_slice(&777u32.to_le_bytes());
        let parsed = parse_instruction(&idl, &data, &registry).unwrap().unwrap();
        assert_eq!(parsed.fields[0].value, OrderedJsonValue::Number(777u64.into()));

        // None
        let data_none = vec![3, 3, 3, 3, 3, 3, 3, 3, 0];
        let parsed = parse_instruction(&idl, &data_none, &registry).unwrap().unwrap();
        assert_eq!(parsed.fields[0].value, OrderedJsonValue::Null);
    }

    // ── find_instruction_by_discriminator ──

    #[test]
    fn find_instruction_prefers_longest_discriminator() {
        let idl: Idl = serde_json::from_str(
            r#"{
                "address": "11111111111111111111111111111111",
                "metadata": { "name": "t", "version": "0.1.0", "spec": "0.1.0" },
                "instructions": [
                    { "name": "fallback", "accounts": [], "args": [] },
                    { "name": "specific", "discriminator": [1], "accounts": [], "args": [] }
                ]
            }"#,
        )
        .unwrap();

        let data = vec![1, 0, 0, 0];
        let found = find_instruction_by_discriminator(&idl, &data).unwrap();
        assert_eq!(found.name, "specific");
    }

    // ── Direct parse_simple_type tests ──

    #[test]
    fn parse_simple_u8() {
        let data = [42u8];
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "u8").unwrap();
        assert_eq!(val, OrderedJsonValue::Number(42u64.into()));
        assert_eq!(offset, 1);
    }

    #[test]
    fn parse_simple_i8() {
        let data = [(-5i8) as u8];
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "i8").unwrap();
        assert_eq!(val, OrderedJsonValue::Number((-5i64).into()));
        assert_eq!(offset, 1);
    }

    #[test]
    fn parse_simple_u16() {
        let data = 1000u16.to_le_bytes();
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "u16").unwrap();
        assert_eq!(val, OrderedJsonValue::Number(1000u64.into()));
        assert_eq!(offset, 2);
    }

    #[test]
    fn parse_simple_i16() {
        let data = (-300i16).to_le_bytes();
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "i16").unwrap();
        assert_eq!(val, OrderedJsonValue::Number((-300i64).into()));
        assert_eq!(offset, 2);
    }

    #[test]
    fn parse_simple_u32() {
        let data = 70000u32.to_le_bytes();
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "u32").unwrap();
        assert_eq!(val, OrderedJsonValue::Number(70000u64.into()));
        assert_eq!(offset, 4);
    }

    #[test]
    fn parse_simple_i32() {
        let data = (-70000i32).to_le_bytes();
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "i32").unwrap();
        assert_eq!(val, OrderedJsonValue::Number((-70000i64).into()));
        assert_eq!(offset, 4);
    }

    #[test]
    fn parse_simple_u64() {
        let data = u64::MAX.to_le_bytes();
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "u64").unwrap();
        assert_eq!(val, OrderedJsonValue::Number(u64::MAX.into()));
        assert_eq!(offset, 8);
    }

    #[test]
    fn parse_simple_i64() {
        let data = i64::MIN.to_le_bytes();
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "i64").unwrap();
        assert_eq!(val, OrderedJsonValue::Number(i64::MIN.into()));
        assert_eq!(offset, 8);
    }

    #[test]
    fn parse_simple_u128() {
        let val_in: u128 = 340_282_366_920_938_463;
        let data = val_in.to_le_bytes();
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "u128").unwrap();
        assert_eq!(val, OrderedJsonValue::String(val_in.to_string()));
        assert_eq!(offset, 16);
    }

    #[test]
    fn parse_simple_i128() {
        let val_in: i128 = -170_141_183_460_469;
        let data = val_in.to_le_bytes();
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "i128").unwrap();
        assert_eq!(val, OrderedJsonValue::String(val_in.to_string()));
        assert_eq!(offset, 16);
    }

    #[test]
    fn parse_simple_bool_true() {
        let data = [1u8];
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "bool").unwrap();
        assert_eq!(val, OrderedJsonValue::Bool(true));
        assert_eq!(offset, 1);
    }

    #[test]
    fn parse_simple_bool_false() {
        let data = [0u8];
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "bool").unwrap();
        assert_eq!(val, OrderedJsonValue::Bool(false));
        assert_eq!(offset, 1);
    }

    #[test]
    fn parse_simple_pubkey() {
        let pk = Pubkey::from_str("11111111111111111111111111111111").unwrap();
        let data = pk.to_bytes();
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "pubkey").unwrap();
        assert_eq!(val, OrderedJsonValue::String(pk.to_string()));
        assert_eq!(offset, 32);
    }

    #[test]
    fn parse_simple_string() {
        let s = b"hello";
        let mut data = (s.len() as u32).to_le_bytes().to_vec();
        data.extend_from_slice(s);
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "string").unwrap();
        assert_eq!(val, OrderedJsonValue::String("hello".into()));
        assert_eq!(offset, 9);
    }

    #[test]
    fn parse_simple_bytes() {
        let payload = vec![0xAA, 0xBB, 0xCC];
        let mut data = (payload.len() as u32).to_le_bytes().to_vec();
        data.extend_from_slice(&payload);
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "bytes").unwrap();
        assert_eq!(
            val,
            OrderedJsonValue::Array(vec![
                OrderedJsonValue::Number(0xAAu64.into()),
                OrderedJsonValue::Number(0xBBu64.into()),
                OrderedJsonValue::Number(0xCCu64.into()),
            ])
        );
        assert_eq!(offset, 7);
    }

    // ── Error paths ──

    #[test]
    fn parse_simple_type_truncated_u32() {
        let data = [0u8; 2]; // need 4 for u32
        let mut offset = 0;
        let err = parse_simple_type(&data, &mut offset, "u32").unwrap_err();
        assert!(err.to_string().contains("Insufficient data"));
    }

    #[test]
    fn parse_simple_type_truncated_string_length() {
        let data = [0u8; 2]; // need 4 for length prefix
        let mut offset = 0;
        let err = parse_simple_type(&data, &mut offset, "string").unwrap_err();
        assert!(err.to_string().contains("Insufficient data"));
    }

    #[test]
    fn parse_simple_type_truncated_string_body() {
        // length says 10 bytes but only 2 available
        let mut data = 10u32.to_le_bytes().to_vec();
        data.extend_from_slice(&[0, 0]);
        let mut offset = 0;
        let err = parse_simple_type(&data, &mut offset, "string").unwrap_err();
        assert!(err.to_string().contains("Insufficient data"));
    }

    #[test]
    fn parse_simple_type_unknown_falls_back_to_raw() {
        let data = [1, 2, 3, 4];
        let mut offset = 0;
        let val = parse_simple_type(&data, &mut offset, "unknown_type").unwrap();
        // Should return a raw_unparsed_value object
        if let OrderedJsonValue::Object(entries) = &val {
            let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
            assert!(keys.contains(&"context"));
            assert!(keys.contains(&"type_hint"));
            assert!(keys.contains(&"raw_hex"));
        } else {
            panic!("expected Object for unknown type, got {:?}", val);
        }
    }

    // ── Direct container type tests ──

    #[test]
    fn parse_vec_type_u32_elements() {
        let mut data = 3u32.to_le_bytes().to_vec();
        data.extend_from_slice(&10u32.to_le_bytes());
        data.extend_from_slice(&20u32.to_le_bytes());
        data.extend_from_slice(&30u32.to_le_bytes());
        let mut offset = 0;
        let registry = IdlRegistry::new();
        let idl = hello_anchor_idl();
        let element_type = IdlType::Simple("u32".into());
        let val = parse_vec_type(&data, &mut offset, &element_type, &registry, &idl).unwrap();
        assert_eq!(
            val,
            OrderedJsonValue::Array(vec![
                OrderedJsonValue::Number(10u64.into()),
                OrderedJsonValue::Number(20u64.into()),
                OrderedJsonValue::Number(30u64.into()),
            ])
        );
        assert_eq!(offset, 16);
    }

    #[test]
    fn parse_vec_type_empty() {
        let data = 0u32.to_le_bytes();
        let mut offset = 0;
        let registry = IdlRegistry::new();
        let idl = hello_anchor_idl();
        let element_type = IdlType::Simple("u8".into());
        let val = parse_vec_type(&data, &mut offset, &element_type, &registry, &idl).unwrap();
        assert_eq!(val, OrderedJsonValue::Array(vec![]));
        assert_eq!(offset, 4);
    }

    #[test]
    fn parse_vec_type_truncated_length() {
        let data = [0u8; 2];
        let mut offset = 0;
        let registry = IdlRegistry::new();
        let idl = hello_anchor_idl();
        let element_type = IdlType::Simple("u8".into());
        let err = parse_vec_type(&data, &mut offset, &element_type, &registry, &idl).unwrap_err();
        assert!(err.to_string().contains("Insufficient data"));
    }

    #[test]
    fn parse_option_type_some() {
        let mut data = vec![1u8];
        data.extend_from_slice(&500u16.to_le_bytes());
        let mut offset = 0;
        let registry = IdlRegistry::new();
        let idl = hello_anchor_idl();
        let inner = IdlType::Simple("u16".into());
        let val = parse_option_type(&data, &mut offset, &inner, &registry, &idl).unwrap();
        assert_eq!(val, OrderedJsonValue::Number(500u64.into()));
        assert_eq!(offset, 3);
    }

    #[test]
    fn parse_option_type_none() {
        let data = vec![0u8];
        let mut offset = 0;
        let registry = IdlRegistry::new();
        let idl = hello_anchor_idl();
        let inner = IdlType::Simple("u16".into());
        let val = parse_option_type(&data, &mut offset, &inner, &registry, &idl).unwrap();
        assert_eq!(val, OrderedJsonValue::Null);
        assert_eq!(offset, 1);
    }

    #[test]
    fn parse_option_type_truncated_discriminant() {
        let data: [u8; 0] = [];
        let mut offset = 0;
        let registry = IdlRegistry::new();
        let idl = hello_anchor_idl();
        let inner = IdlType::Simple("u8".into());
        let err = parse_option_type(&data, &mut offset, &inner, &registry, &idl).unwrap_err();
        assert!(err.to_string().contains("Insufficient data"));
    }

    #[test]
    fn parse_array_type_fixed_3_u8() {
        let data = vec![10, 20, 30];
        let mut offset = 0;
        let registry = IdlRegistry::new();
        let idl = hello_anchor_idl();
        let array_def = [
            serde_json::json!("u8"),
            serde_json::json!(3),
        ];
        let val = parse_array_type(&data, &mut offset, &array_def, &registry, &idl).unwrap();
        assert_eq!(
            val,
            OrderedJsonValue::Array(vec![
                OrderedJsonValue::Number(10u64.into()),
                OrderedJsonValue::Number(20u64.into()),
                OrderedJsonValue::Number(30u64.into()),
            ])
        );
        assert_eq!(offset, 3);
    }
}
