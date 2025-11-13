use std::collections::HashMap;
use std::str::FromStr;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use colored::Colorize;
use serde::Serialize;
use solana_pubkey::Pubkey;
use solana_transaction::versioned::TransactionVersion;

use crate::{
    account_loader::{ResolvedAccounts, ResolvedLookup},
    cli::{Funding, OutputFormat, ProgramReplacement},
    executor::{ExecutionStatus, SimulationResult},
    instruction_parsers::{ParsedInstruction, ParserRegistry},
    transaction::{AccountReferenceSummary, AccountSourceSummary, ParsedTransaction},
};
use litesvm::types::TransactionMetadata;

pub fn render(
    parsed: &ParsedTransaction,
    resolved: &ResolvedAccounts,
    simulation: &SimulationResult,
    replacements: &[ProgramReplacement],
    fundings: &[Funding],
    parser_registry: &ParserRegistry,
    format: OutputFormat,
    verify_signatures: bool,
) -> Result<()> {
    let report = Report::from_sources(
        parsed,
        resolved,
        simulation,
        replacements,
        fundings,
        parser_registry,
        verify_signatures,
    );
    match format {
        OutputFormat::Text => render_text(&report, resolved, parser_registry),
        OutputFormat::Json => render_json(&report),
    }
}

pub fn render_transaction_only(
    parsed: &ParsedTransaction,
    resolved: &ResolvedAccounts,
    parser_registry: &ParserRegistry,
    format: OutputFormat,
) -> Result<()> {
    let resolver = LookupResolver::new(resolved.lookup_details());
    let transaction =
        TransactionSection::from_sources(parsed, resolved, &resolver, parser_registry, false);
    match format {
        OutputFormat::Text => {
            render_transaction_section_text(&transaction, resolved, parser_registry);
            Ok(())
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&transaction)?;
            println!("{json}");
            Ok(())
        }
    }
}

fn render_text(
    report: &Report,
    resolved: &ResolvedAccounts,
    parser_registry: &ParserRegistry,
) -> Result<()> {
    render_transaction_section_text(&report.transaction, resolved, parser_registry);
    render_fundings_text(&report.fundings);
    render_replacements_text(&report.replacements);
    render_simulation_text(&report.simulation);
    Ok(())
}

fn render_transaction_section_text(
    transaction: &TransactionSection,
    resolved: &ResolvedAccounts,
    parser_registry: &ParserRegistry,
) {
    render_transaction_overview_text(transaction);
    render_lookup_tables_text(transaction);
    render_account_list_text(transaction, resolved);
    render_instruction_details_text(transaction, resolved, parser_registry);
}

fn render_transaction_overview_text(transaction: &TransactionSection) {
    println!("=== Transaction Overview ===");
    println!("Encoding: {}", transaction.encoding);
    println!("Version: {}", transaction.version);
    println!("Recent Blockhash: {}", transaction.recent_blockhash);
    println!("Signatures:");
    for sig in &transaction.signatures {
        println!("  {}", sig);
    }
}

fn render_lookup_tables_text(transaction: &TransactionSection) {
    if transaction.lookups.is_empty() {
        return;
    }

    println!("Address Lookup Tables");
    for (idx, lookup) in transaction.lookups.iter().enumerate() {
        let solscan_linked_key = format_solscan_link(&lookup.account_key);
        println!("  [{}] {}", idx, solscan_linked_key);
    }
}

fn render_account_list_text(transaction: &TransactionSection, resolved: &ResolvedAccounts) {
    println!("\nAccount List:");
    let mut account_index = 0;

    // Render static accounts
    for account in &transaction.static_accounts {
        account_index = render_account_entry_text(
            account_index,
            &account.pubkey,
            account.signer,
            account.writable,
            resolved,
        );
    }

    // Render lookup table accounts (writable first, then readonly)
    for lookup in &transaction.lookups {
        for entry in &lookup.writable {
            account_index =
                render_account_entry_text(account_index, &entry.pubkey, false, true, resolved);
        }
    }

    for lookup in &transaction.lookups {
        for entry in &lookup.readonly {
            account_index =
                render_account_entry_text(account_index, &entry.pubkey, false, false, resolved);
        }
    }
}

