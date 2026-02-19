use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Number as JsonNumber, Value as JsonValue};
use sha2::{Digest, Sha256};
use solana_pubkey::Pubkey;

use crate::parsers::instruction::{
    InstructionParser, OrderedJsonValue, ParsedField, ParsedInstruction,
};
use crate::core::transaction::InstructionSummary;

/// Helper to calculate sighash for discriminators
fn sighash(namespace: &str, name: &str) -> [u8; 8] {
    let preimage = format!("{}:{}", namespace, name);
    let mut hasher = Sha256::new();
    hasher.update(preimage.as_bytes());
    let result = hasher.finalize();
    let mut sighash = [0u8; 8];
    sighash.copy_from_slice(&result[..8]);
    sighash
}

/// Convert camelCase to snake_case for sighash calculation
fn to_snake_case(s: &str) -> String {
    let mut snake = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                snake.push('_');
            }
            snake.push(c.to_ascii_lowercase());
        } else {
            snake.push(c);
        }
    }
    snake
}

/// Enum to handle both legacy (flat) and new (nested metadata) IDL formats
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RawAnchorIdl {
    Current(Idl),
    Legacy(LegacyIdl),
}

impl RawAnchorIdl {
    pub fn convert(self, program_address: &str) -> Idl {
        match self {
            RawAnchorIdl::Current(idl) => idl,
            RawAnchorIdl::Legacy(legacy) => legacy.into_idl(program_address),
        }
    }
}

/// Legacy IDL structure (pre-0.30)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyIdl {
    pub version: String,
    pub name: String,
    pub instructions: Vec<IdlInstruction>,
    #[serde(default)]
    pub accounts: Option<Vec<IdlTypeDefinition>>,
    #[serde(default)]
    pub types: Option<Vec<IdlTypeDefinition>>,
    #[serde(default)]
    pub events: Option<Vec<IdlEvent>>,
    #[serde(default)]
    pub errors: Option<Vec<JsonValue>>,
    #[serde(default)]
    pub metadata: Option<IdlMetadata>,
}

impl LegacyIdl {
    fn into_idl(self, address: &str) -> Idl {
        // 1. Merge 'accounts' into 'types'
        let mut types = self.types.unwrap_or_default();
        if let Some(accounts) = self.accounts {
            types.extend(accounts);
        }

        // 2. Ensure instructions have discriminators
        let instructions: Vec<IdlInstruction> = self
            .instructions
            .into_iter()
            .map(|mut inst| {
                if inst.discriminator.is_none() {
                    // Anchor discriminators for instructions are based on the snake_case Rust function name
                    // IDL usually has camelCase names, so we convert them
                    let snake_name = to_snake_case(&inst.name);
                    inst.discriminator = Some(sighash("global", &snake_name).to_vec());
                }
                inst
            })
            .collect();

        // 3. Handle events - calculate discriminators if missing and merge fields from types if needed
        let events: Option<Vec<IdlEvent>> = self.events.map(|events| {
            events
                .into_iter()
                .map(|mut event| {
                    if event.discriminator.is_none() {
                        event.discriminator = Some(sighash("event", &event.name).to_vec());
                    }
                    event
                })
                .collect()
        });

        // 4. Construct metadata
        let metadata = self.metadata.unwrap_or_else(|| IdlMetadata {
            name: self.name.clone(),
            version: self.version.clone(),
            spec: "0.1.0".to_string(), // Default for legacy
            description: None,
        });

        Idl {
            address: address.to_string(),
            metadata,
            instructions,
            types: if types.is_empty() { None } else { Some(types) },
            events,
        }
    }
}

