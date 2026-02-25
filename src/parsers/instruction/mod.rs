use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Serialize, Serializer, ser::SerializeMap, ser::SerializeSeq};
use serde_json::Number as JsonNumber;
use solana_account::ReadableAccount;
use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;

use crate::core::account_loader::ResolvedAccounts;
use crate::core::transaction::InstructionSummary;

/// Represents a parsed instruction with human-readable data and account names
#[derive(Debug, Clone, Serialize)]
pub struct ParsedInstruction {
    /// The instruction name (e.g., "Transfer", "CreateAccount")
    pub name: String,
    /// Vector of parsed fields preserving order
    pub fields: Vec<ParsedField>,
    /// Human-readable names for each account in the instruction
    pub account_names: Vec<String>,
}

/// Ordered parsed field entry
#[derive(Debug, Clone, Serialize)]
pub struct ParsedField {
    pub name: String,
    pub value: ParsedFieldValue,
}

impl ParsedField {
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self { name: name.into(), value: ParsedFieldValue::Text(value.into()) }
    }

    pub fn json(name: impl Into<String>, value: OrderedJsonValue) -> Self {
        Self { name: name.into(), value: ParsedFieldValue::Json(value) }
    }
}

impl<N, V> From<(N, V)> for ParsedField
where
    N: Into<String>,
    V: Into<String>,
{
    fn from((name, value): (N, V)) -> Self {
        ParsedField::text(name, value)
    }
}

/// Parsed field value, either plain text or structured JSON
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(untagged)]
pub enum ParsedFieldValue {
    Text(String),
    Json(OrderedJsonValue),
}

impl From<String> for ParsedFieldValue {
    fn from(value: String) -> Self {
        ParsedFieldValue::Text(value)
    }
}

impl From<&str> for ParsedFieldValue {
    fn from(value: &str) -> Self {
        ParsedFieldValue::Text(value.to_string())
    }
}

impl PartialEq<&str> for ParsedFieldValue {
    fn eq(&self, other: &&str) -> bool {
        match self {
            ParsedFieldValue::Text(text) => text == *other,
            ParsedFieldValue::Json(_) => false,
        }
    }
}