fn render_account_entry_text(
    index: usize,
    pubkey_str: &str,
    signer: bool,
    writable: bool,
    resolved: &ResolvedAccounts,
) -> usize {
    let pubkey = Pubkey::from_str(pubkey_str).unwrap();
    let solscan_linked_pubkey = format_solscan_link(pubkey_str);
    let executable = resolved.accounts.get(&pubkey).map(|acc| acc.executable).unwrap_or(false);
    println!(
        "  [{}] {} {}",
        index,
        solscan_linked_pubkey,
        account_privilege_emoji(signer, writable, executable)
    );
    index + 1
}

fn render_instruction_details_text(
    transaction: &TransactionSection,
    resolved: &ResolvedAccounts,
    _parser_registry: &ParserRegistry,
) {
    println!("\nInstruction Details:");
    for ix in &transaction.instructions {
        let program_pubkey_with_link = format_solscan_link(&ix.program.pubkey);
        // Display outer instruction with 1-based indexing (#1, #2, #3, etc.)
        let outer_number = ix.index + 1;

        // Try to parse the instruction
        if let Some(parsed) = &ix.parsed {
            println!(
                "  #{} {} [{}]",
                outer_number.to_string().custom_color((255, 165, 0)),
                program_pubkey_with_link.custom_color((62, 132, 230)),
                parsed.name.custom_color((124, 252, 0))
            );

            // Render accounts with parsed names
            for (i, account) in ix.accounts.iter().enumerate() {
                let account_name = if i < parsed.account_names.len() {
                    parsed.account_names[i].clone()
                } else {
                    format!("account_{}", i)
                };
                render_instruction_account_text_with_name(account, resolved, &account_name);
            }

            // Display raw instruction data first
            println!("     🔢 0x{} | {} byte(s)", hex::encode(&ix.data), ix.data.len());

            // Then render parsed fields as formatted JSON, preserving original order
            if !parsed.fields.is_empty() {
                println!("       {{");
                for (idx, (field_name, field_value)) in parsed.fields.iter().enumerate() {
                    // Format as JSON key-value pair with proper indentation (9 spaces total: 7 + 2)
                    let is_last = idx == parsed.fields.len() - 1;
                    let comma = if is_last { "" } else { "," };
                    let formatted_line =
                        format!("         \"{}\": \"{}\"{}", field_name, field_value, comma);
                    println!("{}", formatted_line.custom_color((255, 255, 224)));
                }
                println!("       }}");
            }
        } else {
            println!(
                "  #{} {}",
                outer_number.to_string().custom_color((255, 165, 0)),
                program_pubkey_with_link.custom_color((62, 132, 230))
            );

            for account in &ix.accounts {
                render_instruction_account_text(account, resolved);
            }
            println!("     🔢 0x{} | {} byte(s)", hex::encode(&ix.data), ix.data.len());
        }

        // Display inner instructions if any
        if !ix.inner_instructions.is_empty() {
            for inner_ix in &ix.inner_instructions {
                // Try to parse inner instruction
                if let Some(parsed_inner) = &inner_ix.parsed {
                    println!(
                        "    {} {} [{}]",
                        format!("#{}", inner_ix.label).custom_color((255, 165, 0)),
                        format_solscan_link(&inner_ix.program.pubkey).custom_color((62, 132, 230)),
                        parsed_inner.name.custom_color((124, 252, 0))
                    );

                    // Render accounts with parsed names
                    for (i, account) in inner_ix.accounts.iter().enumerate() {
                        let account_name = if i < parsed_inner.account_names.len() {
                            parsed_inner.account_names[i].clone()
                        } else {
                            format!("account_{}", i)
                        };
                        render_instruction_account_text_with_name(account, resolved, &account_name);
                    }

                    // Display raw instruction data first
                    println!(
                        "     🔢 0x{} | {} byte(s)",
                        hex::encode(&inner_ix.data),
                        inner_ix.data.len()
                    );

                    // Then render parsed fields as formatted JSON, preserving original order
                    if !parsed_inner.fields.is_empty() {
                        println!("       {{");
                        for (idx, (field_name, field_value)) in
                            parsed_inner.fields.iter().enumerate()
                        {
                            // Format as JSON key-value pair with proper indentation (9 spaces total: 7 + 2)
                            let is_last = idx == parsed_inner.fields.len() - 1;
                            let comma = if is_last { "" } else { "," };
                            let formatted_line = format!(
                                "         \"{}\": \"{}\"{}",
                                field_name, field_value, comma
                            );
                            println!("{}", formatted_line.custom_color((255, 255, 224)));
                        }
                        println!("       }}");
                    }
                } else {
                    println!(
                        "    {} {}",
                        format!("#{}", inner_ix.label).custom_color((255, 165, 0)),
                        format_solscan_link(&inner_ix.program.pubkey).custom_color((62, 132, 230))
                    );

                    for account in &inner_ix.accounts {
                        render_instruction_account_text(account, resolved);
                    }
                    println!(
                        "     🔢 0x{} | {} byte(s)",
                        hex::encode(&inner_ix.data),
                        inner_ix.data.len()
                    );
                }
            }
        }
    }
}

