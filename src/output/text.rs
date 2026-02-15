use std::str::FromStr;

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;
use solana_pubkey::Pubkey;
use unicode_width::UnicodeWidthStr;

use crate::{
    account_loader::ResolvedAccounts,
    instruction_parsers::ParsedField,
    instruction_parsers::ParserRegistry,
    log_parser::{LogEntry, LogEntryWithDepth, parse_logs_by_instruction},
};

use super::LogDisplayOptions;
use super::report::{
    BundleReport, BundleTransactionReport, InstructionAccountEntry, Report, SimulationSection,
    SimulationStatusReport, SolBalanceChangeSection, TokenBalanceChangeSection, TransactionSection,
};

/// Single indentation unit (2 spaces).
const INDENT: &str = "  ";
/// Indentation for outer items (level 1 = 2 spaces).
const INDENT_L1: &str = INDENT;
/// Indentation for inner items (level 2 = 4 spaces).
const INDENT_L2: &str = "    ";
/// Indentation for deeply nested items (level 3 = 6 spaces).
const INDENT_L3: &str = "      ";

/// Get effective terminal width for text rendering.
/// Falls back to 80 when width detection is unavailable.
fn terminal_width() -> usize {
    terminal_size::terminal_size()
        .map(|(width, _)| (width.0 as usize).clamp(60, 120))
        .unwrap_or(80)
}

/// Header content width with one-space side margins.
fn header_content_width() -> usize {
    terminal_width().saturating_sub(2).max(1)
}

pub(super) fn render_text(
    report: &Report,
    resolved: &ResolvedAccounts,
    _parser_registry: &mut ParserRegistry,
    show_ix_data: bool,
    show_ix_detail: bool,
    log_opts: LogDisplayOptions,
) -> Result<()> {
    // Summary header (status + CU)
    render_summary_header(&report.simulation, &report.transaction);

    // Execution Trace
    if !report.simulation.logs.is_empty() {
        render_section_title("Execution Trace");
        render_execution_trace_section(&report.simulation, log_opts);
    }

    // Instruction Details
    if show_ix_detail {
        render_section_title("Instruction Details");
        render_instruction_details_text(&report.transaction, resolved, show_ix_data);
    }

    // Balance Changes
    if !report.sol_balance_changes.is_empty() || !report.token_balance_changes.is_empty() {
        render_section_title("Balance Changes");
        render_balance_changes_text(&report.sol_balance_changes, &report.token_balance_changes);
    }

    // Final empty line
    println!();

    Ok(())
}

pub(super) fn render_bundle_text(
    bundle: &BundleReport,
    total_count: usize,
    resolved: &ResolvedAccounts,
    show_ix_data: bool,
    show_ix_detail: bool,
    log_opts: LogDisplayOptions,
) -> Result<()> {
    // Bundle summary header (status + per-TX overview)
    render_bundle_summary_header(bundle, total_count);

    // Execution Trace (per-TX)
    let has_logs = bundle.transactions.iter().any(|t| !t.simulation.logs.is_empty());
    if has_logs {
        for (i, tx_report) in bundle.transactions.iter().enumerate() {
            render_section_title(&format!("Execution Trace: TX {}/{}", i + 1, total_count));
            render_bundle_transaction_trace(tx_report, log_opts);
        }
    }

    // Instruction Details (per-TX)
    if show_ix_detail {
        for (i, tx_report) in bundle.transactions.iter().enumerate() {
            render_section_title(&format!("Instruction Details: TX {}/{}", i + 1, total_count));
            render_bundle_transaction_ix_detail(tx_report, resolved, show_ix_data);
        }
    }

    // Balance Changes
    if !bundle.sol_balance_changes.is_empty() || !bundle.token_balance_changes.is_empty() {
        render_section_title("Balance Changes");
        render_bundle_balance_changes(bundle);
    }

    println!();

    Ok(())
}

pub(super) fn render_transaction_section_text(
    transaction: &TransactionSection,
    resolved: &ResolvedAccounts,
    _parser_registry: &mut ParserRegistry,
    show_ix_data: bool,
    bundle_info: Option<(usize, usize)>,
) {
    // Build TX suffix for bundle mode, e.g. ": TX 1/3"
    let tx_suffix = match bundle_info {
        Some((index, total)) => format!(": TX {}/{}", index, total),
        None => String::new(),
    };

    render_section_title(&format!("Decoded Instructions{}", tx_suffix));
    render_instruction_details_text(transaction, resolved, show_ix_data);

    // Address Lookup Tables (only if present)
    if !transaction.lookups.is_empty() {
        render_section_title(&format!("Address Lookup Tables{}", tx_suffix));
        render_lookup_tables_text(transaction);
    }

    // Account list at the end
    render_section_title(&format!("Account List{}", tx_suffix));
    render_account_list_text(transaction, resolved);
}

