use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use solana_pubkey::Pubkey;

use crate::instruction_parsers::{InstructionParser, ParsedInstruction};
use crate::transaction::InstructionSummary;

/// Complete IDL structure including types for full type resolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteIdl {
    pub address: String,
    pub metadata: IdlMetadata,
    pub instructions: Vec<IdlInstruction>,
    pub types: Option<Vec<IdlTypeDefinition>>,
    #[serde(default)]
    pub events: Option<Vec<IdlEvent>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlMetadata {
    pub name: String,
    pub version: String,
    pub spec: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlInstruction {
    pub name: String,
    pub discriminator: Vec<u8>,
    pub accounts: Vec<IdlAccountItem>,
    pub args: Vec<IdlArg>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlEvent {
    pub name: String,
    pub discriminator: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IdlAccountItem {
    Account(IdlAccount),
    Accounts(IdlAccounts),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlAccount {
    pub name: String,
    #[serde(default)]
    pub writable: bool,
    #[serde(default)]
    pub signer: bool,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlAccounts {
    pub name: String,
    pub accounts: Vec<IdlAccountItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlArg {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: IdlType,
}

/// IDL types - mirrors Anchor IDL type system
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum IdlType {
    Simple(String),
    Vec { vec: Box<IdlType> },
    Option { option: Box<IdlType> },
    Array { array: [JsonValue; 2] },
    Defined { defined: DefinedType },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DefinedType {
    pub name: String,
    #[serde(default)]
    pub generics: Option<Vec<JsonValue>>,
}

/// Custom type definitions (structs and enums)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlTypeDefinition {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: IdlTypeDefinitionBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlTypeDefinitionBody {
    pub kind: String,
    #[serde(default, deserialize_with = "deserialize_optional_idl_fields")]
    pub fields: Option<IdlFields>,
    #[serde(default)]
    pub variants: Option<Vec<IdlEnumVariant>>,
}

/// Fields for IDL types - can be either named (regular structs) or tuple (tuple structs)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IdlFields {
    /// Named fields like struct Foo { field1: Type1, field2: Type2 }
    Named(Vec<IdlField>),
    /// Tuple fields like struct Bar(Type1, Type2)
    Tuple(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlField {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: IdlType,
}

/// Custom deserializer for IDL fields that handles both named and tuple struct formats
fn deserialize_idl_fields<'de, D>(deserializer: D) -> Result<Option<IdlFields>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value = serde_json::Value::deserialize(deserializer)?;

    match value {
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                Ok(Some(IdlFields::Tuple(Vec::new())))
            } else if arr[0].get("name").is_some() {
                // It's an array of objects with "name" field - this is a regular struct
                match serde_json::from_value::<Vec<IdlField>>(serde_json::Value::Array(arr)) {
                    Ok(fields) => Ok(Some(IdlFields::Named(fields))),
                    Err(e) => Err(D::Error::custom(format!("Failed to parse named fields: {}", e))),
                }
            } else {
                // It's an array of strings - this is a tuple struct
                match serde_json::from_value::<Vec<String>>(serde_json::Value::Array(arr)) {
                    Ok(types) => Ok(Some(IdlFields::Tuple(types))),
                    Err(e) => Err(D::Error::custom(format!("Failed to parse tuple fields: {}", e))),
                }
            }
        }
        _ => Err(D::Error::custom("Fields must be an array")),
    }
}

/// Custom deserializer for optional IDL fields that handles missing fields gracefully
fn deserialize_optional_idl_fields<'de, D>(deserializer: D) -> Result<Option<IdlFields>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{Error, Visitor};

    struct OptionalIdlFieldsVisitor;

    impl<'de> Visitor<'de> for OptionalIdlFieldsVisitor {
        type Value = Option<IdlFields>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an array of field definitions or null")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_any(OptionalIdlFieldsVisitor)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(None)
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            // Just try to extract the array elements - this is a fallback
            let mut arr = Vec::new();
            while let Some(element) = seq.next_element::<serde_json::Value>()? {
                arr.push(element);
            }

            deserialize_idl_fields_(serde_json::Value::Array(arr)).map_err(A::Error::custom)
        }

        fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_any(OptionalIdlFieldsVisitor)
        }
    }

    // Actually, a simpler approach is to use Option::deserialize and handle the Some case
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;

    match value {
        Some(val) => deserialize_idl_fields_(val).map_err(D::Error::custom),
        None => Ok(None),
    }
}

fn deserialize_idl_fields_(value: serde_json::Value) -> Result<Option<IdlFields>, String> {
    match value {
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                Ok(Some(IdlFields::Tuple(Vec::new())))
            } else if arr[0].get("name").is_some() {
                // It's an array of objects with "name" field - this is a regular struct
                match serde_json::from_value::<Vec<IdlField>>(serde_json::Value::Array(arr)) {
                    Ok(fields) => Ok(Some(IdlFields::Named(fields))),
                    Err(e) => Err(format!("Failed to parse named fields: {}", e)),
                }
            } else {
                // It's an array of strings - this is a tuple struct
                match serde_json::from_value::<Vec<String>>(serde_json::Value::Array(arr)) {
                    Ok(types) => Ok(Some(IdlFields::Tuple(types))),
                    Err(e) => Err(format!("Failed to parse tuple fields: {}", e)),
                }
            }
        }
        _ => Err("Fields must be an array".to_string()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlEnumVariant {
    pub name: String,
    #[serde(default)]
    pub fields: Option<JsonValue>,
}

/// Registry for loading and managing IDL files
#[derive(Debug, Clone)]
pub struct IdlRegistry {
    pub(crate) inner: Arc<IdlRegistryInner>,
}

#[derive(Debug, Clone)]
pub(crate) struct IdlRegistryInner {
    pub(crate) idls: HashMap<Pubkey, CompleteIdl>,
    // Maps (program_id, type_name) to type definition to avoid conflicts between programs
    pub(crate) types_by_program_and_name: HashMap<(Pubkey, String), IdlTypeDefinition>,
}

impl IdlRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(IdlRegistryInner {
                idls: HashMap::new(),
                types_by_program_and_name: HashMap::new(),
            }),
        }
    }

    /// Load all IDL files from a directory
    pub fn load_idls(&mut self, dir: &Path) -> Result<()> {
        if !dir.exists() {
            return Err(anyhow!("IDL directory does not exist: {}", dir.display()));
        }

        if !dir.is_dir() {
            return Err(anyhow!("IDL path is not a directory: {}", dir.display()));
        }

        for entry in fs::read_dir(dir).with_context(|| "Failed to read IDL directory")? {
            let entry = entry.with_context(|| "Failed to read directory entry")?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read IDL file: {}", path.display()))?;

            let idl: CompleteIdl = serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse IDL JSON: {}", path.display()))?;

            let pubkey = Pubkey::from_str(&idl.address)
                .with_context(|| format!("Invalid program address in IDL: {}", idl.address))?;

            // Index all type definitions by (program_id, type_name) for fast lookup
            if let Some(types) = &idl.types {
                for type_def in types {
                    Arc::get_mut(&mut self.inner)
                        .unwrap()
                        .types_by_program_and_name
                        .insert((pubkey, type_def.name.clone()), type_def.clone());
                }
            }

            Arc::get_mut(&mut self.inner).unwrap().idls.insert(pubkey, idl);
        }

        Ok(())
    }

    /// Get an IDL by program ID
    pub fn get(&self, program_id: &Pubkey) -> Option<&CompleteIdl> {
        self.inner.idls.get(program_id)
    }

    /// Get a type definition by (program_id, type_name)
    pub fn get_type_by_program(
        &self,
        program_id: &Pubkey,
        name: &str,
    ) -> Option<&IdlTypeDefinition> {
        self.inner.types_by_program_and_name.get(&(*program_id, name.to_string()))
    }

    /// Get a type definition by name only (for backward compatibility, prefer get_type_by_program)
    pub fn get_type(&self, name: &str) -> Option<&IdlTypeDefinition> {
        // Fallback to any type with this name (may not be the correct one)
        self.inner
            .types_by_program_and_name
            .iter()
            .find(|((_, type_name), _)| type_name == name)
            .map(|(_, type_def)| type_def)
    }

    /// Check if an IDL exists for a program
    pub fn contains(&self, program_id: &Pubkey) -> bool {
        self.inner.idls.contains_key(program_id)
    }

    /// Get all registered IDL program IDs
    pub fn program_ids(&self) -> Vec<Pubkey> {
        self.inner.idls.keys().cloned().collect()
    }
}

