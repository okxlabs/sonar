use anyhow::{Result, anyhow};
use solana_pubkey::Pubkey;

use crate::idl::*;
use crate::indexed::{IdlParsedField, IndexedIdl};
use crate::value::IdlValue;

pub(super) fn parse_instruction_args(
    data: &[u8],
    offset: &mut usize,
    args: &[IdlArg],
    indexed: &IndexedIdl,
) -> Result<Vec<IdlParsedField>> {
    let mut fields = Vec::new();
    for arg in args {
        // When data is exactly consumed, treat remaining trailing args as absent
        // (null). Programs commonly add optional trailing args that callers may
        // omit; the Solana runtime does not enforce that programs read all
        // instruction data.
        if *offset >= data.len() && !fields.is_empty() {
            fields.push(IdlParsedField { name: arg.name.clone(), value: IdlValue::Null });
            continue;
        }
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
    indexed: &IndexedIdl,
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
    indexed: &IndexedIdl,
) -> Result<IdlValue> {
    match fields {
        IdlFields::Named(named_fields) => Ok(IdlValue::Struct(parse_named_fields_to_entries(
            data,
            offset,
            named_fields,
            indexed,
        )?)),
        IdlFields::Tuple(tuple_fields) => {
            Ok(IdlValue::Array(parse_tuple_fields_to_values(data, offset, tuple_fields, indexed)?))
        }
    }
}

pub(crate) fn parse_idl_fields_as_parsed_fields(
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
    indexed: &IndexedIdl,
) -> Result<IdlValue> {
    match idl_type {
        IdlType::Simple(type_name) => parse_simple_type(data, offset, type_name),
        IdlType::Vec { vec } => parse_vec_type(data, offset, vec, indexed),
        IdlType::Option { option } => parse_option_type(data, offset, option, indexed),
        IdlType::Array { array } => parse_array_type(data, offset, array, indexed),
        IdlType::Defined { defined } => parse_defined_type(data, offset, defined, indexed),
        IdlType::Tuple { tuple } => parse_tuple_type(data, offset, tuple, indexed),
    }
}

pub(crate) fn parse_tuple_type(
    data: &[u8],
    offset: &mut usize,
    element_types: &[IdlType],
    indexed: &IndexedIdl,
) -> Result<IdlValue> {
    let mut elements = Vec::with_capacity(element_types.len());
    for element_type in element_types {
        elements.push(parse_type(data, offset, element_type, indexed)?);
    }
    Ok(IdlValue::Array(elements))
}

pub(crate) fn parse_simple_type(
    data: &[u8],
    offset: &mut usize,
    type_name: &str,
) -> Result<IdlValue> {
    let start = *offset;

    // Decode a fixed-width little-endian integer. `check_data_len` guards the
    // slice, so the `try_into` over the exact-width range is infallible.
    macro_rules! int {
        ($t:ty, $variant:ident) => {{
            const N: usize = std::mem::size_of::<$t>();
            check_data_len(data, start, N)?;
            let bytes: [u8; N] = data[start..start + N].try_into().unwrap();
            (IdlValue::$variant(<$t>::from_le_bytes(bytes)), N)
        }};
    }

    let (value, bytes_read) = match type_name {
        "u8" => int!(u8, U8),
        "i8" => int!(i8, I8),
        "u16" => int!(u16, U16),
        "i16" => int!(i16, I16),
        "u32" => int!(u32, U32),
        "i32" => int!(i32, I32),
        "u64" => int!(u64, U64),
        "i64" => int!(i64, I64),
        "u128" => int!(u128, U128),
        "i128" => int!(i128, I128),
        "pubkey" | "publicKey" => {
            check_data_len(data, start, 32)?;
            let pubkey = Pubkey::try_from(&data[start..start + 32])
                .map_err(|_| anyhow!("Invalid pubkey data"))?;
            (IdlValue::Pubkey(pubkey), 32)
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
    indexed: &IndexedIdl,
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
    indexed: &IndexedIdl,
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
    indexed: &IndexedIdl,
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
    indexed: &IndexedIdl,
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
    indexed: &IndexedIdl,
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
                    return Ok(match variant.fields.as_ref() {
                        Some(fields) => {
                            let payload = parse_idl_fields_value(data, offset, fields, indexed)?;
                            IdlValue::Struct(vec![(variant.name.clone(), payload)])
                        }
                        None => IdlValue::EnumUnit(variant.name.clone()),
                    });
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