fn render_instruction_account_text(account: &InstructionAccountEntry, resolved: &ResolvedAccounts) {
    let solscan_linked_pubkey = format_solscan_link(&account.pubkey);
    let executable = if let Ok(pubkey) = Pubkey::from_str(&account.pubkey) {
        resolved.accounts.get(&pubkey).map(|acc| acc.executable).unwrap_or(false)
    } else {
        false
    };
    println!(
        "     {} [{}] {} {}",
        account.source,
        account.index,
        solscan_linked_pubkey,
        account_privilege_emoji(account.signer, account.writable, executable)
    );
}

fn render_instruction_account_text_with_name(
    account: &InstructionAccountEntry,
    resolved: &ResolvedAccounts,
    name: &str,
) {
    let solscan_linked_pubkey = format_solscan_link(&account.pubkey);
    let executable = if let Ok(pubkey) = Pubkey::from_str(&account.pubkey) {
        resolved.accounts.get(&pubkey).map(|acc| acc.executable).unwrap_or(false)
    } else {
        false
    };
    println!(
        "     {} [{}] {} {} ({})",
        account.source,
        account.index,
        solscan_linked_pubkey,
        account_privilege_emoji(account.signer, account.writable, executable),
        name.custom_color((135, 206, 235))
    );
}

fn render_fundings_text(fundings: &[FundingSection]) {
    if fundings.is_empty() {
        return;
    }

    println!("\nAccount Funding:");
    for funding in fundings {
        println!("  {} <= {} SOL", funding.pubkey, funding.amount_sol);
    }
}

fn render_replacements_text(replacements: &[ReplacementSection]) {
    if replacements.is_empty() {
        return;
    }

    println!("\nProgram Replacements:");
    for replacement in replacements {
        println!("  {} <= {}", replacement.program_id, replacement.path);
    }
}

fn render_simulation_text(simulation: &SimulationSection) {
    println!("\n=== Simulation Result ===");
    match &simulation.status {
        SimulationStatusReport::Succeeded => {
            println!("🟢");
        }
        SimulationStatusReport::Failed { error } => {
            println!("🔴 ({})", error);
        }
    }
    println!("Compute Units Consumed: {}", simulation.compute_units_consumed);
    println!("Log Entries: {}", simulation.logs.len());
    if !simulation.logs.is_empty() {
        println!("Log Content:");
        for line in &simulation.logs {
            println!("  {}", line);
        }
    }

    if let Some(return_data) = &simulation.return_data {
        println!(
            "Return Data: Program {} ({} bytes, base64: {})",
            return_data.program_id,
            return_data.size,
            truncate_display(&return_data.data_base64, 120)
        );
    }

    println!("Returned Account Count: {}", simulation.post_account_count);
}

fn render_json(report: &Report) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    println!("{json}");
    Ok(())
}

#[derive(Serialize)]
struct Report {
    transaction: TransactionSection,
    simulation: SimulationSection,
    replacements: Vec<ReplacementSection>,
    fundings: Vec<FundingSection>,
}