impl Default for IdlRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Parser for Anchor programs using IDL data
pub struct AnchorIdlParser {
    program_id: Pubkey,
    pub(crate) idl: CompleteIdl,
    pub(crate) registry: IdlRegistry,
}

impl AnchorIdlParser {
    pub fn new(program_id: Pubkey, idl: CompleteIdl, registry: IdlRegistry) -> Self {
        Self { program_id, idl, registry }
    }
}

impl InstructionParser for AnchorIdlParser {
    fn program_id(&self) -> &Pubkey {
        &self.program_id
    }

    fn parse_instruction(
        &self,
        instruction: &InstructionSummary,
    ) -> Result<Option<ParsedInstruction>> {
        if instruction.data.len() < 8 {
            return Ok(None);
        }

        let discriminator = &instruction.data[..8];

        if let Some(idl_instruction) = find_instruction_by_discriminator(&self.idl, discriminator) {
            let mut offset = 8;
            let data = parse_instruction_data(
                &instruction.data,
                &mut offset,
                &idl_instruction.args,
                &self.registry,
                &self.idl,
            )?;

            let account_names = extract_account_names(&idl_instruction.accounts);

            let parsed = ParsedInstruction {
                name: idl_instruction.name.clone(),
                fields: data,
                account_names,
            };

            Ok(Some(parsed))
        } else {
            Ok(None)
        }
    }