impl PartialEq<String> for ParsedFieldValue {
    fn eq(&self, other: &String) -> bool {
        match self {
            ParsedFieldValue::Text(text) => text == other,
            ParsedFieldValue::Json(_) => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrderedJsonValue {
    Null,
    Bool(bool),
    Number(JsonNumber),
    String(String),
    Array(Vec<OrderedJsonValue>),
    Object(Vec<(String, OrderedJsonValue)>),
}

impl Serialize for OrderedJsonValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            OrderedJsonValue::Null => serializer.serialize_unit(),
            OrderedJsonValue::Bool(value) => serializer.serialize_bool(*value),
            OrderedJsonValue::Number(num) => {
                if let Some(value) = num.as_i64() {
                    serializer.serialize_i64(value)
                } else if let Some(value) = num.as_u64() {
                    serializer.serialize_u64(value)
                } else if let Some(value) = num.as_f64() {
                    serializer.serialize_f64(value)
                } else {
                    serializer.serialize_str(&num.to_string())
                }
            }
            OrderedJsonValue::String(value) => serializer.serialize_str(value),
            OrderedJsonValue::Array(values) => {
                let mut seq = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    seq.serialize_element(value)?;
                }
                seq.end()
            }
            OrderedJsonValue::Object(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

/// Trait for parsing instructions of specific Solana programs
pub trait InstructionParser: Send + Sync {
    /// Returns the program ID this parser handles
    fn program_id(&self) -> &Pubkey;

    /// Attempts to parse the given instruction
    /// Returns Ok(Some(parsed)) if this parser can handle the instruction
    /// Returns Ok(None) if the instruction is not recognized by this parser
    /// Returns Err if parsing fails due to invalid data
    fn parse_instruction(
        &self,
        instruction: &InstructionSummary,
    ) -> Result<Option<ParsedInstruction>>;

    /// Attempt to parse a CPI event if this parser supports it
    fn parse_cpi_event(
        &self,
        _instruction: &InstructionSummary,
        _program_id: &Pubkey,
    ) -> Result<Option<ParsedInstruction>> {
        Ok(None) // Default: not supported
    }
}

/// Registry of instruction parsers for well-known programs
pub struct ParserRegistry {
    parsers: HashMap<Pubkey, Box<dyn InstructionParser>>,
    /// Optional IDL directory path for lazy loading IDL parsers
    idl_directory: Option<std::path::PathBuf>,
}

impl ParserRegistry {
    /// Creates a new parser registry with default parsers
    pub fn new(idl_directory: Option<std::path::PathBuf>) -> Self {
        let mut registry = Self {
            parsers: HashMap::new(),
            idl_directory: idl_directory.or_else(default_idl_cache_dir),
        };

        // Register default parsers
        let system_parser = SystemProgramParser::new();
        registry.parsers.insert(*system_parser.program_id(), Box::new(system_parser));

        // Register Token2022 parser
        let token2022_parser = Token2022ProgramParser::new();
        registry.parsers.insert(*token2022_parser.program_id(), Box::new(token2022_parser));

        // Register SPL Token parser by reusing the Token2022 parser implementation
        let spl_token_parser = Token2022ProgramParser::with_program_id(Pubkey::from_str_const(
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        ));
        registry.parsers.insert(*spl_token_parser.program_id(), Box::new(spl_token_parser));

        // Register Compute Budget parser
        let compute_budget_parser = ComputeBudgetProgramParser::new();
        registry
            .parsers
            .insert(*compute_budget_parser.program_id(), Box::new(compute_budget_parser));

        // Register Associated Token parser
        let associated_token_parser = AssociatedTokenProgramParser::new();
        registry
            .parsers
            .insert(*associated_token_parser.program_id(), Box::new(associated_token_parser));

        // Register Memo parser
        let memo_parser = MemoProgramParser::new();
        registry.parsers.insert(*memo_parser.program_id(), Box::new(memo_parser));

        registry
    }

    /// Returns the configured IDL directory used for lazy loading.
    pub fn idl_directory(&self) -> Option<&std::path::Path> {
        self.idl_directory.as_deref()
    }

    /// Returns program IDs that can be fetched from chain for IDL auto-download.
    pub fn find_fetchable_programs(
        &self,
        program_ids: &[Pubkey],
        resolved_accounts: &ResolvedAccounts,
    ) -> Vec<Pubkey> {
        let Some(idl_dir) = self.idl_directory() else {
            return Vec::new();
        };

        program_ids
            .iter()
            .filter(|program_id| {
                !self.parsers.contains_key(program_id)
                    && !idl_dir.join(format!("{}.json", program_id)).exists()
                    && resolved_accounts
                        .accounts
                        .get(program_id)
                        .is_some_and(|account| *account.owner() == bpf_loader_upgradeable::id())
            })
            .copied()
            .collect()
    }

    /// Attempts to parse an instruction using the parser registered for the given program ID
    /// Returns the parsed instruction if successful, None otherwise
    pub fn parse_instruction(
        &mut self,
        instruction: &InstructionSummary,
        program_id: &Pubkey,
    ) -> Option<ParsedInstruction> {
        // Try to load IDL parser if we don't have one registered
        if !self.parsers.contains_key(program_id) {
            if let Err(err) = self.load_idl_parser_if_needed(program_id) {
                log::debug!("Failed to load IDL parser for {}: {}", program_id, err);
            }
        }

        if let Some(parser) = self.parsers.get(program_id) {
            match parser.parse_instruction(instruction) {
                Ok(parsed) => parsed,
                Err(err) => {
                    log::warn!("Instruction parsing failed: {}", err);
                    None
                }
            }
        } else {
            None
        }
    }
}

impl Default for ParserRegistry {
    fn default() -> Self {
        Self::new(None)
    }
}

fn default_idl_cache_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok()?;
    Some(std::path::PathBuf::from(home).join(".sonar").join("idls"))
}

impl ParserRegistry {
    /// Load IDL parser for a specific program ID if needed
    pub fn load_idl_parser_if_needed(&mut self, program_id: &Pubkey) -> Result<bool> {
        // If parser already exists, return early
        if self.parsers.contains_key(program_id) {
            return Ok(false);
        }

        // If no IDL directory is configured, return early
        let idl_dir = match &self.idl_directory {
            Some(dir) => dir,
            None => return Ok(false),
        };

        // Look for IDL file matching the program ID
        let idl_file_path = idl_dir.join(format!("{}.json", program_id));
        if !idl_file_path.exists() {
            return Ok(false);
        }

        log::info!("Lazy-loading IDL for program: {}", program_id);

        // Load the specific IDL file
        let idl_content = std::fs::read_to_string(&idl_file_path)
            .with_context(|| format!("Failed to read IDL file: {}", idl_file_path.display()))?;

        // Parse as RawAnchorIdl to support both legacy and new formats
        let raw_idl: crate::parsers::instruction::anchor_idl::RawAnchorIdl =
            match serde_json::from_str(&idl_content) {
                Ok(idl) => idl,
                Err(e) => {
                    // Try to debug why it failed by trying to parse as LegacyIdl directly
                    if let Err(legacy_err) = serde_json::from_str::<
                        crate::parsers::instruction::anchor_idl::LegacyIdl,
                    >(&idl_content)
                    {
                        log::warn!("Failed to parse as LegacyIdl: {}", legacy_err);
                    }
                    return Err(anyhow::anyhow!(
                        "Failed to parse IDL JSON: {} - {}",
                        idl_file_path.display(),
                        e
                    ));
                }
            };

        // Convert to canonical Idl
        let idl_data = raw_idl.convert(&program_id.to_string());

        let parser = Box::new(crate::parsers::instruction::anchor_idl::AnchorIdlParser::new(
            *program_id,
            idl_data.clone(),
            crate::parsers::instruction::anchor_idl::IdlRegistry::with_idl(*program_id, &idl_data),
        ));

        self.parsers.insert(*program_id, parser);
        Ok(true)
    }

    /// Load IDL parsers for all program IDs used in a transaction
    pub fn load_idl_parsers_for_programs(&mut self, program_ids: Vec<Pubkey>) -> Result<usize> {
        let mut loaded_count = 0;
        for program_id in program_ids {
            if self.load_idl_parser_if_needed(&program_id)? {
                loaded_count += 1;
            }
        }
        Ok(loaded_count)
    }

    /// Try to parse an Anchor CPI event, loading IDL if needed
    pub fn parse_cpi_event(
        &mut self,
        instruction: &crate::core::transaction::InstructionSummary,
        program_id: &Pubkey,
        _message: &solana_message::VersionedMessage,
        _account_plan: &crate::core::transaction::MessageAccountPlan,
        _lookup_locations: &[crate::core::transaction::LookupLocation],
    ) -> Option<ParsedInstruction> {
        use crate::parsers::instruction::anchor_idl;

        // Check if this is a CPI event
        if !anchor_idl::is_anchor_cpi_event(instruction) {
            log::debug!("Not a CPI event for program: {}", program_id);
            return None;
        }

        log::debug!("Detected CPI event for program: {}", program_id);

        // Try to load IDL if we haven't already (even though we loaded it earlier, double check)
        if !self.parsers.contains_key(program_id) {
            log::debug!("Loading IDL for CPI event parsing for: {}", program_id);
            if let Err(err) = self.load_idl_parser_if_needed(program_id) {
                log::warn!("Failed to load IDL for CPI event parsing: {:?}", err);
                return None;
            }
        }

        // If we have a parser, try to parse the CPI event using the trait method
        if let Some(parser) = self.parsers.get(program_id) {
            match parser.parse_cpi_event(instruction, program_id) {
                Ok(Some(parsed)) => {
                    log::debug!("Successfully parsed CPI event: {}", parsed.name);
                    return Some(parsed);
                }
                Ok(None) => {
                    log::debug!("parse_cpi_event returned None");
                }
                Err(e) => {
                    log::debug!("parse_cpi_event returned error: {:?}", e);
                }
            }
        } else {
            log::debug!("No parser found for program: {}", program_id);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::account_loader::ResolvedAccounts;
    use solana_account::{Account, AccountSharedData};
    use solana_pubkey::Pubkey;
    use solana_sdk_ids::{bpf_loader_upgradeable, system_program};
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn test_token2022_parser_registration() {
        let registry = ParserRegistry::new(None);
        let token2022_id = Pubkey::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

        // The Token2022 parser should be registered
        assert!(registry.parsers.contains_key(&token2022_id));
    }

    #[test]
    fn test_spl_token_parser_registration() {
        let registry = ParserRegistry::new(None);
        let tokenkeg_id = Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

        assert!(registry.parsers.contains_key(&tokenkeg_id));
    }

    #[test]
    fn test_compute_budget_parser_registration() {
        let registry = ParserRegistry::new(None);
        let compute_budget_id =
            Pubkey::from_str_const("ComputeBudget111111111111111111111111111111");

        assert!(registry.parsers.contains_key(&compute_budget_id));
    }

    #[test]
    fn test_associated_token_parser_registration() {
        let registry = ParserRegistry::new(None);
        let associated_token_id =
            Pubkey::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

        assert!(registry.parsers.contains_key(&associated_token_id));
    }

    #[test]
    fn test_memo_parser_registration() {
        let registry = ParserRegistry::new(None);
        let memo_id = Pubkey::from_str_const("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");

        assert!(registry.parsers.contains_key(&memo_id));
    }

    #[test]
    fn test_token2022_parser_program_id() {
        let parser = Token2022ProgramParser::new();
        let expected_id = Pubkey::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

        assert_eq!(*parser.program_id(), expected_id);
    }

    #[test]
    fn test_default_idl_cache_dir_uses_home() {
        let registry = ParserRegistry::new(None);
        let expected = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .map(|home| PathBuf::from(home).join(".sonar").join("idls"));
        assert_eq!(registry.idl_directory(), expected.as_deref());
    }

    #[test]
    fn test_find_fetchable_programs_only_returns_upgradeable_missing_idls() {
        let test_dir = std::env::temp_dir().join(format!(
            "sonar-idl-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be valid")
                .as_nanos()
        ));
        std::fs::create_dir_all(&test_dir).expect("create temp idl dir");

        let with_local_idl = Pubkey::new_unique();
        let fetchable = Pubkey::new_unique();
        let not_upgradeable = Pubkey::new_unique();
        let existing_path = test_dir.join(format!("{}.json", with_local_idl));
        std::fs::write(&existing_path, "{}").expect("create existing idl file");

        let mut accounts = HashMap::new();
        accounts.insert(
            with_local_idl,
            AccountSharedData::from(Account {
                lamports: 0,
                data: Vec::new(),
                owner: bpf_loader_upgradeable::id(),
                executable: true,
                rent_epoch: 0,
            }),
        );
        accounts.insert(
            fetchable,
            AccountSharedData::from(Account {
                lamports: 0,
                data: Vec::new(),
                owner: bpf_loader_upgradeable::id(),
                executable: true,
                rent_epoch: 0,
            }),
        );
        accounts.insert(
            not_upgradeable,
            AccountSharedData::from(Account {
                lamports: 0,
                data: Vec::new(),
                owner: system_program::id(),
                executable: true,
                rent_epoch: 0,
            }),
        );
        let resolved_accounts = ResolvedAccounts { accounts, lookups: vec![] };

        let registry = ParserRegistry::new(Some(test_dir.clone()));
        let candidates = vec![with_local_idl, fetchable, not_upgradeable];

        let fetchable_programs = registry.find_fetchable_programs(&candidates, &resolved_accounts);
        assert_eq!(fetchable_programs, vec![fetchable]);

        std::fs::remove_file(existing_path).ok();
        std::fs::remove_dir_all(test_dir).ok();
    }
}

mod system_program;
pub use system_program::SystemProgramParser;

mod token2022_program;
pub use token2022_program::Token2022ProgramParser;

mod compute_budget_program;
pub use compute_budget_program::ComputeBudgetProgramParser;

mod associated_token_program;
pub use associated_token_program::AssociatedTokenProgramParser;

mod memo_program;
pub use memo_program::MemoProgramParser;

pub mod anchor_idl;
