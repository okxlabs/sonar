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
    balance_changes::{compute_sol_changes, compute_token_changes, extract_mint_decimals_combined},
    cli::{Funding, OutputFormat, ProgramReplacement},
    executor::{ExecutionStatus, SimulationResult},
    funding::PreparedTokenFunding,
    instruction_parsers::anchor_idl::is_anchor_cpi_event,
    instruction_parsers::{ParsedField, ParsedInstruction, ParserRegistry},
    log_parser::{LogEntry, LogEntryWithDepth, parse_logs_by_instruction},
    transaction::{AccountReferenceSummary, AccountSourceSummary, ParsedTransaction},
};
use litesvm::types::TransactionMetadata;

/// Width of the separator line (using ═ character).
const SEPARATOR_WIDTH: usize = 120;

/// Balance change display options.
#[derive(Debug, Clone, Copy, Default)]
pub struct BalanceChangeOptions {
    pub show_balance_change: bool,
}

/// Log display options.
#[derive(Debug, Clone, Copy, Default)]
pub struct LogDisplayOptions {
    /// If true, print raw logs; otherwise print structured execution trace.
    pub show_raw_log: bool,
}

pub fn render(
    parsed: &ParsedTransaction,
    resolved: &ResolvedAccounts,
    simulation: &SimulationResult,
    replacements: &[ProgramReplacement],
    fundings: &[Funding],
    token_fundings: &[PreparedTokenFunding],
    parser_registry: &mut ParserRegistry,
    format: OutputFormat,
    show_ix_data: bool,
    show_ix_detail: bool,
    verify_signatures: bool,
    balance_opts: BalanceChangeOptions,
    log_opts: LogDisplayOptions,
) -> Result<()> {
    let report = Report::from_sources(
        parsed,
        resolved,
        simulation,
        replacements,
        fundings,
        token_fundings,
        parser_registry,
        verify_signatures,
        balance_opts,
    );
    match format {
        OutputFormat::Text => {
            render_text(&report, resolved, parser_registry, show_ix_data, show_ix_detail, log_opts)
        }
        OutputFormat::Json => render_json(&report),
    }
}

pub fn render_transaction_only(
    parsed: &ParsedTransaction,
    resolved: &ResolvedAccounts,
    parser_registry: &mut ParserRegistry,
    format: OutputFormat,
    show_ix_data: bool,
) -> Result<()> {
    let resolver = LookupResolver::new(resolved.lookup_details());
    let transaction =
        TransactionSection::from_sources(parsed, resolved, &resolver, parser_registry, false);
    match format {
        OutputFormat::Text => {
            render_transaction_section_text(&transaction, resolved, parser_registry, show_ix_data);
            Ok(())
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&transaction)?;
            println!("{json}");
            Ok(())
        }
    }
}

/// Render multiple transaction simulation results (bundle simulation).
pub fn render_bundle(
    parsed_txs: &[ParsedTransaction],
    total_tx_count: usize,
    resolved: &ResolvedAccounts,
    simulations: &[SimulationResult],
    replacements: &[ProgramReplacement],
    fundings: &[Funding],
    token_fundings: &[PreparedTokenFunding],
    parser_registry: &mut ParserRegistry,
    format: OutputFormat,
    _show_ix_data: bool,
    verify_signatures: bool,
    balance_opts: BalanceChangeOptions,
) -> Result<()> {
    let bundle_report = BundleReport::from_sources(
        parsed_txs,
        resolved,
        simulations,
        replacements,
        fundings,
        token_fundings,
        parser_registry,
        verify_signatures,
        balance_opts,
    );

    match format {
        OutputFormat::Text => render_bundle_text(&bundle_report, total_tx_count),
        OutputFormat::Json => render_bundle_json(&bundle_report),
    }
}

fn render_bundle_text(bundle: &BundleReport, total_count: usize) -> Result<()> {
    println!("=== Bundle Simulation ({} transactions) ===", total_count);

    // Render executed transactions with compact format
    for (i, tx_report) in bundle.transactions.iter().enumerate() {
        render_bundle_transaction_compact(i + 1, total_count, tx_report);
    }

    // Render skipped transactions (due to fail-fast)
    for i in bundle.transactions.len()..total_count {
        println!("\nTransaction {}/{}: (skipped)", i + 1, total_count);
    }

    // Render overall bundle balance changes
    render_bundle_balance_changes(bundle);

    // Summary
    render_bundle_summary(bundle, total_count);

    Ok(())
}