    fn parse_cpi_event(
        &self,
        instruction: &InstructionSummary,
        program_id: &Pubkey,
    ) -> Result<Option<ParsedInstruction>> {
        parse_anchor_cpi_event(instruction, &self.registry, program_id)
    }
}

fn find_instruction_by_discriminator<'a>(
    idl: &'a CompleteIdl,
    discriminator: &[u8],
) -> Option<&'a IdlInstruction> {
    idl.instructions
        .iter()
        .find(|inst| inst.discriminator.len() == 8 && &inst.discriminator[..8] == discriminator)
}

fn extract_account_names(accounts: &[IdlAccountItem]) -> Vec<String> {
    let mut names = Vec::new();
    for item in accounts {
        match item {
            IdlAccountItem::Account(account) => names.push(account.name.clone()),
            IdlAccountItem::Accounts(accounts) => {
                names.push(format!("{}: []", accounts.name));
            }
        }
    }
    names
}

fn parse_instruction_data(
    data: &[u8],
    offset: &mut usize,
    args: &[IdlArg],
    registry: &IdlRegistry,
    idl: &CompleteIdl,
) -> Result<Vec<(String, String)>> {
    let mut fields = Vec::new();

    for arg in args {
        if *offset >= data.len() {
            break;
        }

        let (value, bytes_read) = parse_type(data, offset, &arg.type_, registry, idl)?;
        fields.push((arg.name.clone(), value));
        *offset += bytes_read;
    }

    Ok(fields)
}

