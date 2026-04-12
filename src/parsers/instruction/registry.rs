use std::collections::HashMap;

use anyhow::{Context, Result};
use solana_account::ReadableAccount;
use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;

use crate::core::transaction::InstructionSummary;
use sonar_sim::internals::ResolvedAccounts;

use super::types::ParsedInstruction;
use super::{
    AssociatedTokenProgramParser, ComputeBudgetProgramParser, InstructionParser, MemoProgramParser,
    SystemProgramParser, Token2022ProgramParser, anchor_idl,
};

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

    /// Load IDL parser for a specific program ID if needed
    pub fn load_idl_parser_if_needed(&mut self, program_id: &Pubkey) -> Result<bool> {
        if self.parsers.contains_key(program_id) {
            return Ok(false);
        }

        let idl_dir = match &self.idl_directory {
            Some(dir) => dir,
            None => return Ok(false),
        };

        let idl_file_path = idl_dir.join(format!("{}.json", program_id));
        if !idl_file_path.exists() {
            return Ok(false);
        }

        log::info!("Lazy-loading IDL for program: {}", program_id);

        let idl_content = std::fs::read_to_string(&idl_file_path)
            .with_context(|| format!("Failed to read IDL file: {}", idl_file_path.display()))?;

        let program_address = program_id.to_string();
        let indexed_idl =
            anchor_idl::IndexedIdl::from_json_with_program_address(&idl_content, &program_address)
                .with_context(|| {
                    format!("Failed to parse IDL JSON: {}", idl_file_path.display())
                })?;

        let parser = Box::new(anchor_idl::AnchorIdlParser::new(*program_id, indexed_idl));

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
        instruction: &InstructionSummary,
        program_id: &Pubkey,
    ) -> Option<ParsedInstruction> {
        if !anchor_idl::is_anchor_cpi_event(instruction) {
            return None;
        }

        log::debug!("Detected CPI event for program: {}", program_id);

        if !self.parsers.contains_key(program_id) {
            if let Err(err) = self.load_idl_parser_if_needed(program_id) {
                log::warn!("Failed to load IDL for CPI event parsing: {:?}", err);
                return None;
            }
        }

        let parser = self.parsers.get(program_id)?;
        match parser.parse_cpi_event(instruction, program_id) {
            Ok(Some(parsed)) => {
                log::debug!("Parsed CPI event: {}", parsed.name);
                Some(parsed)
            }
            Ok(None) => None,
            Err(e) => {
                log::debug!("CPI event parse error: {:?}", e);
                None
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use solana_account::{Account, AccountSharedData};
    use solana_pubkey::Pubkey;
    use solana_sdk_ids::{bpf_loader_upgradeable, system_program};
    use sonar_sim::internals::ResolvedAccounts;
    use std::collections::HashMap;
    use std::path::PathBuf;

    use sonar_idl::IdlValue;

    #[test]
    fn test_token2022_parser_registration() {
        let registry = ParserRegistry::new(None);
        let token2022_id = Pubkey::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

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
    fn parser_registry_decodes_trailing_absent_args_as_null() {
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

        // Instruction data: 8-byte disc + 8-byte u64 arg — no bytes for the
        // trailing u16 arg. Programs commonly add optional trailing args that
        // callers may omit, so sonar should decode what's present and default
        // the absent trailing arg to null.
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
        let fields = parsed.fields.parsed_fields().expect("should be Parsed, not RawHex");
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "amount_in");
        assert_eq!(fields[0].value, IdlValue::U64(0x003cca0866a12500));
        assert_eq!(fields[1].name, "slippage_bps");
        assert_eq!(fields[1].value, IdlValue::Null);

        std::fs::remove_file(idl_path).ok();
        std::fs::remove_dir_all(test_dir).ok();
    }
}