/// Complete IDL structure including types for full type resolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Idl {
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
    #[serde(default)]
    pub discriminator: Option<Vec<u8>>,
    pub accounts: Vec<IdlAccountItem>,
    pub args: Vec<IdlArg>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdlEvent {
    pub name: String,
    #[serde(default)]
    pub discriminator: Option<Vec<u8>>,
    #[serde(default)]
    pub fields: Option<Vec<IdlField>>,
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
    #[serde(default, alias = "isMut")]
    pub writable: bool,
    #[serde(default, alias = "isSigner")]
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
#[serde(untagged)]
pub enum DefinedType {
    Simple(String),
    Complex {
        name: String,
        #[serde(default)]
        generics: Option<Vec<JsonValue>>,
    },
}

impl DefinedType {
    pub fn name(&self) -> &str {
        match self {
            DefinedType::Simple(name) => name,
            DefinedType::Complex { name, .. } => name,
        }
    }
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

/// Custom deserializer for optional IDL fields that handles missing fields gracefully
fn deserialize_optional_idl_fields<'de, D>(deserializer: D) -> Result<Option<IdlFields>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

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
    pub(crate) idls: HashMap<Pubkey, Idl>,
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

    /// Get an IDL by program ID
    pub fn get(&self, program_id: &Pubkey) -> Option<&Idl> {
        self.inner.idls.get(program_id)
    }

    /// Get a type definition by program ID and name
    pub fn get_type_by_program(
        &self,
        program_id: &Pubkey,
        name: &str,
    ) -> Option<&IdlTypeDefinition> {
        self.inner.types_by_program_and_name.get(&(*program_id, name.to_string()))
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
    pub(crate) idl: Idl,
    pub(crate) registry: IdlRegistry,
}

impl AnchorIdlParser {
    pub fn new(program_id: Pubkey, idl: Idl, registry: IdlRegistry) -> Self {
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
        if let Some(idl_instruction) =
            find_instruction_by_discriminator(&self.idl, &instruction.data)
        {
            let mut offset = idl_instruction.discriminator.as_ref().map_or(0, |d| d.len());
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

fn find_instruction_by_discriminator<'a>(idl: &'a Idl, data: &[u8]) -> Option<&'a IdlInstruction> {
    // Find all matching instructions, then pick the one with the longest discriminator.
    // This ensures that a more specific match (e.g. [1]) wins over an empty discriminator ([]).
    idl.instructions
        .iter()
        .filter(|inst| {
            let disc = inst.discriminator.as_deref().unwrap_or(&[]);
            let disc_len = disc.len();
            data.len() >= disc_len && &data[..disc_len] == disc
        })
        .max_by_key(|inst| inst.discriminator.as_ref().map_or(0, |d| d.len()))
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
    idl: &Idl,
) -> Result<Vec<ParsedField>> {
    let mut fields = Vec::new();

    for arg in args {
        if *offset >= data.len() {
            break;
        }

        let value = parse_type(data, offset, &arg.type_, registry, idl)?;
        fields.push(ParsedField::json(arg.name.clone(), value));
    }

    Ok(fields)
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
            (OrderedJsonValue::Number(JsonNumber::from(data[start] as i64)), 1)
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
            let hex_len = (data.len() - start).min(32);
            let hex = hex::encode(&data[start..start + hex_len]);
            (OrderedJsonValue::String(format!("<{}: 0x{}>", type_name, hex)), hex_len)
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
    let hex_len = (data.len() - start).min(32);
    let hex = hex::encode(&data[start..start + hex_len]);
    *offset += hex_len;
    Ok(OrderedJsonValue::String(format!("<{}: 0x{}>", defined.name(), hex)))
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
                    crate::parsers::instruction::anchor_idl::IdlFields::Named(named_fields) => {
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
                    crate::parsers::instruction::anchor_idl::IdlFields::Tuple(type_names) => {
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
    let hex_len = (data.len() - start).min(32);
    let hex = hex::encode(&data[start..start + hex_len]);
    *offset += hex_len;
    Ok(OrderedJsonValue::String(format!("<{}: 0x{}>", type_def.name, hex)))
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

/// Find an event by discriminator
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

const EMIT_CPI_DISCRIMINATOR: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];

/// Check if an inner instruction is an Anchor CPI event
pub fn is_anchor_cpi_event(instruction: &crate::core::transaction::InstructionSummary) -> bool {
    if instruction.data.len() >= 16 {
        // First 8 bytes: emit_cpi discriminator
        // Next 8 bytes: event discriminator
        instruction.data[..8] == EMIT_CPI_DISCRIMINATOR
    } else {
        false
    }
}

/// Parse an Anchor CPI event from instruction data
pub fn parse_anchor_cpi_event(
    instruction: &crate::core::transaction::InstructionSummary,
    idl_registry: &IdlRegistry,
    program_id: &Pubkey,
) -> Result<Option<ParsedInstruction>> {
    if instruction.data.len() < 16 {
        return Ok(None);
    }

    // Check if this is an emit_cpi instruction
    if instruction.data[..8] != EMIT_CPI_DISCRIMINATOR {
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
    // First, check if the event itself has fields (legacy IDL converted)
    let type_fields = if let Some(fields) = &event_def.fields {
        Some(IdlFields::Named(fields.clone()))
    } else {
        // Fallback: Check this IDL's local types
        if let Some(types) = &idl.types {
            types.iter().find(|t| t.name == event_def.name).and_then(|t| t.type_.fields.clone())
        } else {
            // Fallback: Check registry types
            idl_registry
                .get_type_by_program(program_id, &event_def.name)
                .and_then(|t| t.type_.fields.clone())
        }
    };

    // Parse the event data
    let mut offset = 16; // Skip discriminators
    let mut fields = Vec::new();

    match type_fields {
        Some(crate::parsers::instruction::anchor_idl::IdlFields::Named(named_fields)) => {
            // Regular struct with named fields
            for field in named_fields {
                if offset >= instruction.data.len() {
                    break;
                }

                let value =
                    parse_type(&instruction.data, &mut offset, &field.type_, idl_registry, idl)?;
                fields.push(ParsedField::json(field.name.clone(), value));
            }
        }
        Some(crate::parsers::instruction::anchor_idl::IdlFields::Tuple(type_names)) => {
            // Tuple struct like struct Foo(Type1, Type2)
            for (idx, type_name) in type_names.iter().enumerate() {
                if offset >= instruction.data.len() {
                    break;
                }

                let value = parse_simple_type(&instruction.data, &mut offset, type_name)?;
                fields.push(ParsedField::json(format!("field_{}", idx), value));
            }
        }
        None => {
            // No fields defined or found
            if offset < instruction.data.len() {
                let raw_data = &instruction.data[offset..];
                let raw_hex = hex::encode(raw_data).to_uppercase();
                let preview =
                    if raw_hex.len() > 32 { format!("{}...", &raw_hex[..32]) } else { raw_hex };
                fields.push(ParsedField::text("raw_data", format!("0x{}", preview)));
            }
        }
    }

    Ok(Some(ParsedInstruction { name: event_def.name.clone(), fields, account_names: vec![] }))
}

/// Find an account type by matching its discriminator against the IDL types.
/// Returns the type definition if found.
fn find_account_type_by_discriminator<'a>(
    idl: &'a Idl,
    discriminator: &[u8],
) -> Option<&'a IdlTypeDefinition> {
    let types = idl.types.as_ref()?;

    types.iter().find(|type_def| {
        // Only match struct types (accounts are structs)
        if type_def.type_.kind != "struct" {
            return false;
        }

        // Anchor account discriminators use the PascalCase name (original type name)
        let expected_discriminator = sighash("account", &type_def.name);
        discriminator == expected_discriminator
    })
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
    // Account data must have at least 8 bytes for the discriminator
    if account_data.len() < 8 {
        return Err(anyhow!(
            "Account data too short: {} bytes (expected at least 8 for discriminator)",
            account_data.len()
        ));
    }

    let discriminator = &account_data[..8];

    // Find the matching account type by discriminator
    let Some(type_def) = find_account_type_by_discriminator(idl, discriminator) else {
        return Ok(None);
    };

    // Parse the account data after the discriminator
    let mut offset = 8;
    let parsed_value = parse_type_definition(account_data, &mut offset, type_def, registry, idl)?;

    Ok(Some((type_def.name.clone(), parsed_value)))
}
