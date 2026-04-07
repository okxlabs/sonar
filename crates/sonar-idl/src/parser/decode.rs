use anyhow::{Result, anyhow};
use serde_json::{Map, Number as JsonNumber, Value};
use solana_pubkey::Pubkey;

use crate::models::*;

use super::IdlParsedField;
use super::indexed::IndexedIdl;

pub(super) fn parse_instruction_args(
    data: &[u8],
    offset: &mut usize,
    args: &[IdlArg],
    indexed: &IndexedIdl,
) -> Result<Vec<IdlParsedField>> {
    let mut fields = Vec::new();

    for arg in args {
        let value = parse_type(data, offset, &arg.type_, indexed)?;
        fields.push(IdlParsedField { name: arg.name.clone(), value });
    }

    Ok(fields)
}

fn parse_named_fields_to_entries(
    data: &[u8],
    offset: &mut usize,
    fields: &[IdlField],
    indexed: &IndexedIdl,
) -> Result<Map<String, Value>> {
    let mut entries = Map::new();

    for field in fields {
        let value = parse_type(data, offset, &field.type_, indexed)?;
        entries.insert(field.name.clone(), value);
    }

    Ok(entries)
}

fn parse_tuple_fields_to_values(
    data: &[u8],
    offset: &mut usize,
    fields: &[IdlType],
    indexed: &IndexedIdl,
) -> Result<Vec<Value>> {
    let mut values = Vec::new();

    for field_type in fields {
        values.push(parse_type(data, offset, field_type, indexed)?);
    }

    Ok(values)
}

fn parse_idl_fields_value(
    data: &[u8],
    offset: &mut usize,
    fields: &IdlFields,
    indexed: &IndexedIdl,
) -> Result<Value> {
    match fields {
        IdlFields::Named(named_fields) => {
            Ok(Value::Object(parse_named_fields_to_entries(data, offset, named_fields, indexed)?))
        }
        IdlFields::Tuple(tuple_fields) => {
            Ok(Value::Array(parse_tuple_fields_to_values(data, offset, tuple_fields, indexed)?))
        }
    }
}

pub(super) fn parse_idl_fields_as_parsed_fields(
    data: &[u8],
    offset: &mut usize,
    fields: &IdlFields,
    indexed: &IndexedIdl,
) -> Result<Vec<IdlParsedField>> {
    match fields {
        IdlFields::Named(named_fields) => {
            Ok(parse_named_fields_to_entries(data, offset, named_fields, indexed)?
                .into_iter()
                .map(|(name, value)| IdlParsedField { name, value })
                .collect())
        }
        IdlFields::Tuple(tuple_fields) => {
            Ok(parse_tuple_fields_to_values(data, offset, tuple_fields, indexed)?
                .into_iter()
                .enumerate()
                .map(|(idx, value)| IdlParsedField { name: format!("field_{}", idx), value })
                .collect())
        }
    }
}

pub(super) fn raw_unparsed_value(context: &str, type_name: &str, raw_data: &[u8]) -> Value {
    let mut object = Map::new();
    object.insert("context".into(), Value::String(context.to_string()));
    object.insert("type_hint".into(), Value::String(type_name.to_string()));
    object.insert("raw_hex".into(), Value::String(hex::encode(raw_data)));
    Value::Object(object)
}

fn parse_type(
    data: &[u8],
    offset: &mut usize,
    idl_type: &IdlType,
    indexed: &IndexedIdl,
) -> Result<Value> {
    match idl_type {
        IdlType::Simple(type_name) => parse_simple_type(data, offset, type_name),
        IdlType::Vec { vec } => parse_vec_type(data, offset, vec, indexed),
        IdlType::Option { option } => parse_option_type(data, offset, option, indexed),
        IdlType::Array { array } => parse_array_type(data, offset, array, indexed),
        IdlType::Defined { defined } => parse_defined_type(data, offset, defined, indexed),
    }
}