impl Report {
    fn from_sources(
        parsed: &ParsedTransaction,
        resolved: &ResolvedAccounts,
        simulation: &SimulationResult,
        replacements: &[ProgramReplacement],
        fundings: &[Funding],
        parser_registry: &ParserRegistry,
        verify_signatures: bool,
    ) -> Self {
        let resolver = LookupResolver::new(resolved.lookup_details());
        let transaction = TransactionSection::from_sources(
            parsed,
            resolved,
            &resolver,
            parser_registry,
            verify_signatures,
        );
        let simulation = SimulationSection::from_result(simulation);
        let replacements = replacements
            .iter()
            .map(|entry| ReplacementSection {
                program_id: entry.program_id.to_string(),
                path: entry.so_path.display().to_string(),
            })
            .collect();
        let fundings = fundings
            .iter()
            .map(|entry| FundingSection {
                pubkey: entry.pubkey.to_string(),
                amount_sol: entry.amount_sol,
            })
            .collect();
        Self { transaction, simulation, replacements, fundings }
    }
}

#[derive(Serialize)]
struct TransactionSection {
    encoding: String,
    version: String,
    signatures: Vec<String>,
    recent_blockhash: String,
    static_accounts: Vec<AccountEntry>,
    lookups: Vec<LookupSection>,
    instructions: Vec<InstructionSection>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    verify_signatures: bool,
}

impl TransactionSection {
    fn from_sources(
        parsed: &ParsedTransaction,
        resolved: &ResolvedAccounts,
        resolver: &LookupResolver,
        parser_registry: &ParserRegistry,
        verify_signatures: bool,
    ) -> Self {
        let encoding = match parsed.encoding {
            crate::transaction::RawTransactionEncoding::Base58 => "base58",
            crate::transaction::RawTransactionEncoding::Base64 => "base64",
        }
        .to_string();

        let version = match parsed.version {
            TransactionVersion::Legacy(_) => "legacy".to_string(),
            TransactionVersion::Number(v) => format!("v{v}"),
        };

        let static_accounts = parsed
            .summary
            .static_accounts
            .iter()
            .map(|entry| AccountEntry {
                index: entry.index,
                pubkey: entry.pubkey.clone(),
                signer: entry.signer,
                writable: entry.writable,
            })
            .collect();

        let instructions = parsed
            .summary
            .instructions
            .iter()
            .map(|ix| {
                InstructionSection::from_summary(
                    ix,
                    resolver,
                    &parsed.summary.inner_instructions,
                    parsed,
                    parser_registry,
                )
            })
            .collect();

        let lookups = resolved.lookups.iter().map(LookupSection::from_lookup).collect();

        Self {
            encoding,
            version,
            signatures: parsed.summary.signatures.clone(),
            recent_blockhash: parsed.summary.recent_blockhash.clone(),
            static_accounts,
            lookups,
            instructions,
            verify_signatures,
        }
    }
}

#[derive(Serialize)]
struct AccountEntry {
    index: usize,
    pubkey: String,
    signer: bool,
    writable: bool,
}

#[derive(Serialize)]
struct LookupSection {
    account_key: String,
    writable: Vec<LookupAddressEntry>,
    readonly: Vec<LookupAddressEntry>,
}

impl LookupSection {
    fn from_lookup(lookup: &ResolvedLookup) -> Self {
        let writable = lookup
            .writable_indexes
            .iter()
            .zip(&lookup.writable_addresses)
            .map(|(idx, key)| LookupAddressEntry { index: *idx, pubkey: key.to_string() })
            .collect();
        let readonly = lookup
            .readonly_indexes
            .iter()
            .zip(&lookup.readonly_addresses)
            .map(|(idx, key)| LookupAddressEntry { index: *idx, pubkey: key.to_string() })
            .collect();

        Self { account_key: lookup.account_key.to_string(), writable, readonly }
    }
}

#[derive(Serialize)]
struct LookupAddressEntry {
    index: u8,
    pubkey: String,
}

#[derive(Serialize)]
struct InstructionSection {
    index: usize,
    program: InstructionAccountEntry,
    accounts: Vec<InstructionAccountEntry>,
    data: Box<[u8]>,
    parsed: Option<ParsedInstruction>,
    inner_instructions: Vec<InnerInstructionSection>,
}