fn render_bundle_transaction_compact(
    index: usize,
    total: usize,
    tx_report: &BundleTransactionReport,
) {
    let sig = tx_report
        .transaction
        .signatures
        .first()
        .map(|s| truncate_sig(s, 12))
        .unwrap_or_else(|| "<no-sig>".to_string());

    println!("\nTransaction {}/{}: {}", index, total, sig);

    match &tx_report.simulation.status {
        SimulationStatusReport::Succeeded => println!("  Status: 🟢 SUCCESS"),
        SimulationStatusReport::Failed { error } => println!("  Status: 🔴 FAILED ({})", error),
    }
    println!("  Compute Units: {}", tx_report.simulation.compute_units_consumed);

    if !tx_report.simulation.logs.is_empty() {
        println!("  Logs:");
        for log in &tx_report.simulation.logs {
            println!("    {}", log);
        }
    }
}

/// Render overall bundle balance changes (first tx pre -> last successful tx post)
fn render_bundle_balance_changes(bundle: &BundleReport) {
    if !bundle.sol_balance_changes.is_empty() {
        println!("\n=== SOL Balance Changes (Bundle Total) ===");
        for change in &bundle.sol_balance_changes {
            let sol_before = change.before as f64 / 1_000_000_000.0;
            let sol_after = change.after as f64 / 1_000_000_000.0;
            let sign = if change.change >= 0 { "+" } else { "" };
            let color = if change.change >= 0 { (0, 255, 0) } else { (255, 0, 0) };
            println!(
                "  {} {:.9} | {:.9} | {}",
                change.account,
                sol_before,
                sol_after,
                format!("{}{:.9}", sign, change.change_sol).custom_color(color)
            );
        }
    }

    if !bundle.token_balance_changes.is_empty() {
        println!("\n=== Token Balance Changes (Bundle Total) ===");
        for change in &bundle.token_balance_changes {
            let divisor = 10f64.powi(change.decimals as i32);
            let ui_before = change.before as f64 / divisor;
            let ui_after = change.after as f64 / divisor;
            let sign = if change.change >= 0 { "+" } else { "" };
            let color = if change.change >= 0 { (0, 255, 0) } else { (255, 0, 0) };
            println!(
                "  {} ({}) {:.prec$} | {:.prec$} | {}",
                change.account,
                change.mint,
                ui_before,
                ui_after,
                format!("{}{:.prec$}", sign, change.ui_change, prec = change.decimals as usize)
                    .custom_color(color),
                prec = change.decimals as usize
            );
        }
    }
}

fn truncate_sig(sig: &str, prefix_len: usize) -> String {
    if sig.len() <= prefix_len * 2 + 3 {
        sig.to_string()
    } else {
        format!("{}...{}", &sig[..prefix_len], &sig[sig.len() - prefix_len..])
    }
}

fn render_bundle_summary(bundle: &BundleReport, total_count: usize) {
    let succeeded = bundle
        .transactions
        .iter()
        .filter(|t| matches!(t.simulation.status, SimulationStatusReport::Succeeded))
        .count();

    println!("\n{}", "═".repeat(50));
    if succeeded == total_count {
        println!("Bundle Summary: {}/{} succeeded", succeeded, total_count);
    } else {
        let failed_at = bundle
            .transactions
            .iter()
            .position(|t| matches!(t.simulation.status, SimulationStatusReport::Failed { .. }))
            .map(|i| i + 1);
        if let Some(idx) = failed_at {
            println!("Bundle Summary: FAILED at transaction {}", idx);
        } else {
            println!("Bundle Summary: {}/{} executed", bundle.transactions.len(), total_count);
        }
    }
    println!("{}", "═".repeat(50));
}

fn render_bundle_json(bundle: &BundleReport) -> Result<()> {
    let json = serde_json::to_string_pretty(bundle)?;
    println!("{json}");
    Ok(())
}

fn render_text(
    report: &Report,
    resolved: &ResolvedAccounts,
    _parser_registry: &mut ParserRegistry,
    show_ix_data: bool,
    show_ix_detail: bool,
    log_opts: LogDisplayOptions,
) -> Result<()> {
    // 1. Summary header (status + CU) - displayed first
    render_summary_header(&report.simulation, &report.transaction);

    // 3. Execution Trace (no title)
    render_execution_trace_section(&report.simulation, log_opts);

    if show_ix_detail {
        // 4. Section separator with empty lines
        render_section_separator();

        // 5. Instruction details (no title)
        render_instruction_details_text(&report.transaction, resolved, show_ix_data);

        // 6. Section separator with empty lines
        render_section_separator();
    }

    // 7. Balance Changes (no title)
    render_balance_changes_text(&report.sol_balance_changes, &report.token_balance_changes);

    // 8. Final empty line
    println!();

    Ok(())
}