/// Render the bundle summary header showing overall status and per-transaction compact rows.
fn render_bundle_summary_header(bundle: &BundleReport, total_count: usize) {
    // Determine overall bundle status
    let succeeded = bundle
        .transactions
        .iter()
        .filter(|t| matches!(t.simulation.status, SimulationStatusReport::Succeeded))
        .count();
    let failed_at = bundle
        .transactions
        .iter()
        .position(|t| matches!(t.simulation.status, SimulationStatusReport::Failed { .. }))
        .map(|i| i + 1);

    let status_str = if succeeded == total_count {
        "🟢 ALL SUCCEEDED".to_string()
    } else if let Some(idx) = failed_at {
        format!("🔴 FAILED (TX {})", idx)
    } else {
        "⚠️  PARTIAL".to_string()
    };

    // Total CU consumed across all transactions
    let total_cu: u64 =
        bundle.transactions.iter().map(|t| t.simulation.compute_units_consumed).sum();

    let summary_text = format!(
        "Bundle: {} | TX: {}/{} | CU: {}",
        status_str,
        bundle.transactions.len(),
        total_count,
        format_with_commas(total_cu)
    );
    render_section_title(&summary_text);
    println!();

    let tx_col_width = total_count.to_string().len().max(2);
    const CU_COL_WIDTH: usize = 12;

    // Per-transaction compact rows
    for (i, tx_report) in bundle.transactions.iter().enumerate() {
        let idx = i + 1;
        let status_icon = match &tx_report.simulation.status {
            SimulationStatusReport::Succeeded => "🟢",
            SimulationStatusReport::Failed { .. } => "🔴",
        };
        let cu_used = tx_report.simulation.compute_units_consumed;
        let cu_limit = extract_compute_unit_limit(&tx_report.transaction).unwrap_or(200_000);
        let percentage =
            if cu_limit > 0 { (cu_used as f64 / cu_limit as f64 * 100.0) as u32 } else { 0 };
        let sig = tx_report
            .transaction
            .signatures
            .first()
            .map(|s| truncate_sig(s, 6))
            .unwrap_or_else(|| "<no-sig>".to_string());

        println!(
            "{}TX {:>tx_w$}/{:<tx_w$}  {}  CU: {:>cu_w$} / {:>cu_w$} ({:>3}%)  {}",
            INDENT_L1,
            idx,
            total_count,
            status_icon,
            format_with_commas(cu_used),
            format_with_commas(cu_limit),
            percentage,
            sig,
            tx_w = tx_col_width,
            cu_w = CU_COL_WIDTH
        );
    }

    // Render skipped transactions
    for i in bundle.transactions.len()..total_count {
        println!(
            "{}TX {:>tx_w$}/{:<tx_w$}  ⏭️  SKIPPED",
            INDENT_L1,
            i + 1,
            total_count,
            tx_w = tx_col_width
        );
    }

}

fn render_bundle_transaction_trace(
    tx_report: &BundleTransactionReport,
    log_opts: LogDisplayOptions,
) {
    if let SimulationStatusReport::Failed { error } = &tx_report.simulation.status {
        println!("{}↳ Error: {}", INDENT_L1, error);
    }
    render_execution_trace_section(&tx_report.simulation, log_opts);
}

fn render_bundle_transaction_ix_detail(
    tx_report: &BundleTransactionReport,
    resolved: &ResolvedAccounts,
    show_ix_data: bool,
) {
    render_instruction_details_text(&tx_report.transaction, resolved, show_ix_data);
}

