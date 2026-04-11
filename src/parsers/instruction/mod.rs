use std::collections::HashMap;
use std::ops::Deref;

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;
use solana_account::ReadableAccount;
use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;

use crate::core::transaction::InstructionSummary;
use sonar_sim::internals::ResolvedAccounts;

/// Represents a parsed instruction with human-readable data and account names
#[derive(Debug, Clone, Serialize)]
pub struct ParsedInstruction {
    /// The instruction name (e.g., "Transfer", "CreateAccount")
    pub name: String,
    /// Parsed field state preserving either structured fields or raw hex fallback
    pub fields: ParsedInstructionFields,
    /// Human-readable names for each account in the instruction
    pub account_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum ParsedInstructionFields {
    Parsed(Vec<ParsedField>),
    RawHex(String),
}

impl ParsedInstructionFields {
    /// Returns the parsed fields if decoding succeeded, or `None` for the raw hex variant.
    ///
    /// Prefer this over `Deref` when you need to distinguish "no fields" from "failed to decode".
    pub fn parsed_fields(&self) -> Option<&[ParsedField]> {
        match self {
            Self::Parsed(fields) => Some(fields),
            Self::RawHex(_) => None,
        }
    }
}

impl From<Vec<ParsedField>> for ParsedInstructionFields {
    fn from(fields: Vec<ParsedField>) -> Self {
        Self::Parsed(fields)
    }
}

/// **Caution:** Returns an empty slice for `RawHex`, which is indistinguishable from a
/// zero-field instruction. Use [`ParsedInstructionFields::parsed_fields`] when you need to
/// differentiate between "no fields" and "failed to decode".
impl Deref for ParsedInstructionFields {
    type Target = [ParsedField];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Parsed(fields) => fields.as_slice(),
            Self::RawHex(_) => &[],
        }
    }
}

impl<'a> IntoIterator for &'a ParsedInstructionFields {
    type Item = &'a ParsedField;
    type IntoIter = std::slice::Iter<'a, ParsedField>;

    fn into_iter(self) -> Self::IntoIter {
        self.deref().iter()
    }
}

impl Serialize for ParsedInstructionFields {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Parsed(fields) => fields.serialize(serializer),
            Self::RawHex(raw_hex) => serializer.serialize_str(raw_hex),
        }
    }
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

    pub fn json(name: impl Into<String>, value: Value) -> Self {
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
    Json(Value),
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

/// Append numbered account names for accounts beyond the named set.
///
/// For example, if an instruction has 5 accounts but only 3 are named,
/// this appends `{prefix}1` and `{prefix}2` for the remaining accounts.
pub(crate) fn append_extra_account_names(
    account_names: &mut Vec<String>,
    total_accounts: usize,
    named_accounts: usize,
    prefix: &str,
) {
    for i in 0..(total_accounts.saturating_sub(named_accounts)) {
        account_names.push(format!("{}{}", prefix, i + 1));
    }
}

/// Generate parser struct boilerplate: struct definition, `new()`, and `Default`.
///
/// Usage: `define_parser!(MyProgramParser, "ProgramId1111...");`
/// Then implement `InstructionParser` manually (the struct has a `program_id` field).
macro_rules! define_parser {
    ($name:ident, $program_id:expr) => {
        pub struct $name {
            program_id: Pubkey,
        }

        impl $name {
            pub fn new() -> Self {
                Self { program_id: Pubkey::from_str_const($program_id) }
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
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

        registry.register(SystemProgramParser::new());
        registry.register(Token2022ProgramParser::new());
        registry.register(Token2022ProgramParser::with_program_id(Pubkey::from_str_const(
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        )));
        registry.register(ComputeBudgetProgramParser::new());
        registry.register(AssociatedTokenProgramParser::new());
        registry.register(MemoProgramParser::new());

        registry
    }

    /// Register a parser, keyed by its program ID.
    fn register(&mut self, parser: impl InstructionParser + 'static) {
        self.parsers.insert(*parser.program_id(), Box::new(parser));
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

        let program_address = program_id.to_string();
        let indexed_idl =
            crate::parsers::instruction::anchor_idl::IndexedIdl::from_json_with_program_address(
                &idl_content,
                &program_address,
            )
            .with_context(|| format!("Failed to parse IDL JSON: {}", idl_file_path.display()))?;

        let parser = Box::new(crate::parsers::instruction::anchor_idl::AnchorIdlParser::new(
            *program_id,
            indexed_idl,
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
        // Check if this is a CPI event
        if !self::anchor_idl::is_anchor_cpi_event(instruction) {
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
    use solana_account::{Account, AccountSharedData};
    use solana_pubkey::Pubkey;
    use solana_sdk_ids::{bpf_loader_upgradeable, system_program};
    use sonar_sim::internals::ResolvedAccounts;
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

    #[test]
    fn parser_registry_marks_truncated_idl_args_as_raw_hex() {
        let test_dir = std::env::temp_dir().join(format!(
            "sonar-idl-unparsed-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be valid")
                .as_nanos()
        ));
        std::fs::create_dir_all(&test_dir).expect("create temp idl dir");

        let program_id = Pubkey::new_unique();
        let idl_path = test_dir.join(format!("{program_id}.json"));
        std::fs::write(
            &idl_path,
            format!(
                r#"{{
                    "address": "{program_id}",
                    "metadata": {{ "name": "demo", "version": "0.1.0", "spec": "0.1.0" }},
                    "instructions": [{{
                        "name": "swap_toc",
                        "discriminator": [187, 201, 212, 51, 16, 155, 236, 60],
                        "accounts": [{{ "name": "payer", "writable": true, "signer": true }}],
                        "args": [
                            {{ "name": "amount_in", "type": "u64" }},
                            {{ "name": "slippage_bps", "type": "u16" }}
                        ]
                    }}],
                    "types": []
                }}"#
            ),
        )
        .expect("write temp idl");

        let instruction = crate::core::transaction::InstructionSummary {
            index: 7,
            program: crate::core::transaction::AccountReferenceSummary {
                index: 0,
                pubkey: Some(program_id.to_string()),
                signer: false,
                writable: false,
                source: crate::core::transaction::AccountSourceSummary::Static,
            },
            accounts: vec![crate::core::transaction::AccountReferenceSummary {
                index: 1,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: true,
                writable: true,
                source: crate::core::transaction::AccountSourceSummary::Static,
            }],
            data: vec![187, 201, 212, 51, 16, 155, 236, 60, 0, 37, 161, 102, 8, 202, 60, 0]
                .into_boxed_slice(),
        };

        let mut registry = ParserRegistry::new(Some(test_dir.clone()));
        let parsed = registry
            .parse_instruction(&instruction, &program_id)
            .expect("expected parser registry to preserve instruction metadata");

        assert_eq!(parsed.name, "swap_toc");
        assert_eq!(parsed.account_names, vec!["payer"]);
        assert!(matches!(
            parsed.fields,
            ParsedInstructionFields::RawHex(raw_hex) if raw_hex == "0025a16608ca3c00"
        ));

        std::fs::remove_file(idl_path).ok();
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