/// Render a double-line separator.
fn render_separator() {
    println!("{}", "═".repeat(SEPARATOR_WIDTH));
}

/// Render a section separator with empty lines before and after.
fn render_section_separator() {
    println!();
    println!();
}

/// Render the summary header showing status and compute units (displayed first).
fn render_summary_header(simulation: &SimulationSection, transaction: &TransactionSection) {
    render_separator();

    // For failed transactions, don't print the error reason
    let status_str = match &simulation.status {
        SimulationStatusReport::Succeeded => "🟢 SUCCESS".to_string(),
        SimulationStatusReport::Failed { .. } => "🔴 FAILED".to_string(),
    };

    // Try to extract compute unit limit from SetComputeUnitLimit instruction
    let cu_limit = extract_compute_unit_limit(transaction).unwrap_or(200_000);
    let cu_used = simulation.compute_units_consumed;
    let percentage =
        if cu_limit > 0 { (cu_used as f64 / cu_limit as f64 * 100.0) as u32 } else { 0 };

    let result_text = format!(
        "Result: {} | CU: {} / {} ({}%)",
        status_str,
        format_with_commas(cu_used),
        format_with_commas(cu_limit),
        percentage
    );

    // Center the result text
    let text_len = result_text.chars().count();
    let padding = (SEPARATOR_WIDTH.saturating_sub(text_len)) / 2;
    println!("{:>width$}", result_text, width = padding + text_len);

    render_separator();
    println!(); // Empty line after summary
}