/// Render overall bundle balance changes (first tx pre -> last successful tx post)
fn render_bundle_balance_changes(bundle: &BundleReport) {
    println!();

    if !bundle.sol_balance_changes.is_empty() {
        for change in &bundle.sol_balance_changes {
            let sol_before = change.before as f64 / 1_000_000_000.0;
            let sol_after = change.after as f64 / 1_000_000_000.0;
            let sign = if change.change >= 0 { "+" } else { "" };
            let color = if change.change >= 0 { (152, 195, 121) } else { (224, 108, 117) };
            println!(
                "{}{} {} | {} | {}",
                INDENT_L1,
                change.account.cyan(),
                format!("{:.9}", sol_before).custom_color((171, 178, 191)),
                format!("{:.9}", sol_after).custom_color((171, 178, 191)),
                format!("{}{:.9}", sign, change.change_sol).custom_color(color)
            );
        }
    }

    println!();

    if !bundle.token_balance_changes.is_empty() {
        for change in &bundle.token_balance_changes {
            let divisor = 10f64.powi(change.decimals as i32);
            let ui_before = change.before as f64 / divisor;
            let ui_after = change.after as f64 / divisor;
            let sign = if change.change >= 0 { "+" } else { "" };
            let color = if change.change >= 0 { (152, 195, 121) } else { (224, 108, 117) };
            println!(
                "{}{} ({}) {} | {} | {}",
                INDENT_L1,
                change.account.cyan(),
                change.mint.custom_color((139, 170, 214)),
                format!("{:.prec$}", ui_before, prec = change.decimals as usize)
                    .custom_color((171, 178, 191)),
                format!("{:.prec$}", ui_after, prec = change.decimals as usize)
                    .custom_color((171, 178, 191)),
                format!("{}{:.prec$}", sign, change.ui_change, prec = change.decimals as usize)
                    .custom_color(color),
            );
        }
    }
}

/// Render a section title with centered text flanked by `─` lines.
fn render_section_title(title: &str) {
    let width = header_content_width();
    let title_with_padding = format!(" {} ", title);
    let title_len = UnicodeWidthStr::width(title_with_padding.as_str());
    let remaining = width.saturating_sub(title_len);
    let left = remaining / 2;
    let right = remaining - left;
    println!();
    println!(
        " {}{}{} ",
        "─".repeat(left).dimmed(),
        title_with_padding.dimmed(),
        "─".repeat(right).dimmed(),
    );
}