impl InstructionSection {
    fn from_summary(
        summary: &crate::transaction::InstructionSummary,
        resolver: &LookupResolver,
        inner_instructions_list: &[solana_message::inner_instruction::InnerInstructions],
        parsed: &ParsedTransaction,
        parser_registry: &ParserRegistry,
    ) -> Self {
        let program =
            InstructionAccountEntry::from_reference_with_resolver(&summary.program, Some(resolver));
        let accounts = summary
            .accounts
            .iter()
            .map(|account| {
                InstructionAccountEntry::from_reference_with_resolver(account, Some(resolver))
            })
            .collect();

        // Try to parse the instruction
        let parsed_instruction = if let Some(program_pubkey) = &summary.program.pubkey {
            if let Ok(program_id) = Pubkey::from_str(program_pubkey) {
                parser_registry.parse_instruction(summary, &program_id)
            } else {
                None
            }
        } else {
            None
        };

        let inner_instructions = if summary.index < inner_instructions_list.len() {
            inner_instructions_list[summary.index]
                .iter()
                .enumerate()
                .map(|(inner_idx, inner_ix)| {
                    InnerInstructionSection::from_inner_instruction(
                        inner_ix,
                        resolver,
                        &format!("{}.{}", summary.index + 1, inner_idx + 1),
                        parsed,
                        parser_registry,
                    )
                })
                .collect()
        } else {
            Vec::new()
        };

        Self {
            index: summary.index,
            program,
            accounts,
            data: summary.data.clone(),
            parsed: parsed_instruction,
            inner_instructions,
        }
    }
}

#[derive(Serialize)]
struct InnerInstructionSection {
    label: String,
    program: InstructionAccountEntry,
    accounts: Vec<InstructionAccountEntry>,
    data: Box<[u8]>,
    parsed: Option<ParsedInstruction>,
}

impl InnerInstructionSection {
    fn from_inner_instruction(
        inner_ix: &solana_message::inner_instruction::InnerInstruction,
        resolver: &LookupResolver,
        label: &str,
        parsed: &ParsedTransaction,
        parser_registry: &ParserRegistry,
    ) -> Self {
        // Resolve inner instruction accounts using the same logic as outer instructions
        let message = &parsed.transaction.message;
        let lookup_locations =
            crate::transaction::build_lookup_locations(&parsed.account_plan.address_lookups);

        let program = {
            let ref_summary = crate::transaction::classify_account_reference(
                message,
                inner_ix.instruction.program_id_index as usize,
                &parsed.account_plan,
                &lookup_locations,
            );
            InstructionAccountEntry::from_reference_with_resolver(&ref_summary, Some(resolver))
        };

        let accounts = inner_ix
            .instruction
            .accounts
            .iter()
            .map(|account_index| {
                let ref_summary = crate::transaction::classify_account_reference(
                    message,
                    *account_index as usize,
                    &parsed.account_plan,
                    &lookup_locations,
                );
                InstructionAccountEntry::from_reference_with_resolver(&ref_summary, Some(resolver))
            })
            .collect();

        // Try to parse the inner instruction
        let parsed_instruction = if let Ok(program_id) = Pubkey::from_str(&program.pubkey) {
            // Create a summary for the inner instruction
            let inner_summary = crate::transaction::InstructionSummary {
                index: 0, // Inner instruction index doesn't matter for parsing
                program: crate::transaction::AccountReferenceSummary {
                    index: inner_ix.instruction.program_id_index as usize,
                    pubkey: Some(program_id.to_string()),
                    signer: false,
                    writable: false,
                    source: crate::transaction::AccountSourceSummary::Static,
                },
                accounts: inner_ix
                    .instruction
                    .accounts
                    .iter()
                    .map(|account_index| {
                        let ref_summary = crate::transaction::classify_account_reference(
                            message,
                            *account_index as usize,
                            &parsed.account_plan,
                            &lookup_locations,
                        );
                        ref_summary
                    })
                    .collect(),
                data: inner_ix.instruction.data.clone().into_boxed_slice(),
            };
            parser_registry.parse_instruction(&inner_summary, &program_id)
        } else {
            None
        };

        Self {
            label: label.to_string(),
            program,
            accounts,
            data: inner_ix.instruction.data.clone().into_boxed_slice(),
            parsed: parsed_instruction,
        }
    }
}

#[derive(Serialize)]
struct InstructionAccountEntry {
    index: usize,
    pubkey: String,
    signer: bool,
    writable: bool,
    source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    lookup_table: Option<LookupReference>,
}