/// Extract compute unit limit from SetComputeUnitLimit instruction if present.
fn extract_compute_unit_limit(transaction: &TransactionSection) -> Option<u64> {
    use crate::instruction_parsers::{OrderedJsonValue, ParsedFieldValue};

    for ix in &transaction.instructions {
        if let Some(parsed) = &ix.parsed {
            if parsed.name == "SetComputeUnitLimit" {
                for field in &parsed.fields {
                    if field.name == "units" {
                        match &field.value {
                            ParsedFieldValue::Text(text) => {
                                if let Ok(units) = text.parse::<u64>() {
                                    return Some(units);
                                }
                            }
                            ParsedFieldValue::Json(json) => {
                                if let OrderedJsonValue::Number(num) = json {
                                    return num.as_u64();
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Format a number with comma separators for readability.
fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Render execution trace section with centered header.
fn render_execution_trace_section(simulation: &SimulationSection, log_opts: LogDisplayOptions) {
    if simulation.logs.is_empty() {
        return;
    }

    if log_opts.show_raw_log {
        for line in &simulation.logs {
            println!("{}", line);
        }
    } else {
        render_logs_structured(&simulation.logs);
    }
}

/// Render balance changes section with centered header.
/// Render balance changes without header (for new layout).
fn render_balance_changes_text(
    sol_changes: &[SolBalanceChangeSection],
    token_changes: &[TokenBalanceChangeSection],
) {
    if sol_changes.is_empty() && token_changes.is_empty() {
        return;
    }

    // SOL balance changes first
    for change in sol_changes {
        let sol_before = change.before as f64 / 1_000_000_000.0;
        let sol_after = change.after as f64 / 1_000_000_000.0;
        let sign = if change.change >= 0 { "+" } else { "" };
        let color = if change.change >= 0 { (0, 255, 0) } else { (255, 0, 0) };

        println!(
            "{} {:.9} | {:.9} | {}",
            change.account,
            sol_before,
            sol_after,
            format!("{}{:.9}", sign, change.change_sol).custom_color(color)
        );
    }

    // Empty line between SOL and Token changes
    if !sol_changes.is_empty() && !token_changes.is_empty() {
        println!();
    }

    // Token balance changes
    for change in token_changes {
        let divisor = 10f64.powi(change.decimals as i32);
        let ui_before = change.before as f64 / divisor;
        let ui_after = change.after as f64 / divisor;
        let sign = if change.change >= 0 { "+" } else { "" };
        let color = if change.change >= 0 { (0, 255, 0) } else { (255, 0, 0) };

        println!(
            "{} ({}) {:.prec$} | {:.prec$} | {}",
            change.account,
            change.mint,
            ui_before,
            ui_after,
            format!("{}{:.prec$}", sign, change.ui_change, prec = change.decimals as usize)
                .custom_color(color),
            prec = change.decimals as usize
        );
    }
}

fn render_transaction_section_text(
    transaction: &TransactionSection,
    resolved: &ResolvedAccounts,
    _parser_registry: &mut ParserRegistry,
    show_ix_data: bool,
) {
    println!(); // Empty line before instructions
    render_instruction_details_text(transaction, resolved, show_ix_data);
    println!();
    // Account list at the end
    render_lookup_tables_text(transaction);
    render_account_list_text(transaction, resolved);
}

fn render_lookup_tables_text(transaction: &TransactionSection) {
    if transaction.lookups.is_empty() {
        return;
    }

    for (idx, lookup) in transaction.lookups.iter().enumerate() {
        let solscan_linked_key = format_solscan_link(&lookup.account_key);
        println!("  [{}] {}", idx, solscan_linked_key);
    }
}

fn render_account_list_text(transaction: &TransactionSection, resolved: &ResolvedAccounts) {
    println!();
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

/// Render instruction details section with centered header.
fn render_instruction_details_text(
    transaction: &TransactionSection,
    resolved: &ResolvedAccounts,
    show_ix_data: bool,
) {
    for ix in &transaction.instructions {
        let program_pubkey_with_link = format_solscan_link(&ix.program.pubkey);
        // Display outer instruction with 1-based indexing (#1, #2, #3, etc.)
        let outer_number = ix.index + 1;

        // Try to parse the instruction
        if let Some(parsed) = &ix.parsed {
            println!(
                "#{} {} [{}]",
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

            // Display raw instruction data only when requested
            if show_ix_data {
                println!("  🔢 0x{} | {} byte(s)", hex::encode(&ix.data), ix.data.len());
            }

            // Then render parsed fields as formatted JSON, preserving original order
            render_parsed_fields(&parsed.fields);
        } else {
            println!(
                "#{} {}",
                outer_number.to_string().custom_color((255, 165, 0)),
                program_pubkey_with_link.custom_color((62, 132, 230))
            );

            for account in &ix.accounts {
                render_instruction_account_text(account, resolved);
            }
            println!("  🔢 0x{} | {} byte(s)", hex::encode(&ix.data), ix.data.len());
        }

        // Display inner instructions if any
        if !ix.inner_instructions.is_empty() {
            for inner_ix in &ix.inner_instructions {
                // Try to parse inner instruction
                if let Some(parsed_inner) = &inner_ix.parsed {
                    println!(
                        "  {} {} [{}]",
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
                        render_inner_instruction_account_text_with_name(
                            account,
                            resolved,
                            &account_name,
                        );
                    }

                    // Display raw instruction data only when requested
                    if show_ix_data {
                        println!(
                            "    🔢 0x{} | {} byte(s)",
                            hex::encode(&inner_ix.data),
                            inner_ix.data.len()
                        );
                    }

                    // Then render parsed fields as formatted JSON, preserving original order
                    render_inner_parsed_fields(&parsed_inner.fields);
                } else {
                    println!(
                        "  {} {}",
                        format!("#{}", inner_ix.label).custom_color((255, 165, 0)),
                        format_solscan_link(&inner_ix.program.pubkey).custom_color((62, 132, 230))
                    );

                    for account in &inner_ix.accounts {
                        render_inner_instruction_account_text(account, resolved);
                    }
                    println!(
                        "    🔢 0x{} | {} byte(s)",
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
        "  {} [{}] {} {}",
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
        "  {} [{}] {} {} ({})",
        account.source,
        account.index,
        solscan_linked_pubkey,
        account_privilege_emoji(account.signer, account.writable, executable),
        name.custom_color((135, 206, 235))
    );
}

fn render_inner_instruction_account_text(
    account: &InstructionAccountEntry,
    resolved: &ResolvedAccounts,
) {
    let solscan_linked_pubkey = format_solscan_link(&account.pubkey);
    let executable = if let Ok(pubkey) = Pubkey::from_str(&account.pubkey) {
        resolved.accounts.get(&pubkey).map(|acc| acc.executable).unwrap_or(false)
    } else {
        false
    };
    println!(
        "    {} [{}] {} {}",
        account.source,
        account.index,
        solscan_linked_pubkey,
        account_privilege_emoji(account.signer, account.writable, executable)
    );
}

fn render_inner_instruction_account_text_with_name(
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
        "    {} [{}] {} {} ({})",
        account.source,
        account.index,
        solscan_linked_pubkey,
        account_privilege_emoji(account.signer, account.writable, executable),
        name.custom_color((135, 206, 235))
    );
}

/// Render logs in a structured format, grouped by instruction with proper indentation.
fn render_logs_structured(logs: &[String]) {
    let instruction_logs = parse_logs_by_instruction(logs);

    for inst_logs in &instruction_logs {
        // Print instruction header
        let program_name = get_program_display_name(&inst_logs.program);
        println!(
            "\n{} {} instruction",
            format!("#{}", inst_logs.instruction_index + 1).bold(),
            program_name.bold()
        );

        // Print log entries with proper indentation
        for entry_with_depth in &inst_logs.entries {
            render_log_entry(entry_with_depth);
        }
    }
}

/// Get a display name for a program (friendly name or address).
fn get_program_display_name(pubkey: &str) -> &str {
    // Return the address as-is (known_programs feature not implemented)
    pubkey
}

/// Render a single log entry with appropriate formatting and color.
fn render_log_entry(entry_with_depth: &LogEntryWithDepth) {
    let depth = entry_with_depth.depth as usize;
    // Base indent for logs under instruction header, plus additional for CPI depth
    let indent = "  ".repeat(depth);

    match &entry_with_depth.entry {
        LogEntry::Invoke { program, depth: invoke_depth } => {
            // Only show "Invoking" for CPI calls (depth > 1)
            if *invoke_depth > 1 {
                let program_name = get_program_display_name(program);
                println!("{}{} {}", indent, "> Invoking".cyan(), program_name.cyan());
            }
        }
        LogEntry::Log { message } => {
            println!(
                "{}{} {}",
                indent,
                ">".custom_color((128, 128, 128)),
                format!("Program log: {}", message).custom_color((128, 128, 128))
            );
        }
        LogEntry::Data { data } => {
            println!(
                "{}{} {}",
                indent,
                ">".custom_color((128, 128, 128)),
                format!("Program data: {}", truncate_display(data, 60))
                    .custom_color((128, 128, 128))
            );
        }
        LogEntry::Consumed { _program: _, used, total } => {
            println!(
                "{}{} {}",
                indent,
                ">".custom_color((128, 128, 128)),
                format!("Program consumed: {} of {} compute units", used, total)
                    .custom_color((128, 128, 128))
            );
        }
        LogEntry::Success { _program: _ } => {
            println!("{}{} {}", indent, ">".green(), "Program returned success".green());
        }
        LogEntry::Failed { _program: _, error } => {
            println!("{}{} {}", indent, ">".red(), format!("Program failed: {}", error).red());
        }
        LogEntry::Return { _program: _, data } => {
            println!(
                "{}{} {}",
                indent,
                ">".custom_color((128, 128, 128)),
                format!("Program return: {}", truncate_display(data, 60))
                    .custom_color((128, 128, 128))
            );
        }
        LogEntry::Other(msg) => {
            if !msg.is_empty() {
                println!(
                    "{}{} {}",
                    indent,
                    ">".custom_color((128, 128, 128)),
                    msg.custom_color((128, 128, 128))
                );
            }
        }
    }
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
    token_fundings: Vec<TokenFundingSection>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sol_balance_changes: Vec<SolBalanceChangeSection>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    token_balance_changes: Vec<TokenBalanceChangeSection>,
}

#[derive(Serialize)]
struct SolBalanceChangeSection {
    account: String,
    before: u64,
    after: u64,
    change: i128,
    change_sol: f64,
}

#[derive(Serialize)]
struct TokenBalanceChangeSection {
    account: String,
    mint: String,
    before: u64,
    after: u64,
    change: i128,
    decimals: u8,
    ui_change: f64,
}

/// Report structure for bundle simulation (multiple transactions).
#[derive(Serialize)]
struct BundleReport {
    transactions: Vec<BundleTransactionReport>,
    replacements: Vec<ReplacementSection>,
    fundings: Vec<FundingSection>,
    token_fundings: Vec<TokenFundingSection>,
    /// SOL balance changes for the entire bundle (first tx pre -> last tx post)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sol_balance_changes: Vec<SolBalanceChangeSection>,
    /// Token balance changes for the entire bundle (first tx pre -> last tx post)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    token_balance_changes: Vec<TokenBalanceChangeSection>,
}

#[derive(Serialize)]
struct BundleTransactionReport {
    index: usize,
    transaction: TransactionSection,
    simulation: SimulationSection,
}

impl BundleReport {
    fn from_sources(
        parsed_txs: &[ParsedTransaction],
        resolved: &ResolvedAccounts,
        simulations: &[SimulationResult],
        replacements: &[ProgramReplacement],
        fundings: &[Funding],
        token_fundings: &[PreparedTokenFunding],
        parser_registry: &mut ParserRegistry,
        verify_signatures: bool,
        balance_opts: BalanceChangeOptions,
    ) -> Self {
        let resolver = LookupResolver::new(resolved.lookup_details());

        let transactions = parsed_txs
            .iter()
            .zip(simulations)
            .enumerate()
            .map(|(index, (parsed, simulation))| {
                let transaction = TransactionSection::from_sources(
                    parsed,
                    resolved,
                    &resolver,
                    parser_registry,
                    verify_signatures,
                );
                let simulation_section = SimulationSection::from_result(simulation);

                BundleTransactionReport { index, transaction, simulation: simulation_section }
            })
            .collect();

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

        let token_fundings = token_fundings
            .iter()
            .map(|entry| TokenFundingSection {
                account: entry.account.to_string(),
                mint: entry.mint.to_string(),
                decimals: entry.decimals,
                ui_amount: entry.ui_amount,
                amount_raw: entry.amount_raw,
            })
            .collect();

        // Compute overall bundle balance changes (first tx pre -> last successful tx post)
        let (sol_balance_changes, token_balance_changes) =
            if balance_opts.show_balance_change && !simulations.is_empty() {
                compute_bundle_overall_balance_changes(resolved, simulations, balance_opts)
            } else {
                (Vec::new(), Vec::new())
            };

        Self {
            transactions,
            replacements,
            fundings,
            token_fundings,
            sol_balance_changes,
            token_balance_changes,
        }
    }
}

impl Report {
    fn from_sources(
        parsed: &ParsedTransaction,
        resolved: &ResolvedAccounts,
        simulation: &SimulationResult,
        replacements: &[ProgramReplacement],
        fundings: &[Funding],
        token_fundings: &[PreparedTokenFunding],
        parser_registry: &mut ParserRegistry,
        verify_signatures: bool,
        balance_opts: BalanceChangeOptions,
    ) -> Self {
        let resolver = LookupResolver::new(resolved.lookup_details());
        let transaction = TransactionSection::from_sources(
            parsed,
            resolved,
            &resolver,
            parser_registry,
            verify_signatures,
        );
        let simulation_section = SimulationSection::from_result(simulation);
        let replacements = replacements
            .iter()
            .map(|entry| ReplacementSection {
                program_id: entry.program_id.to_string(),
                path: entry.so_path.display().to_string(),
            })
            .collect();
        // Compute balance changes before fundings is shadowed below
        let (sol_balance_changes, token_balance_changes) =
            if matches!(simulation.status, ExecutionStatus::Succeeded)
                && balance_opts.show_balance_change
            {
                compute_balance_changes_for_single_tx(resolved, simulation, fundings, balance_opts)
            } else {
                (Vec::new(), Vec::new())
            };

        let fundings = fundings
            .iter()
            .map(|entry| FundingSection {
                pubkey: entry.pubkey.to_string(),
                amount_sol: entry.amount_sol,
            })
            .collect();
        let token_fundings = token_fundings
            .iter()
            .map(|entry| TokenFundingSection {
                account: entry.account.to_string(),
                mint: entry.mint.to_string(),
                decimals: entry.decimals,
                ui_amount: entry.ui_amount,
                amount_raw: entry.amount_raw,
            })
            .collect();

        Self {
            transaction,
            simulation: simulation_section,
            replacements,
            fundings,
            token_fundings,
            sol_balance_changes,
            token_balance_changes,
        }
    }
}

/// Compute balance changes for single transaction mode.
/// Uses resolved.accounts as pre-state and simulation.post_accounts as post-state.
/// When SOL fundings are present, applies them to the pre-state so that the
/// balance change only reflects the transaction's effect, not the funding itself.
fn compute_balance_changes_for_single_tx(
    resolved: &ResolvedAccounts,
    simulation: &SimulationResult,
    fundings: &[Funding],
    balance_opts: BalanceChangeOptions,
) -> (Vec<SolBalanceChangeSection>, Vec<TokenBalanceChangeSection>) {
    let mut sol_changes = Vec::new();
    let mut token_changes = Vec::new();

    if balance_opts.show_balance_change {
        // Build pre_accounts with SOL fundings applied so pre/post baselines match.
        let funded_accounts;
        let pre_accounts = if fundings.is_empty() {
            &resolved.accounts
        } else {
            let mut accounts = resolved.accounts.clone();
            for funding in fundings {
                let lamports = (funding.amount_sol * 1_000_000_000.0) as u64;
                if let Some(account) = accounts.get_mut(&funding.pubkey) {
                    account.lamports = lamports;
                } else {
                    let system_program_id = solana_sdk_ids::system_program::id();
                    accounts.insert(
                        funding.pubkey,
                        solana_account::Account {
                            lamports,
                            owner: system_program_id,
                            ..Default::default()
                        },
                    );
                }
            }
            funded_accounts = accounts;
            &funded_accounts
        };

        let changes = compute_sol_changes(&pre_accounts, &simulation.post_accounts);
        sol_changes = changes
            .into_iter()
            .map(|c| SolBalanceChangeSection {
                account: c.account.to_string(),
                before: c.before,
                after: c.after,
                change: c.change,
                change_sol: c.change as f64 / 1_000_000_000.0,
            })
            .collect();

        let mint_decimals =
            extract_mint_decimals_combined(&pre_accounts, &simulation.post_accounts);
        let changes =
            compute_token_changes(&pre_accounts, &simulation.post_accounts, &mint_decimals);
        token_changes = changes
            .into_iter()
            .map(|c| {
                let divisor = 10f64.powi(c.decimals as i32);
                TokenBalanceChangeSection {
                    account: c.account.to_string(),
                    mint: c.mint.to_string(),
                    before: c.before,
                    after: c.after,
                    change: c.change,
                    decimals: c.decimals,
                    ui_change: c.change as f64 / divisor,
                }
            })
            .collect();
    }

    (sol_changes, token_changes)
}

/// Compute overall balance changes for the entire bundle.
/// Only computes when ALL transactions in the bundle succeeded.
/// Uses the first transaction's pre_accounts and the last transaction's post_accounts.
fn compute_bundle_overall_balance_changes(
    resolved: &ResolvedAccounts,
    simulations: &[SimulationResult],
    balance_opts: BalanceChangeOptions,
) -> (Vec<SolBalanceChangeSection>, Vec<TokenBalanceChangeSection>) {
    use solana_account::Account;

    if !balance_opts.show_balance_change || simulations.is_empty() {
        return (Vec::new(), Vec::new());
    }

    // Only compute balance changes if ALL transactions succeeded
    let all_succeeded =
        simulations.iter().all(|sim| matches!(sim.status, ExecutionStatus::Succeeded));
    if !all_succeeded {
        return (Vec::new(), Vec::new());
    }

    // Get pre_accounts from the first transaction
    let first_simulation = &simulations[0];
    let pre_accounts: HashMap<Pubkey, Account> =
        first_simulation.pre_accounts.iter().map(|(k, v)| (*k, Account::from(v.clone()))).collect();

    // Get post_accounts from the last transaction (all succeeded, so use the last one)
    let last_simulation = simulations.last().unwrap();
    let post_accounts = &last_simulation.post_accounts;

    // Compute SOL balance changes
    let sol_changes: Vec<SolBalanceChangeSection> =
        compute_sol_changes(&pre_accounts, post_accounts)
            .into_iter()
            .map(|c| SolBalanceChangeSection {
                account: c.account.to_string(),
                before: c.before,
                after: c.after,
                change: c.change,
                change_sol: c.change as f64 / 1_000_000_000.0,
            })
            .collect();

    // Extract mint decimals from both resolved accounts and post accounts
    let mint_decimals = extract_mint_decimals_combined(&resolved.accounts, post_accounts);
    let token_changes: Vec<TokenBalanceChangeSection> =
        compute_token_changes(&pre_accounts, post_accounts, &mint_decimals)
            .into_iter()
            .map(|c| {
                let divisor = 10f64.powi(c.decimals as i32);
                TokenBalanceChangeSection {
                    account: c.account.to_string(),
                    mint: c.mint.to_string(),
                    before: c.before,
                    after: c.after,
                    change: c.change,
                    decimals: c.decimals,
                    ui_change: c.change as f64 / divisor,
                }
            })
            .collect();

    (sol_changes, token_changes)
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
        parser_registry: &mut ParserRegistry,
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
        parser_registry: &mut ParserRegistry,
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

fn parse_inner_instruction_as_regular(
    inner_ix: &solana_message::inner_instruction::InnerInstruction,
    message: &solana_message::VersionedMessage,
    account_plan: &crate::transaction::MessageAccountPlan,
    lookup_locations: &[crate::transaction::LookupLocation],
    parser_registry: &mut ParserRegistry,
    program_id: &Pubkey,
) -> Option<ParsedInstruction> {
    let inner_accounts: Vec<crate::transaction::AccountReferenceSummary> = inner_ix
        .instruction
        .accounts
        .iter()
        .map(|account_index| {
            crate::transaction::classify_account_reference(
                message,
                *account_index as usize,
                account_plan,
                lookup_locations,
            )
        })
        .collect();

    let inner_summary = crate::transaction::InstructionSummary {
        index: 0, // Inner instruction index doesn't matter for parsing
        program: crate::transaction::AccountReferenceSummary {
            index: inner_ix.instruction.program_id_index as usize,
            pubkey: Some(program_id.to_string()),
            signer: false,
            writable: false,
            source: crate::transaction::AccountSourceSummary::Static,
        },
        accounts: inner_accounts,
        data: inner_ix.instruction.data.clone().into_boxed_slice(),
    };
    parser_registry.parse_instruction(&inner_summary, program_id)
}

impl InnerInstructionSection {
    fn from_inner_instruction(
        inner_ix: &solana_message::inner_instruction::InnerInstruction,
        resolver: &LookupResolver,
        label: &str,
        parsed: &ParsedTransaction,
        parser_registry: &mut ParserRegistry,
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

        let accounts: Vec<InstructionAccountEntry> = inner_ix
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
            // First check if this is a CPI event
            let temp_summary = crate::transaction::InstructionSummary {
                index: 0,
                program: crate::transaction::AccountReferenceSummary {
                    index: inner_ix.instruction.program_id_index as usize,
                    pubkey: Some(program_id.to_string()),
                    signer: false,
                    writable: false,
                    source: crate::transaction::AccountSourceSummary::Static,
                },
                accounts: Vec::new(), // Not needed for CPI event detection
                data: inner_ix.instruction.data.clone().into_boxed_slice(),
            };

            // Check for CPI event first
            if is_anchor_cpi_event(&temp_summary) {
                // Try to parse as CPI event
                let cpi_result = parser_registry.parse_cpi_event(
                    &temp_summary,
                    &program_id,
                    message,
                    &parsed.account_plan,
                    &lookup_locations,
                );
                log::debug!(
                    "CPI event parse result for {}: {:?}",
                    program_id,
                    cpi_result.is_some()
                );
                cpi_result
            } else {
                // Regular instruction parsing
                // Try to load IDL parser if needed
                if let Err(err) = parser_registry.load_idl_parser_if_needed(&program_id) {
                    log::debug!("Failed to load IDL parser for {}: {}", program_id, err);
                }
                parse_inner_instruction_as_regular(
                    &inner_ix,
                    message,
                    &parsed.account_plan,
                    &lookup_locations,
                    parser_registry,
                    &program_id,
                )
            }
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
struct TokenFundingSection {
    account: String,
    mint: String,
    decimals: u8,
    ui_amount: f64,
    amount_raw: u64,
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

/// Check if the terminal supports OSC 8 hyperlinks.
fn supports_hyperlinks() -> bool {
    // iTerm2, WezTerm, VSCode integrated terminal
    if let Ok(term) = std::env::var("TERM_PROGRAM") {
        if term.contains("iTerm")
            || term.contains("WezTerm")
            || term.contains("vscode")
            || term.contains("Apple_Terminal")
        {
            return true;
        }
    }
    // Windows Terminal
    if std::env::var("WT_SESSION").is_ok() {
        return true;
    }
    // VTE-based terminals (GNOME Terminal, etc.)
    if std::env::var("VTE_VERSION").is_ok() {
        return true;
    }
    false
}

fn format_solscan_link(account_pubkey: &str) -> String {
    if supports_hyperlinks() {
        let solscan_url = format!("https://solscan.io/account/{}", account_pubkey);
        format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", solscan_url, account_pubkey)
    } else {
        account_pubkey.to_string()
    }
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

fn render_parsed_fields(fields: &[ParsedField]) {
    if fields.is_empty() {
        return;
    }

    struct OrderedFields<'a>(&'a [ParsedField]);

    impl Serialize for OrderedFields<'_> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            use serde::ser::SerializeMap;

            let mut map = serializer.serialize_map(Some(self.0.len()))?;
            for field in self.0 {
                map.serialize_entry(&field.name, &field.value)?;
            }
            map.end()
        }
    }

    let ordered = OrderedFields(fields);
    let pretty = serde_json::to_string_pretty(&ordered).unwrap_or_else(|_| "{}".to_string());

    for line in pretty.lines() {
        println!("{}", format!("    {}", line).custom_color((255, 255, 224)));
    }
}

fn render_inner_parsed_fields(fields: &[ParsedField]) {
    if fields.is_empty() {
        return;
    }

    struct OrderedFields<'a>(&'a [ParsedField]);

    impl Serialize for OrderedFields<'_> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            use serde::ser::SerializeMap;

            let mut map = serializer.serialize_map(Some(self.0.len()))?;
            for field in self.0 {
                map.serialize_entry(&field.name, &field.value)?;
            }
            map.end()
        }
    }

    let ordered = OrderedFields(fields);
    let pretty = serde_json::to_string_pretty(&ordered).unwrap_or_else(|_| "{}".to_string());

    for line in pretty.lines() {
        println!("{}", format!("      {}", line).custom_color((255, 255, 224)));
    }
}