pub(super) fn parse_simple_type(data: &[u8], offset: &mut usize, type_name: &str) -> Result<Value> {
    let start = *offset;

    let (value, bytes_read) = match type_name {
        "u8" => {
            check_data_len(data, start, 1)?;
            (Value::Number(JsonNumber::from(u64::from(data[start]))), 1)
        }
        "i8" => {
            check_data_len(data, start, 1)?;
            (Value::Number(JsonNumber::from(data[start] as i8 as i64)), 1)
        }
        "u16" => {
            check_data_len(data, start, 2)?;
            let value = u16::from_le_bytes([data[start], data[start + 1]]);
            (Value::Number(JsonNumber::from(u64::from(value))), 2)
        }
        "i16" => {
            check_data_len(data, start, 2)?;
            let value = i16::from_le_bytes([data[start], data[start + 1]]);
            (Value::Number(JsonNumber::from(value as i64)), 2)
        }
        "u32" => {
            check_data_len(data, start, 4)?;
            let value = u32::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
            ]);
            (Value::Number(JsonNumber::from(u64::from(value))), 4)
        }
        "i32" => {
            check_data_len(data, start, 4)?;
            let value = i32::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
            ]);
            (Value::Number(JsonNumber::from(value as i64)), 4)
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
            (Value::Number(JsonNumber::from(value)), 8)
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
            (Value::Number(JsonNumber::from(value)), 8)
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
            (Value::String(value.to_string()), 16)
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
            (Value::String(value.to_string()), 16)
        }
        "pubkey" | "publicKey" => {
            check_data_len(data, start, 32)?;
            let pubkey = Pubkey::try_from(&data[start..start + 32])
                .map_err(|_| anyhow!("Invalid pubkey data"))?;
            (Value::String(pubkey.to_string()), 32)
        }
        "bool" => {
            check_data_len(data, start, 1)?;
            let value = data[start] != 0;
            (Value::Bool(value), 1)
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
            (Value::String(value), 4 + length)
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
                array.push(Value::Number(JsonNumber::from(u64::from(*byte))));
            }
            (Value::Array(array), 4 + length)
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

pub(super) fn parse_vec_type(
    data: &[u8],
    offset: &mut usize,
    element_type: &IdlType,
    indexed: &IndexedIdl,
) -> Result<Value> {
    let start = *offset;
    check_data_len(data, start, 4)?;

    let length =
        u32::from_le_bytes([data[start], data[start + 1], data[start + 2], data[start + 3]])
            as usize;
    *offset += 4;

    let mut elements = Vec::with_capacity(length);
    for _ in 0..length {
        let element_start = *offset;
        let element = parse_type(data, offset, element_type, indexed)?;
        if *offset == element_start {
            break;
        }
        elements.push(element);
    }

    Ok(Value::Array(elements))
}

pub(super) fn parse_option_type(
    data: &[u8],
    offset: &mut usize,
    inner_type: &IdlType,
    indexed: &IndexedIdl,
) -> Result<Value> {
    let start = *offset;
    check_data_len(data, start, 1)?;

    let is_some = data[start] != 0;
    *offset += 1;

    if !is_some { Ok(Value::Null) } else { parse_type(data, offset, inner_type, indexed) }
}

pub(super) fn parse_array_type(
    data: &[u8],
    offset: &mut usize,
    array_def: &IdlArrayType,
    indexed: &IndexedIdl,
) -> Result<Value> {
    let mut elements = Vec::with_capacity(array_def.length);
    for _ in 0..array_def.length {
        let element = parse_type(data, offset, &array_def.element_type, indexed)?;
        elements.push(element);
    }

    Ok(Value::Array(elements))
}

fn parse_defined_type(
    data: &[u8],
    offset: &mut usize,
    defined: &DefinedType,
    indexed: &IndexedIdl,
) -> Result<Value> {
    if let Some(type_def) = indexed.find_type_definition(defined.name()) {
        return parse_type_definition(data, offset, type_def, indexed);
    }

    let start = *offset;
    let remaining = &data[start..];
    let bytes_read = remaining.len().min(32);
    *offset += bytes_read;
    Ok(raw_unparsed_value("defined_type", defined.name(), &remaining[..bytes_read]))
}

pub(super) fn parse_type_definition(
    data: &[u8],
    offset: &mut usize,
    type_def: &IdlTypeDefinition,
    indexed: &IndexedIdl,
) -> Result<Value> {
    match &type_def.type_.kind {
        IdlTypeDefinitionKind::Struct => {
            if let Some(fields) = &type_def.type_.fields {
                return parse_idl_fields_value(data, offset, fields, indexed);
            }
        }
        IdlTypeDefinitionKind::Enum => {
            if let Some(variants) = &type_def.type_.variants {
                check_data_len(data, *offset, 1)?;
                let variant_index = data[*offset] as usize;
                *offset += 1;

                if variant_index < variants.len() {
                    let variant = &variants[variant_index];
                    let payload = match variant.fields.as_ref() {
                        Some(fields) => parse_idl_fields_value(data, offset, fields, indexed)?,
                        None => Value::Null,
                    };

                    let mut object = Map::new();
                    object.insert(variant.name.clone(), payload);
                    return Ok(Value::Object(object));
                }
            }
        }
        IdlTypeDefinitionKind::Other(_) => {}
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