impl InstructionAccountEntry {
    fn from_reference_with_resolver(
        reference: &AccountReferenceSummary,
        resolver: Option<&LookupResolver>,
    ) -> Self {
        let (pubkey, source, lookup_table) = match &reference.source {
            AccountSourceSummary::Static => {
                (reference.pubkey.clone().unwrap_or_else(|| "<missing>".into()), "⚓", None)
            }
            AccountSourceSummary::Lookup { table_account, lookup_index, writable } => {
                let resolved =
                    resolver.and_then(|res| res.resolve(table_account, *writable, *lookup_index));
                let pubkey = resolved
                    .or_else(|| reference.pubkey.clone())
                    .unwrap_or_else(|| "<lookup-not-resolved>".into());
                let lookup_ref = LookupReference {
                    account_key: table_account.clone(),
                    index: *lookup_index,
                    writable: *writable,
                };
                (pubkey, "🔍", Some(lookup_ref))
            }
            AccountSourceSummary::Unknown => {
                (reference.pubkey.clone().unwrap_or_else(|| "<unknown>".into()), "unknown", None)
            }
        };

        Self {
            index: reference.index,
            pubkey,
            signer: reference.signer,
            writable: reference.writable,
            source,
            lookup_table,
        }
    }
}

#[derive(Serialize)]
struct LookupReference {
    account_key: String,
    index: u8,
    writable: bool,
}

#[derive(Serialize)]
struct SimulationSection {
    status: SimulationStatusReport,
    compute_units_consumed: u64,
    logs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    return_data: Option<ReturnDataReport>,
    post_account_count: usize,
}

impl SimulationSection {
    fn from_result(result: &SimulationResult) -> Self {
        let (status, post_account_count) = match &result.status {
            ExecutionStatus::Succeeded => {
                (SimulationStatusReport::Succeeded, result.post_accounts.len())
            }
            ExecutionStatus::Failed(error) => {
                (SimulationStatusReport::Failed { error: error.clone() }, 0)
            }
        };

        Self {
            status,
            compute_units_consumed: result.meta.compute_units_consumed,
            logs: result.meta.logs.clone(),
            return_data: ReturnDataReport::from_metadata(&result.meta),
            post_account_count,
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
enum SimulationStatusReport {
    Succeeded,
    Failed { error: String },
}

#[derive(Serialize)]
struct ReturnDataReport {
    program_id: String,
    size: usize,
    data_base64: String,
}

impl ReturnDataReport {
    fn from_metadata(meta: &TransactionMetadata) -> Option<Self> {
        if meta.return_data.data.is_empty() {
            None
        } else {
            Some(ReturnDataReport {
                program_id: meta.return_data.program_id.to_string(),
                size: meta.return_data.data.len(),
                data_base64: BASE64_STANDARD.encode(&meta.return_data.data),
            })
        }
    }
}

#[derive(Serialize)]
struct FundingSection {
    pubkey: String,
    amount_sol: f64,
}

#[derive(Serialize)]
struct ReplacementSection {
    program_id: String,
    path: String,
}

struct LookupResolver {
    entries: HashMap<(String, bool, u8), String>,
}

impl LookupResolver {
    fn new(lookups: &[ResolvedLookup]) -> Self {
        let mut entries = HashMap::new();
        for lookup in lookups {
            let account_key = lookup.account_key.to_string();
            for (idx, key) in lookup.writable_indexes.iter().zip(&lookup.writable_addresses) {
                entries.insert((account_key.clone(), true, *idx), key.to_string());
            }
            for (idx, key) in lookup.readonly_indexes.iter().zip(&lookup.readonly_addresses) {
                entries.insert((account_key.clone(), false, *idx), key.to_string());
            }
        }
        Self { entries }
    }

    fn resolve(&self, table: &str, writable: bool, index: u8) -> Option<String> {
        self.entries.get(&(table.to_string(), writable, index)).cloned()
    }
}

fn truncate_display(value: &str, limit: usize) -> String {
    if value.len() <= limit { value.to_string() } else { format!("{}…", &value[..limit]) }
}

fn format_solscan_link(account_pubkey: &str) -> String {
    let solscan_url = format!("https://solscan.io/account/{}", account_pubkey);
    format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", solscan_url, account_pubkey)
}

fn account_privilege_emoji(signer: bool, writable: bool, executable: bool) -> &'static str {
    if executable {
        "⚡"
    } else {
        match (signer, writable) {
            (true, true) => "📜 🔑",
            (true, false) => "🔒 🔑",
            (false, true) => "📜",
            (false, false) => "🔒",
        }
    }
}