fn parse_type(
    data: &[u8],
    offset: &usize,
    idl_type: &IdlType,
    registry: &IdlRegistry,
    idl: &CompleteIdl,
) -> Result<(String, usize)> {
    match idl_type {
        IdlType::Simple(type_name) => parse_simple_type(data, offset, type_name),
        IdlType::Vec { vec } => parse_vec_type(data, offset, vec, registry, idl),
        IdlType::Option { option } => parse_option_type(data, offset, option, registry, idl),
        IdlType::Array { array } => parse_array_type(data, offset, array, registry, idl),
        IdlType::Defined { defined } => parse_defined_type(data, offset, defined, registry, idl),
    }
}

fn parse_simple_type(data: &[u8], offset: &usize, type_name: &str) -> Result<(String, usize)> {
    let start = *offset;

    match type_name {
        "u8" => {
            check_data_len(data, start, 1)?;
            Ok((data[start].to_string(), 1))
        }
        "i8" => {
            check_data_len(data, start, 1)?;
            let value = data[start] as i8;
            Ok((value.to_string(), 1))
        }
        "u16" => {
            check_data_len(data, start, 2)?;
            let value = u16::from_le_bytes([data[start], data[start + 1]]);
            Ok((value.to_string(), 2))
        }
        "i16" => {
            check_data_len(data, start, 2)?;
            let value = i16::from_le_bytes([data[start], data[start + 1]]);
            Ok((value.to_string(), 2))
        }
        "u32" => {
            check_data_len(data, start, 4)?;
            let value = u32::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
            ]);
            Ok((value.to_string(), 4))
        }
        "i32" => {
            check_data_len(data, start, 4)?;
            let value = i32::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
            ]);
            Ok((value.to_string(), 4))
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
            Ok((value.to_string(), 8))
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
            Ok((value.to_string(), 8))
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
            Ok((value.to_string(), 16))
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
            Ok((value.to_string(), 16))
        }
        "pubkey" | "publicKey" => {
            check_data_len(data, start, 32)?;
            let pubkey = Pubkey::try_from(&data[start..start + 32])
                .map_err(|_| anyhow!("Invalid pubkey data"))?;
            Ok((pubkey.to_string(), 32))
        }
        "bool" => {
            check_data_len(data, start, 1)?;
            let value = data[start] != 0;
            Ok((value.to_string(), 1))
        }
        "string" => {
            // String: 4-byte length + data
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
            let value = String::from_utf8_lossy(string_data);
            Ok((value.to_string(), 4 + length))
        }
        _ => {
            // Unknown type
            let hex_len = (data.len() - start).min(32);
            let hex = hex::encode(&data[start..start + hex_len]);
            Ok((format!("<{}: 0x{}>", type_name, hex), hex_len))
        }
    }
}

fn parse_vec_type(
    data: &[u8],
    offset: &usize,
    element_type: &IdlType,
    registry: &IdlRegistry,
    idl: &CompleteIdl,
) -> Result<(String, usize)> {
    let start = *offset;
    check_data_len(data, start, 4)?;

    let length =
        u32::from_le_bytes([data[start], data[start + 1], data[start + 2], data[start + 3]])
            as usize;
    let mut current_offset = start + 4;

    if length == 0 {
        return Ok(("[]".to_string(), current_offset - start));
    }

    let mut elements = Vec::new();
    for _i in 0..length {
        if current_offset >= data.len() {
            break;
        }
        let (element, bytes_read) = parse_type(data, &current_offset, element_type, registry, idl)?;
        elements.push("            ".to_owned() + &element);
        current_offset += bytes_read;
    }

    let display = format!("[\n{}\n          ]", elements.join(",\n"));

    Ok((display, current_offset - start))
}

fn parse_option_type(
    data: &[u8],
    offset: &usize,
    inner_type: &IdlType,
    registry: &IdlRegistry,
    idl: &CompleteIdl,
) -> Result<(String, usize)> {
    let start = *offset;
    check_data_len(data, start, 1)?;

    let is_some = data[start] != 0;
    if !is_some {
        return Ok(("None".to_string(), 1));
    }

    let (value, bytes_read) = parse_type(data, &(start + 1), inner_type, registry, idl)?;
    Ok((format!("Some({})", value), 1 + bytes_read))
}