/// Render the summary header showing status and compute units (displayed first).
fn render_summary_header(simulation: &SimulationSection, transaction: &TransactionSection) {
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
        "{} | CU: {} / {} ({}%)",
        status_str,
        format_with_commas(cu_used),
        format_with_commas(cu_limit),
        percentage
    );
    render_section_title(&result_text);
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

    if log_opts.raw_log {
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
    println!();

    // SOL balance changes first
    for change in sol_changes {
        let sol_before = change.before as f64 / 1_000_000_000.0;
        let sol_after = change.after as f64 / 1_000_000_000.0;
        let sign = if change.change >= 0 { "+" } else { "" };
        let color = if change.change >= 0 { (152, 195, 121) } else { (224, 108, 117) };

        println!(
            "{} {} | {} | {}",
            change.account.cyan(),
            format!("{:.9}", sol_before).custom_color((171, 178, 191)),
            format!("{:.9}", sol_after).custom_color((171, 178, 191)),
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
        let color = if change.change >= 0 { (152, 195, 121) } else { (224, 108, 117) };

        println!(
            "{} ({}) {} | {} | {}",
            change.account.cyan(),
            change.mint.custom_color((139, 170, 214)),
            format!("{:.prec$}", ui_before, prec = change.decimals as usize)
                .custom_color((171, 178, 191)),
            format!("{:.prec$}", ui_after, prec = change.decimals as usize)
                .custom_color((171, 178, 191)),
            format!("{}{:.prec$}", sign, change.ui_change, prec = change.decimals as usize)
                .custom_color(color),
        );
    }
}

fn render_lookup_tables_text(transaction: &TransactionSection) {
    if transaction.lookups.is_empty() {
        return;
    }
    println!();

    for (idx, lookup) in transaction.lookups.iter().enumerate() {
        let solscan_linked_key = format_solscan_link(&lookup.account_key);
        println!("{}[{}] {}", INDENT_L1, idx, solscan_linked_key);
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
        "{}[{}] {} {}",
        INDENT_L1,
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
    println!();

    for ix in &transaction.instructions {
        let program_pubkey_with_link = format_solscan_link(&ix.program.pubkey);
        // Display outer instruction with 1-based indexing (#1, #2, #3, etc.)
        let outer_number = ix.index + 1;

        // Try to parse the instruction
        if let Some(parsed) = &ix.parsed {
            println!(
                "#{} {} [{}]",
                outer_number.to_string().custom_color((229, 192, 123)),
                program_pubkey_with_link.cyan(),
                parsed.name.custom_color((152, 195, 121))
            );

            // Render accounts with parsed names
            for (i, account) in ix.accounts.iter().enumerate() {
                let account_name = if i < parsed.account_names.len() {
                    parsed.account_names[i].clone()
                } else {
                    format!("account_{}", i + 1)
                };
                render_instruction_account_text(account, resolved, Some(&account_name), INDENT_L1);
            }

            // Display raw instruction data only when requested
            if show_ix_data {
                println!("{}🔢 0x{} | {} byte(s)", INDENT_L1, hex::encode(&ix.data), ix.data.len());
            }

            // Then render parsed fields as formatted JSON, preserving original order
            render_parsed_fields(&parsed.fields, INDENT_L2);
        } else {
            println!(
                "#{} {}",
                outer_number.to_string().custom_color((229, 192, 123)),
                program_pubkey_with_link.cyan()
            );

            for account in &ix.accounts {
                render_instruction_account_text(account, resolved, None, INDENT_L1);
            }
            println!("{}🔢 0x{} | {} byte(s)", INDENT_L1, hex::encode(&ix.data), ix.data.len());
        }

        // Display inner instructions if any
        if !ix.inner_instructions.is_empty() {
            for inner_ix in &ix.inner_instructions {
                // Try to parse inner instruction
                if let Some(parsed_inner) = &inner_ix.parsed {
                    println!(
                        "{}{} {} [{}]",
                        INDENT_L1,
                        format!("#{}", inner_ix.label).custom_color((229, 192, 123)),
                        format_solscan_link(&inner_ix.program.pubkey).cyan(),
                        parsed_inner.name.custom_color((152, 195, 121))
                    );

                    // Render accounts with parsed names
                    for (i, account) in inner_ix.accounts.iter().enumerate() {
                        let account_name = if i < parsed_inner.account_names.len() {
                            parsed_inner.account_names[i].clone()
                        } else {
                            format!("account_{}", i + 1)
                        };
                        render_instruction_account_text(
                            account,
                            resolved,
                            Some(&account_name),
                            INDENT_L2,
                        );
                    }

                    // Display raw instruction data only when requested
                    if show_ix_data {
                        println!(
                            "{}🔢 0x{} | {} byte(s)",
                            INDENT_L2,
                            hex::encode(&inner_ix.data),
                            inner_ix.data.len()
                        );
                    }

                    // Then render parsed fields as formatted JSON, preserving original order
                    render_parsed_fields(&parsed_inner.fields, INDENT_L3);
                } else {
                    println!(
                        "{}{} {}",
                        INDENT_L1,
                        format!("#{}", inner_ix.label).custom_color((229, 192, 123)),
                        format_solscan_link(&inner_ix.program.pubkey).cyan()
                    );

                    for account in &inner_ix.accounts {
                        render_instruction_account_text(account, resolved, None, INDENT_L2);
                    }
                    println!(
                        "{}🔢 0x{} | {} byte(s)",
                        INDENT_L2,
                        hex::encode(&inner_ix.data),
                        inner_ix.data.len()
                    );
                }
            }
        }
    }
}

fn render_instruction_account_text(
    account: &InstructionAccountEntry,
    resolved: &ResolvedAccounts,
    name: Option<&str>,
    indent: &str,
) {
    let solscan_linked_pubkey = format_solscan_link(&account.pubkey);
    let executable = if let Ok(pubkey) = Pubkey::from_str(&account.pubkey) {
        resolved.accounts.get(&pubkey).map(|acc| acc.executable).unwrap_or(false)
    } else {
        false
    };
    let name_suffix = match name {
        Some(n) => format!(" ({})", n.custom_color((139, 170, 214))),
        None => String::new(),
    };
    println!(
        "{}{} [{}] {} {}{}",
        indent,
        account.source,
        account.index,
        solscan_linked_pubkey,
        account_privilege_emoji(account.signer, account.writable, executable),
        name_suffix
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
    let indent = INDENT.repeat(depth);

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

fn truncate_sig(sig: &str, prefix_len: usize) -> String {
    if sig.len() <= prefix_len * 2 + 3 {
        sig.to_string()
    } else {
        format!("{}...{}", &sig[..prefix_len], &sig[sig.len() - prefix_len..])
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

fn render_parsed_fields(fields: &[ParsedField], indent: &str) {
    if fields.is_empty() {
        return;
    }

    let ordered = OrderedFields(fields);
    let pretty = serde_json::to_string_pretty(&ordered).unwrap_or_else(|_| "{}".to_string());

    for line in pretty.lines() {
        println!("{}", format!("{}{}", indent, line).custom_color((171, 178, 191)));
    }
}
