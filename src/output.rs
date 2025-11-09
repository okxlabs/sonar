use std::collections::HashMap;

use anyhow::Result;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde::Serialize;
use solana_sdk::transaction::TransactionVersion;

use crate::{
    account_loader::{ResolvedAccounts, ResolvedLookup},
    cli::{OutputFormat, ProgramReplacement},
    executor::{ExecutionStatus, SimulationResult},
    transaction::{AccountReferenceSummary, AccountSourceSummary, ParsedTransaction},
};
use litesvm::types::TransactionMetadata;

pub fn render(
    parsed: &ParsedTransaction,
    resolved: &ResolvedAccounts,
    simulation: &SimulationResult,
    replacements: &[ProgramReplacement],
    format: OutputFormat,
) -> Result<()> {
    let report = Report::from_sources(parsed, resolved, simulation, replacements);
    match format {
        OutputFormat::Text => render_text(&report),
        OutputFormat::Json => render_json(&report),
    }
}

pub fn render_transaction_only(
    parsed: &ParsedTransaction,
    resolved: &ResolvedAccounts,
    format: OutputFormat,
) -> Result<()> {
    let resolver = LookupResolver::new(resolved.lookup_details());
    let transaction = TransactionSection::from_sources(parsed, resolved, &resolver);
    match format {
        OutputFormat::Text => {
            render_transaction_section_text(&transaction);
            Ok(())
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&transaction)?;
            println!("{json}");
            Ok(())
        }
    }
}

fn render_text(report: &Report) -> Result<()> {
    render_transaction_section_text(&report.transaction);
    render_replacements_text(&report.replacements);
    render_simulation_text(&report.simulation);
    Ok(())
}

fn render_transaction_section_text(transaction: &TransactionSection) {
    println!("=== Transaction Overview ===");
    println!("Encoding: {}", transaction.encoding);
    println!("Version: {}", transaction.version);
    println!("Recent Blockhash: {}", transaction.recent_blockhash);
    println!("Signatures:");
    for sig in &transaction.signatures {
        println!("  - {}", sig);
    }

    println!("\nAccount List:");
    for account in &transaction.static_accounts {
        println!(
            "  [{}] {} (signer: {}, writable: {})",
            account.index, account.pubkey, account.signer, account.writable
        );
    }

    if !transaction.lookups.is_empty() {
        println!("\nAddress Lookup Tables:");
        for lookup in &transaction.lookups {
            println!("  Table: {}", lookup.account_key);
            if !lookup.writable.is_empty() {
                println!("    Writable Accounts:");
                for entry in &lookup.writable {
                    println!("      - [{}] {}", entry.index, entry.pubkey);
                }
            }
            if !lookup.readonly.is_empty() {
                println!("    Read-only Accounts:");
                for entry in &lookup.readonly {
                    println!("      - [{}] {}", entry.index, entry.pubkey);
                }
            }
        }
    }

    println!("\nInstruction Details:");
    for ix in &transaction.instructions {
        println!("  #{} Program: {}", ix.index, ix.program.pubkey);
        for account in &ix.accounts {
            println!(
                "    - [{}] {} (signer: {}, writable: {}, source: {})",
                account.index, account.pubkey, account.signer, account.writable, account.source
            );
            // if let Some(lookup) = &account.lookup_table {
            //     println!(
            //         "      lookup: {} [{}] (writable: {})",
            //         lookup.account_key, lookup.index, lookup.writable
            //     );
            // }
        }
        println!("    Data Length: {} bytes", ix.data_length);
    }
}

fn render_replacements_text(replacements: &[ReplacementSection]) {
    if replacements.is_empty() {
        return;
    }

    println!("\nProgram Replacements:");
    for replacement in replacements {
        println!("  - {} <= {}", replacement.program_id, replacement.path);
    }
}

fn render_simulation_text(simulation: &SimulationSection) {
    println!("\n=== Simulation Result ===");
    match &simulation.status {
        SimulationStatusReport::Succeeded => {
            println!("Status: Success");
        }
        SimulationStatusReport::Failed { error } => {
            println!("Status: Failed ({error})");
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
}

impl Report {
    fn from_sources(
        parsed: &ParsedTransaction,
        resolved: &ResolvedAccounts,
        simulation: &SimulationResult,
        replacements: &[ProgramReplacement],
    ) -> Self {
        let resolver = LookupResolver::new(resolved.lookup_details());
        let transaction = TransactionSection::from_sources(parsed, resolved, &resolver);
        let simulation = SimulationSection::from_result(simulation);
        let replacements = replacements
            .iter()
            .map(|entry| ReplacementSection {
                program_id: entry.program_id.to_string(),
                path: entry.so_path.display().to_string(),
            })
            .collect();
        Self {
            transaction,
            simulation,
            replacements,
        }
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
}

impl TransactionSection {
    fn from_sources(
        parsed: &ParsedTransaction,
        resolved: &ResolvedAccounts,
        resolver: &LookupResolver,
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
            .map(|ix| InstructionSection::from_summary(ix, resolver))
            .collect();

        let lookups = resolved
            .lookups
            .iter()
            .map(LookupSection::from_lookup)
            .collect();

        Self {
            encoding,
            version,
            signatures: parsed.summary.signatures.clone(),
            recent_blockhash: parsed.summary.recent_blockhash.clone(),
            static_accounts,
            lookups,
            instructions,
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
            .map(|(idx, key)| LookupAddressEntry {
                index: *idx,
                pubkey: key.to_string(),
            })
            .collect();
        let readonly = lookup
            .readonly_indexes
            .iter()
            .zip(&lookup.readonly_addresses)
            .map(|(idx, key)| LookupAddressEntry {
                index: *idx,
                pubkey: key.to_string(),
            })
            .collect();

        Self {
            account_key: lookup.account_key.to_string(),
            writable,
            readonly,
        }
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
    data_length: usize,
}

impl InstructionSection {
    fn from_summary(
        summary: &crate::transaction::InstructionSummary,
        resolver: &LookupResolver,
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
        Self {
            index: summary.index,
            program,
            accounts,
            data_length: summary.data_length,
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
            AccountSourceSummary::Static => (
                reference
                    .pubkey
                    .clone()
                    .unwrap_or_else(|| "<missing>".into()),
                "static",
                None,
            ),
            AccountSourceSummary::Lookup {
                table_account,
                lookup_index,
                writable,
            } => {
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
                (pubkey, "lookup", Some(lookup_ref))
            }
            AccountSourceSummary::Unknown => (
                reference
                    .pubkey
                    .clone()
                    .unwrap_or_else(|| "<unknown>".into()),
                "unknown",
                None,
            ),
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
            ExecutionStatus::Succeeded => (
                SimulationStatusReport::Succeeded,
                result.post_accounts.len(),
            ),
            ExecutionStatus::Failed(error) => (
                SimulationStatusReport::Failed {
                    error: error.clone(),
                },
                0,
            ),
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
            for (idx, key) in lookup
                .writable_indexes
                .iter()
                .zip(&lookup.writable_addresses)
            {
                entries.insert((account_key.clone(), true, *idx), key.to_string());
            }
            for (idx, key) in lookup
                .readonly_indexes
                .iter()
                .zip(&lookup.readonly_addresses)
            {
                entries.insert((account_key.clone(), false, *idx), key.to_string());
            }
        }
        Self { entries }
    }

    fn resolve(&self, table: &str, writable: bool, index: u8) -> Option<String> {
        self.entries
            .get(&(table.to_string(), writable, index))
            .cloned()
    }
}

fn truncate_display(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        value.to_string()
    } else {
        format!("{}…", &value[..limit])
    }
}