fn parse_array_type(
    data: &[u8],
    offset: &usize,
    array_def: &[JsonValue; 2],
    registry: &IdlRegistry,
    idl: &CompleteIdl,
) -> Result<(String, usize)> {
    let start = *offset;

    let element_type = array_def[0].clone();
    let length = array_def[1].as_u64().ok_or_else(|| anyhow!("Invalid array length"))? as usize;

    // Convert JSON value to IdlType
    let idl_type = serde_json::from_value(element_type)
        .map_err(|_| anyhow!("Failed to parse array element type"))?;

    let mut elements = Vec::new();
    let mut current_offset = start;

    for _ in 0..length {
        if current_offset >= data.len() {
            break;
        }
        let (element, bytes_read) = parse_type(data, &current_offset, &idl_type, registry, idl)?;
        elements.push(element);
        current_offset += bytes_read;
    }

    let display = format!("[{}]; {}", elements.join(", "), length);

    Ok((display, current_offset - start))
}

fn parse_defined_type(
    data: &[u8],
    offset: &usize,
    defined: &DefinedType,
    registry: &IdlRegistry,
    idl: &CompleteIdl,
) -> Result<(String, usize)> {
    // First check the registry (global types)
    if let Some(type_def) = registry.get_type(&defined.name) {
        return parse_type_definition(data, offset, type_def, registry, idl);
    }

    // Then check the IDL's local types
    if let Some(types) = &idl.types {
        if let Some(type_def) = types.iter().find(|t| t.name == defined.name) {
            return parse_type_definition(data, offset, type_def, registry, idl);
        }
    }

    // Type not found, show hex
    let start = *offset;
    let hex_len = (data.len() - start).min(32);
    let hex = hex::encode(&data[start..start + hex_len]);
    Ok((format!("<{}: 0x{}>", defined.name, hex), hex_len))
}

