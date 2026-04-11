use anyhow::{Result, anyhow};
use solana_pubkey::Pubkey;

use crate::idl::*;
use crate::indexed::IdlParsedField;
use crate::value::IdlValue;

pub(super) fn parse_instruction_args(
    data: &[u8],
    offset: &mut usize,
    args: &[IdlArg],
    indexed: &crate::indexed::IndexedIdl,
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
    indexed: &crate::indexed::IndexedIdl,
) -> Result<Vec<(String, IdlValue)>> {
    let mut entries = Vec::new();
    for field in fields {
        let value = parse_type(data, offset, &field.type_, indexed)?;
        entries.push((field.name.clone(), value));
    }
    Ok(entries)
}

fn parse_tuple_fields_to_values(
    data: &[u8],
    offset: &mut usize,
    fields: &[IdlType],
    indexed: &crate::indexed::IndexedIdl,
) -> Result<Vec<IdlValue>> {
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
    indexed: &crate::indexed::IndexedIdl,
) -> Result<IdlValue> {
    match fields {
        IdlFields::Named(named_fields) => {
            Ok(IdlValue::Struct(parse_named_fields_to_entries(data, offset, named_fields, indexed)?))
        }
        IdlFields::Tuple(tuple_fields) => {
            Ok(IdlValue::Array(parse_tuple_fields_to_values(data, offset, tuple_fields, indexed)?))
        }
    }
}

pub(crate) fn parse_idl_fields_as_parsed_fields(
    data: &[u8],
    offset: &mut usize,
    fields: &IdlFields,
    indexed: &crate::indexed::IndexedIdl,
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

pub(crate) fn raw_unparsed_value(context: &str, type_name: &str, raw_data: &[u8]) -> IdlValue {
    IdlValue::Struct(vec![
        ("context".into(), IdlValue::String(context.to_string())),
        ("type_hint".into(), IdlValue::String(type_name.to_string())),
        ("raw_hex".into(), IdlValue::String(hex::encode(raw_data))),
    ])
}

fn parse_type(
    data: &[u8],
    offset: &mut usize,
    idl_type: &IdlType,
    indexed: &crate::indexed::IndexedIdl,
) -> Result<IdlValue> {
    match idl_type {
        IdlType::Simple(type_name) => parse_simple_type(data, offset, type_name),
        IdlType::Vec { vec } => parse_vec_type(data, offset, vec, indexed),
        IdlType::Option { option } => parse_option_type(data, offset, option, indexed),
        IdlType::Array { array } => parse_array_type(data, offset, array, indexed),
        IdlType::Defined { defined } => parse_defined_type(data, offset, defined, indexed),
    }
}

pub(crate) fn parse_simple_type(
    data: &[u8],
    offset: &mut usize,
    type_name: &str,
) -> Result<IdlValue> {
    let start = *offset;

    let (value, bytes_read) = match type_name {
        "u8" => {
            check_data_len(data, start, 1)?;
            (IdlValue::Uint(u128::from(data[start])), 1)
        }
        "i8" => {
            check_data_len(data, start, 1)?;
            (IdlValue::Int(i128::from(data[start] as i8)), 1)
        }
        "u16" => {
            check_data_len(data, start, 2)?;
            let value = u16::from_le_bytes([data[start], data[start + 1]]);
            (IdlValue::Uint(u128::from(value)), 2)
        }
        "i16" => {
            check_data_len(data, start, 2)?;
            let value = i16::from_le_bytes([data[start], data[start + 1]]);
            (IdlValue::Int(i128::from(value)), 2)
        }
        "u32" => {
            check_data_len(data, start, 4)?;
            let value = u32::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
            ]);
            (IdlValue::Uint(u128::from(value)), 4)
        }
        "i32" => {
            check_data_len(data, start, 4)?;
            let value = i32::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
            ]);
            (IdlValue::Int(i128::from(value)), 4)
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
            (IdlValue::Uint(u128::from(value)), 8)
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
            (IdlValue::Int(i128::from(value)), 8)
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
            (IdlValue::Uint(value), 16)
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
            (IdlValue::Int(value), 16)
        }
        "pubkey" | "publicKey" => {
            check_data_len(data, start, 32)?;
            let pubkey = Pubkey::try_from(&data[start..start + 32])
                .map_err(|_| anyhow!("Invalid pubkey data"))?;
            (IdlValue::String(pubkey.to_string()), 32)
        }
        "bool" => {
            check_data_len(data, start, 1)?;
            let value = data[start] != 0;
            (IdlValue::Bool(value), 1)
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
            (IdlValue::String(value), 4 + length)
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
            let bytes = data[content_start..content_start + length].to_vec();
            (IdlValue::Bytes(bytes), 4 + length)
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

pub(crate) fn parse_vec_type(
    data: &[u8],
    offset: &mut usize,
    element_type: &IdlType,
    indexed: &crate::indexed::IndexedIdl,
) -> Result<IdlValue> {
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

    Ok(IdlValue::Array(elements))
}

pub(crate) fn parse_option_type(
    data: &[u8],
    offset: &mut usize,
    inner_type: &IdlType,
    indexed: &crate::indexed::IndexedIdl,
) -> Result<IdlValue> {
    let start = *offset;
    check_data_len(data, start, 1)?;

    let is_some = data[start] != 0;
    *offset += 1;

    if !is_some { Ok(IdlValue::Null) } else { parse_type(data, offset, inner_type, indexed) }
}

pub(crate) fn parse_array_type(
    data: &[u8],
    offset: &mut usize,
    array_def: &IdlArrayType,
    indexed: &crate::indexed::IndexedIdl,
) -> Result<IdlValue> {
    let mut elements = Vec::with_capacity(array_def.length);
    for _ in 0..array_def.length {
        let element = parse_type(data, offset, &array_def.element_type, indexed)?;
        elements.push(element);
    }

    Ok(IdlValue::Array(elements))
}

fn parse_defined_type(
    data: &[u8],
    offset: &mut usize,
    defined: &DefinedType,
    indexed: &crate::indexed::IndexedIdl,
) -> Result<IdlValue> {
    if let Some(type_def) = indexed.find_type_definition(defined.name()) {
        return parse_type_definition(data, offset, type_def, indexed);
    }

    let start = *offset;
    let remaining = &data[start..];
    let bytes_read = remaining.len().min(32);
    *offset += bytes_read;
    Ok(raw_unparsed_value("defined_type", defined.name(), &remaining[..bytes_read]))
}

pub(crate) fn parse_type_definition(
    data: &[u8],
    offset: &mut usize,
    type_def: &IdlTypeDefinition,
    indexed: &crate::indexed::IndexedIdl,
) -> Result<IdlValue> {
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
                        None => IdlValue::Null,
                    };

                    return Ok(IdlValue::Struct(vec![(variant.name.clone(), payload)]));
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