fn parse_type_definition(
    data: &[u8],
    offset: &usize,
    type_def: &IdlTypeDefinition,
    registry: &IdlRegistry,
    idl: &CompleteIdl,
) -> Result<(String, usize)> {
    let start = *offset;

    match type_def.type_.kind.as_str() {
        "struct" => {
            if let Some(fields) = &type_def.type_.fields {
                let mut current_offset = start;

                return match fields {
                    crate::instruction_parsers::anchor_idl::IdlFields::Named(named_fields) => {
                        // Regular struct with named fields
                        let mut field_values = Vec::new();
                        for field in named_fields {
                            if current_offset >= data.len() {
                                break;
                            }
                            let (value, bytes_read) =
                                parse_type(data, &current_offset, &field.type_, registry, idl)?;
                            field_values.push(format!("{}: {}", field.name, value));
                            current_offset += bytes_read;
                        }

                        Ok((
                            format!("{} {{{}}}", type_def.name, field_values.join(", ")),
                            current_offset - start,
                        ))
                    }
                    crate::instruction_parsers::anchor_idl::IdlFields::Tuple(type_names) => {
                        // Tuple struct like struct Foo(Type1, Type2)
                        let mut field_values = Vec::new();
                        for type_name in type_names {
                            if current_offset >= data.len() {
                                break;
                            }
                            let (value, bytes_read) =
                                parse_simple_type(data, &current_offset, type_name)?;
                            field_values.push(value);
                            current_offset += bytes_read;
                        }

                        Ok((
                            format!("{}({})", type_def.name, field_values.join(", ")),
                            current_offset - start,
                        ))
                    }
                };
            }
        }
        "enum" => {
            if let Some(variants) = &type_def.type_.variants {
                // Borsh enum: 1-byte variant index + variant data
                check_data_len(data, start, 1)?;
                let variant_index = data[start] as usize;

                if variant_index < variants.len() {
                    let variant = &variants[variant_index];
                    let mut current_offset = start + 1;

                    let variant_str = if let Some(fields) = &variant.fields {
                        // Parse the fields JSON dynamically
                        if let Some(fields_array) = fields.as_array() {
                            if fields_array.is_empty() {
                                variant.name.clone()
                            } else {
                                // Determine if it's named fields or tuple fields
                                let first_field = &fields_array[0];
                                if first_field.get("name").is_some() {
                                    // Named fields
                                    let mut field_values = Vec::new();
                                    for field in fields_array {
                                        if let (Some(name), Some(type_value)) =
                                            (field.get("name"), field.get("type"))
                                        {
                                            let type_ = serde_json::from_value(type_value.clone())
                                                .map_err(|_| {
                                                    anyhow!("Failed to parse field type")
                                                })?;
                                            let (value, bytes_read) = parse_type(
                                                data,
                                                &current_offset,
                                                &type_,
                                                registry,
                                                idl,
                                            )?;
                                            field_values.push(format!(
                                                "{}: {}",
                                                name.as_str().unwrap_or(""),
                                                value
                                            ));
                                            current_offset += bytes_read;
                                        }
                                    }
                                    format!("{}::{{{}}}", variant.name, field_values.join(", "))
                                } else {
                                    // Tuple fields
                                    let mut field_values = Vec::new();
                                    for type_value in fields_array {
                                        let type_ = serde_json::from_value(type_value.clone())
                                            .map_err(|_| {
                                                anyhow!("Failed to parse tuple field type")
                                            })?;
                                        let (value, bytes_read) = parse_type(
                                            data,
                                            &current_offset,
                                            &type_,
                                            registry,
                                            idl,
                                        )?;
                                        field_values.push(value);
                                        current_offset += bytes_read;
                                    }
                                    format!("{}({})", variant.name, field_values.join(", "))
                                }
                            }
                        } else {
                            // Not an array, treat as simple variant
                            variant.name.clone()
                        }
                    } else {
                        variant.name.clone()
                    };

                    return Ok((variant_str, current_offset - start));
                }
            }
        }
        _ => {}
    }

    // Fallback
    let hex_len = (data.len() - start).min(32);
    let hex = hex::encode(&data[start..start + hex_len]);
    Ok((format!("<{}: 0x{}>", type_def.name, hex), hex_len))
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

/// Load IDL files from the default directory
pub fn load_idls_from_default_dir() -> Result<IdlRegistry> {
    let idl_dir = PathBuf::from("idl");
    let mut registry = IdlRegistry::new();

    log::debug!(
        "Looking for IDL directory at: {}",
        idl_dir.canonicalize().unwrap_or(idl_dir.clone()).display()
    );

    if idl_dir.exists() && idl_dir.is_dir() {
        log::info!("Loading IDLs from: {}", idl_dir.display());
        registry
            .load_idls(&idl_dir)
            .with_context(|| "Failed to load IDLs from default 'idl/' directory")?;
    } else {
        log::warn!("IDL directory does not exist at: {}", idl_dir.display());
    }

    Ok(registry)
}

/// Find an event by discriminator
pub fn find_event_by_discriminator<'a>(
    idl: &'a CompleteIdl,
    discriminator: &[u8],
) -> Option<&'a IdlEvent> {
    if let Some(events) = &idl.events {
        events.iter().find(|event| {
            event.discriminator.len() == 8 && &event.discriminator[..8] == discriminator
        })
    } else {
        None
    }
}

const EMIT_CPI_DISCRIMINATOR: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];

/// Check if an inner instruction is an Anchor CPI event
pub fn is_anchor_cpi_event(instruction: &crate::transaction::InstructionSummary) -> bool {
    if instruction.data.len() >= 16 {
        // First 8 bytes: emit_cpi discriminator
        // Next 8 bytes: event discriminator
        &instruction.data[..8] == EMIT_CPI_DISCRIMINATOR
    } else {
        false
    }
}

/// Parse an Anchor CPI event from instruction data
pub fn parse_anchor_cpi_event(
    instruction: &crate::transaction::InstructionSummary,
    idl_registry: &IdlRegistry,
    program_id: &Pubkey,
) -> Result<Option<ParsedInstruction>> {
    if instruction.data.len() < 16 {
        return Ok(None);
    }

    // Check if this is an emit_cpi instruction
    if &instruction.data[..8] != EMIT_CPI_DISCRIMINATOR {
        return Ok(None);
    }

    // Get the event discriminator (bytes 8-15)
    let event_discriminator = &instruction.data[8..16];

    // Look up the IDL for this program
    let Some(idl) = idl_registry.get(program_id) else {
        return Ok(None);
    };

    // Find the event by discriminator
    let Some(event_def) = find_event_by_discriminator(idl, event_discriminator) else {
        return Ok(None);
    };

    // Find the corresponding type definition for the event data
    // First check this IDL's local types
    let type_def = if let Some(types) = &idl.types {
        types.iter().find(|t| t.name == event_def.name)
    } else {
        None
    };

    let Some(type_def) = type_def.or_else(|| idl_registry.get_type(&event_def.name)) else {
        // Fallback: if no type definition, show raw data
        let mut fields = Vec::new();
        if instruction.data.len() > 16 {
            let raw_data = &instruction.data[16..];
            let raw_hex = hex::encode(raw_data).to_uppercase();
            let preview =
                if raw_hex.len() > 32 { format!("{}...", &raw_hex[..32]) } else { raw_hex };
            fields.push(("raw_data".to_string(), format!("0x{}", preview)));
        }
        return Ok(Some(ParsedInstruction {
            name: event_def.name.clone(),
            fields,
            account_names: vec![],
        }));
    };

    // Parse the event data
    let mut offset = 16; // Skip discriminators
    let mut fields = Vec::new();

    match &type_def.type_.fields {
        Some(crate::instruction_parsers::anchor_idl::IdlFields::Named(named_fields)) => {
            // Regular struct with named fields
            for field in named_fields {
                if offset >= instruction.data.len() {
                    break;
                }

                let (value, bytes_read) =
                    parse_type(&instruction.data, &mut offset, &field.type_, idl_registry, idl)?;
                fields.push((field.name.clone(), value));
                offset += bytes_read;
            }
        }
        Some(crate::instruction_parsers::anchor_idl::IdlFields::Tuple(type_names)) => {
            // Tuple struct like struct Foo(Type1, Type2)
            for (idx, type_name) in type_names.iter().enumerate() {
                if offset >= instruction.data.len() {
                    break;
                }

                let (value, bytes_read) = parse_simple_type(&instruction.data, &offset, type_name)?;
                fields.push((format!("field_{}", idx), value));
                offset += bytes_read;
            }
        }
        None => {
            // No fields defined
            if offset < instruction.data.len() {
                let raw_data = &instruction.data[offset..];
                let raw_hex = hex::encode(raw_data).to_uppercase();
                let preview =
                    if raw_hex.len() > 32 { format!("{}...", &raw_hex[..32]) } else { raw_hex };
                fields.push(("raw_data".to_string(), format!("0x{}", preview)));
            }
        }
    }

    Ok(Some(ParsedInstruction { name: event_def.name.clone(), fields, account_names: vec![] }))
}

/// Create parsers from an IDL registry
pub fn create_parsers_from_idl_registry(registry: &IdlRegistry) -> Vec<Box<dyn InstructionParser>> {
    let mut parsers: Vec<Box<dyn InstructionParser>> = Vec::new();

    for program_id in registry.program_ids() {
        if let Some(idl) = registry.get(&program_id) {
            let parser = AnchorIdlParser::new(program_id, idl.clone(), registry.clone());
            parsers.push(Box::new(parser) as Box<dyn InstructionParser>);
        }
    }

    parsers
}
