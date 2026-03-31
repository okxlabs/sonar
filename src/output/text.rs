use std::str::FromStr;

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;
use serde::ser::SerializeMap;
use solana_account::ReadableAccount;
use solana_pubkey::Pubkey;
use unicode_width::UnicodeWidthStr;

use crate::parsers::{
    instruction::{OrderedJsonValue, ParsedField, ParsedFieldValue, ParserRegistry},
    log_parser::{LogEntry, LogEntryWithDepth, parse_logs_by_instruction},
};
use sonar_sim::ResolvedAccounts;

use super::LogDisplayOptions;
use super::report::{
    BundleReport, BundleTransactionReport, InstructionAccountEntry, Report, SimulationSection,
    SimulationStatusReport, SolBalanceChangeSection, TokenBalanceChangeSection, TransactionSection,
};
use super::terminal::render_section_title;

/// Single indentation unit (2 spaces).
const INDENT: &str = "  ";
/// Indentation for outer items (level 1 = 2 spaces).
const INDENT_L1: &str = INDENT;
/// Indentation for inner items (level 2 = 4 spaces).
const INDENT_L2: &str = "    ";

/// Subdued gray for metadata columns (index labels, permission flags, account names).
const DIM_GRAY: colored::CustomColor = colored::CustomColor { r: 128, g: 128, b: 128 };

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

    // Execution Trace (also show when failed without runtime logs)
    if !report.simulation.logs.is_empty()
        || matches!(&report.simulation.status, SimulationStatusReport::Failed { .. })
    {
        render_section_title("Execution Trace");
        render_execution_trace_section(&report.simulation, log_opts);
    }

    // Instruction Details
    if show_ix_detail {
        render_section_title("Instruction Details");
        render_instruction_details_text(&report.transaction, resolved, show_ix_data);
    }

    // SOL Balance Changes
    if !report.sol_balance_changes.is_empty() {
        render_section_title("SOL Balance Changes");
        render_sol_balance_changes(&report.sol_balance_changes, "");
    }

    // Token Balance Changes
    if !report.token_balance_changes.is_empty() {
        render_section_title("Token Balance Changes");
        render_token_balance_changes(&report.token_balance_changes, "");
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

    // Execution Trace (per-TX): render TX with logs, or failed TX without logs.
    let has_logs_or_failed = bundle.transactions.iter().any(|t| {
        !t.simulation.logs.is_empty()
            || matches!(&t.simulation.status, SimulationStatusReport::Failed { .. })
    });
    if has_logs_or_failed {
        for (i, tx_report) in bundle.transactions.iter().enumerate() {
            let should_render = !tx_report.simulation.logs.is_empty()
                || matches!(&tx_report.simulation.status, SimulationStatusReport::Failed { .. });
            if !should_render {
                continue;
            }
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

    // SOL Balance Changes
    if !bundle.sol_balance_changes.is_empty() {
        render_section_title("SOL Balance Changes");
        render_sol_balance_changes(&bundle.sol_balance_changes, INDENT_L1);
    }

    // Token Balance Changes
    if !bundle.token_balance_changes.is_empty() {
        render_section_title("Token Balance Changes");
        render_token_balance_changes(&bundle.token_balance_changes, INDENT_L1);
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

    // Final empty line for consistent CLI output termination.
    println!();
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
        "✓ ALL SUCCEEDED".to_string()
    } else if let Some(idx) = failed_at {
        format!("✗ FAILED (TX {})", idx)
    } else {
        "~  PARTIAL".to_string()
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

    let tx_col_width = total_count.to_string().len().max(2);
    const CU_COL_WIDTH: usize = 12;

    // Per-transaction compact rows
    for (i, tx_report) in bundle.transactions.iter().enumerate() {
        let idx = i + 1;
        let status_icon = match &tx_report.simulation.status {
            SimulationStatusReport::Succeeded => "✓",
            SimulationStatusReport::Failed { .. } => "✗",
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
            "{}TX {:>tx_w$}/{:<tx_w$}  »  SKIPPED",
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
    render_execution_trace_section(&tx_report.simulation, log_opts);
}

fn render_bundle_transaction_ix_detail(
    tx_report: &BundleTransactionReport,
    resolved: &ResolvedAccounts,
    show_ix_data: bool,
) {
    render_instruction_details_text(&tx_report.transaction, resolved, show_ix_data);
}

/// Render the summary header showing status and compute units (displayed first).
fn render_summary_header(simulation: &SimulationSection, transaction: &TransactionSection) {
    // For failed transactions, don't print the error reason
    let status_str = match &simulation.status {
        SimulationStatusReport::Succeeded => "✓ SUCCESS".to_string(),
        SimulationStatusReport::Failed { .. } => "✗ FAILED".to_string(),
    };

    // Try to extract compute unit limit from SetComputeUnitLimit instruction
    let cu_limit = extract_compute_unit_limit(transaction).unwrap_or(200_000);
    let cu_used = simulation.compute_units_consumed;
    let percentage =
        if cu_limit > 0 { (cu_used as f64 / cu_limit as f64 * 100.0) as u32 } else { 0 };

    let result_text = format!(
        "{} | CU: {} / {} ({}%) | Size: {} bytes",
        status_str,
        format_with_commas(cu_used),
        format_with_commas(cu_limit),
        percentage,
        transaction.size_bytes
    );
    render_section_title(&result_text);
}

/// Extract compute unit limit from SetComputeUnitLimit instruction if present.
fn extract_compute_unit_limit(transaction: &TransactionSection) -> Option<u64> {
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
        render_no_trace_hint(simulation, INDENT_L1);
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

fn render_no_trace_hint(simulation: &SimulationSection, indent: &str) {
    if let SimulationStatusReport::Failed { error } = &simulation.status {
        println!("{}↳ Failed before program invocation: {}", indent, error);
    }
}

fn render_sol_balance_changes(sol_changes: &[SolBalanceChangeSection], indent: &str) {
    let rows: Vec<_> = sol_changes
        .iter()
        .map(|c| {
            let sol_before = c.before as f64 / 1_000_000_000.0;
            let sol_after = c.after as f64 / 1_000_000_000.0;
            let sign = if c.change >= 0 { "+" } else { "" };
            let col_delta = format!("{}{:.9}", sign, c.change_sol);
            let col_before = format!("{:.9}", sol_before);
            let col_after = format!("{:.9}", sol_after);
            let color = if c.change >= 0 { (152, 195, 121) } else { (224, 108, 117) };
            (c.account.as_str(), col_delta, col_before, col_after, color)
        })
        .collect();

    let w_account = rows.iter().map(|r| r.0.len()).max().unwrap_or(0);
    let w_delta = rows.iter().map(|r| r.1.len()).max().unwrap_or(0);
    let w_before = rows.iter().map(|r| r.2.len()).max().unwrap_or(0);

    for (col_account, col_delta, col_before, col_after, color) in &rows {
        println!(
            "{}{}{:<wa$}  {}  {}",
            indent,
            INDENT_L1,
            col_account,
            format!("{:<width$}", col_delta, width = w_delta).custom_color(*color),
            format!("{:<wb$} → {}", col_before, col_after, wb = w_before)
                .custom_color((171, 178, 191)),
            wa = w_account,
        );
    }

    println!(
        "\n{}",
        "  account  ±Δ(SOL)  before → after | (+) increase  (-) decrease".custom_color(DIM_GRAY)
    );
}

fn render_token_balance_changes(token_changes: &[TokenBalanceChangeSection], indent: &str) {
    let rows: Vec<_> = token_changes
        .iter()
        .map(|c| {
            let divisor = 10f64.powi(c.decimals as i32);
            let ui_before = c.before as f64 / divisor;
            let ui_after = c.after as f64 / divisor;
            let prec = c.decimals as usize;
            let sign = if c.change >= 0 { "+" } else { "" };
            let col_before = format!("{:.prec$}", ui_before, prec = prec);
            let col_after = format!("{:.prec$}", ui_after, prec = prec);
            let col_delta = format!("{}{:.prec$}", sign, c.ui_change, prec = prec);
            let color = if c.change >= 0 { (152, 195, 121) } else { (224, 108, 117) };
            (
                c.token_account.as_str(),
                c.owner.as_str(),
                col_before,
                col_after,
                col_delta,
                c.mint.as_str(),
                color,
            )
        })
        .collect();

    let w_ta = rows.iter().map(|r| r.0.len()).max().unwrap_or(0);
    let w_owner = rows.iter().map(|r| r.1.len()).max().unwrap_or(0);
    let w_before = rows.iter().map(|r| r.2.len()).max().unwrap_or(0);
    let w_after = rows.iter().map(|r| r.3.len()).max().unwrap_or(0);
    let w_delta = rows.iter().map(|r| r.4.len()).max().unwrap_or(0);

    for (col_ta, col_owner, col_before, col_after, col_delta, col_mint, color) in &rows {
        println!(
            "{}{}{:<wt$}  {:<wo$}  {}  {}  {}",
            indent,
            INDENT_L1,
            col_ta,
            col_owner,
            format!("{:<wd$}", col_delta, wd = w_delta).custom_color(*color),
            format!("{:<wb$} → {:<wa$}", col_before, col_after, wb = w_before, wa = w_after)
                .custom_color((171, 178, 191)),
            col_mint,
            wt = w_ta,
            wo = w_owner,
        );
    }

    println!(
        "\n{}",
        "  token-account  owner  ±Δ(amount)  before → after  mint | (+) increase  (-) decrease"
            .custom_color(DIM_GRAY)
    );
}

fn render_lookup_tables_text(transaction: &TransactionSection) {
    if transaction.lookups.is_empty() {
        return;
    }
    let index_width = index_label_width(transaction.lookups.len().saturating_sub(1));

    for (idx, lookup) in transaction.lookups.iter().enumerate() {
        println!(
            "{}{} {}",
            INDENT_L1,
            render_account_index_label(idx, index_width),
            lookup.account_key
        );
    }
}

fn render_account_list_text(transaction: &TransactionSection, resolved: &ResolvedAccounts) {
    let mut account_index = 0;
    let layout = account_list_layout(transaction);

    for account in &transaction.static_accounts {
        account_index = render_account_entry_text(
            account_index,
            &account.pubkey,
            account.signer,
            account.writable,
            &layout,
            resolved,
            None,
        );
    }

    for (table_idx, lookup) in transaction.lookups.iter().enumerate() {
        for entry in &lookup.writable {
            account_index = render_account_entry_text(
                account_index,
                &entry.pubkey,
                false,
                true,
                &layout,
                resolved,
                Some((table_idx, entry.index)),
            );
        }
    }

    for (table_idx, lookup) in transaction.lookups.iter().enumerate() {
        for entry in &lookup.readonly {
            account_index = render_account_entry_text(
                account_index,
                &entry.pubkey,
                false,
                false,
                &layout,
                resolved,
                Some((table_idx, entry.index)),
            );
        }
    }
}

fn render_account_entry_text(
    index: usize,
    pubkey_str: &str,
    signer: bool,
    writable: bool,
    layout: &ColumnLayout,
    resolved: &ResolvedAccounts,
    lookup_info: Option<(usize, u8)>,
) -> usize {
    let pubkey = Pubkey::from_str(pubkey_str).unwrap();
    let executable = resolved.accounts.get(&pubkey).map(|acc| acc.executable()).unwrap_or(false);
    let index_label = render_account_index_label(index, layout.index_width);
    let marker = render_account_marker(signer, writable, executable);
    let pubkey_display = format!("{:<width$}", pubkey_str, width = layout.pubkey_width);
    let lookup_suffix = match lookup_info {
        Some((table_idx, table_inner_idx)) => {
            format!("  ALT[{}] #{}", table_idx, table_inner_idx).custom_color(DIM_GRAY).to_string()
        }
        None => String::new(),
    };
    println!("{}{} {} {}{}", INDENT_L1, index_label, pubkey_display, marker, lookup_suffix);
    index + 1
}

/// Render instruction details section with centered header.
fn render_instruction_details_text(
    transaction: &TransactionSection,
    resolved: &ResolvedAccounts,
    show_ix_data: bool,
) {
    let layout = instruction_account_layout(transaction);

    let data_indent = " ".repeat(INDENT_L1.len() + layout.index_width + 3);
    let inner_data_indent = " ".repeat(INDENT_L2.len() + layout.index_width + 3);

    for (ix_pos, ix) in transaction.instructions.iter().enumerate() {
        if ix_pos > 0 {
            println!();
        }
        let program_pubkey = ix.program.pubkey.as_str();
        // Display outer instruction with 1-based indexing (#1, #2, #3, etc.)
        let outer_number = ix.index + 1;

        let current_data_indent = if ix.accounts.is_empty() {
            " ".repeat(1 + outer_number.to_string().len() + 1)
        } else {
            data_indent.clone()
        };

        // Try to parse the instruction
        if let Some(parsed) = &ix.parsed {
            println!(
                "#{} {} {}",
                outer_number.to_string().custom_color((229, 192, 123)),
                program_pubkey.cyan(),
                format!("[{}]", parsed.name).custom_color((152, 195, 121))
            );

            // Render accounts with parsed names
            for (i, account) in ix.accounts.iter().enumerate() {
                let account_name = if i < parsed.account_names.len() {
                    parsed.account_names[i].clone()
                } else {
                    format!("account_{}", i + 1)
                };
                render_instruction_account_text(
                    account,
                    resolved,
                    Some(&account_name),
                    INDENT_L1,
                    &layout,
                );
            }

            if show_ix_data {
                render_instruction_data_text(&current_data_indent, &ix.data);
            }

            render_parsed_fields(&parsed.fields, &current_data_indent);
        } else {
            println!(
                "#{} {}",
                outer_number.to_string().custom_color((229, 192, 123)),
                program_pubkey.cyan()
            );

            for account in &ix.accounts {
                render_instruction_account_text(account, resolved, None, INDENT_L1, &layout);
            }
            render_instruction_data_text(&current_data_indent, &ix.data);
        }

        if !ix.inner_instructions.is_empty() {
            for inner_ix in &ix.inner_instructions {
                let current_inner_data_indent = if inner_ix.accounts.is_empty() {
                    " ".repeat(INDENT_L1.len() + 1 + inner_ix.label.len() + 1)
                } else {
                    inner_data_indent.clone()
                };

                if let Some(parsed_inner) = &inner_ix.parsed {
                    println!(
                        "{}{} {} {}",
                        INDENT_L1,
                        format!("#{}", inner_ix.label).custom_color((229, 192, 123)),
                        inner_ix.program.pubkey.as_str().cyan(),
                        format!("[{}]", parsed_inner.name).custom_color((152, 195, 121))
                    );

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
                            &layout,
                        );
                    }

                    if show_ix_data {
                        render_instruction_data_text(&current_inner_data_indent, &inner_ix.data);
                    }

                    render_parsed_fields(&parsed_inner.fields, &current_inner_data_indent);
                } else {
                    println!(
                        "{}{} {}",
                        INDENT_L1,
                        format!("#{}", inner_ix.label).custom_color((229, 192, 123)),
                        inner_ix.program.pubkey.as_str().cyan()
                    );

                    for account in &inner_ix.accounts {
                        render_instruction_account_text(
                            account, resolved, None, INDENT_L2, &layout,
                        );
                    }
                    render_instruction_data_text(&current_inner_data_indent, &inner_ix.data);
                }
            }
        }
    }

    let legend = render_account_legend(transaction);
    println!("\n{}", legend.custom_color(DIM_GRAY));
}

fn render_account_legend(transaction: &TransactionSection) -> String {
    let static_count = transaction.static_accounts.len();
    let lookup_count: usize =
        transaction.lookups.iter().map(|l| l.writable.len() + l.readonly.len()).sum();

    let mut legend = format!(
        "  s=signer w=writable r=readonly x=executable | [0..{}] static",
        static_count.saturating_sub(1)
    );
    if lookup_count > 0 {
        let lookup_end = static_count + lookup_count - 1;
        legend.push_str(&format!(" [{}..{}] lookup", static_count, lookup_end));
    }
    legend
}

fn render_instruction_account_text(
    account: &InstructionAccountEntry,
    resolved: &ResolvedAccounts,
    name: Option<&str>,
    indent: &str,
    layout: &ColumnLayout,
) {
    let executable = if let Ok(pubkey) = Pubkey::from_str(&account.pubkey) {
        resolved.accounts.get(&pubkey).map(|acc| acc.executable()).unwrap_or(false)
    } else {
        false
    };
    let name_suffix = match name {
        Some(n) => format!(" {}", n.custom_color(DIM_GRAY)),
        None => String::new(),
    };
    let source_marker = render_account_marker(account.signer, account.writable, executable);
    let index_label = render_account_index_label(account.index, layout.index_width);
    let pubkey_display = format!("{:<width$}", account.pubkey, width = layout.pubkey_width);
    println!("{}{} {} {}{}", indent, index_label, pubkey_display, source_marker, name_suffix);
}

fn render_account_marker(signer: bool, writable: bool, executable: bool) -> String {
    let mode = account_mode_bits(signer, writable, executable);
    format!("[{mode}]").custom_color(DIM_GRAY).to_string()
}

fn account_mode_bits(signer: bool, writable: bool, executable: bool) -> String {
    let signer_bit = if signer { 's' } else { '-' };
    let access_bit = if writable { 'w' } else { 'r' };
    let exec_bit = if executable { 'x' } else { '-' };
    format!("{signer_bit}{access_bit}{exec_bit}")
}

/// Pre-computed column widths for aligned account display.
struct ColumnLayout {
    index_width: usize,
    pubkey_width: usize,
}

fn instruction_account_layout(transaction: &TransactionSection) -> ColumnLayout {
    let mut max_index = 0usize;
    let mut max_pubkey = 0usize;
    for ix in &transaction.instructions {
        for account in &ix.accounts {
            max_index = max_index.max(account.index);
            max_pubkey = max_pubkey.max(UnicodeWidthStr::width(account.pubkey.as_str()));
        }
        for inner_ix in &ix.inner_instructions {
            for account in &inner_ix.accounts {
                max_index = max_index.max(account.index);
                max_pubkey = max_pubkey.max(UnicodeWidthStr::width(account.pubkey.as_str()));
            }
        }
    }
    ColumnLayout { index_width: index_label_width(max_index), pubkey_width: max_pubkey }
}

fn account_list_layout(transaction: &TransactionSection) -> ColumnLayout {
    let total = transaction.static_accounts.len()
        + transaction.lookups.iter().map(|l| l.writable.len() + l.readonly.len()).sum::<usize>();
    let mut max_pubkey = 0usize;
    for account in &transaction.static_accounts {
        max_pubkey = max_pubkey.max(UnicodeWidthStr::width(account.pubkey.as_str()));
    }
    for lookup in &transaction.lookups {
        for entry in &lookup.writable {
            max_pubkey = max_pubkey.max(UnicodeWidthStr::width(entry.pubkey.as_str()));
        }
        for entry in &lookup.readonly {
            max_pubkey = max_pubkey.max(UnicodeWidthStr::width(entry.pubkey.as_str()));
        }
    }
    ColumnLayout {
        index_width: index_label_width(total.saturating_sub(1)),
        pubkey_width: max_pubkey,
    }
}

fn index_label_width(max_index: usize) -> usize {
    if max_index >= 100 { 3 } else { 2 }
}

fn render_account_index_label(index: usize, width: usize) -> String {
    format!("[{:>width$}]", index, width = width).custom_color(DIM_GRAY).to_string()
}

/// Render logs in a structured format, grouped by instruction with proper indentation.
fn render_logs_structured(logs: &[String]) {
    let instruction_logs = parse_logs_by_instruction(logs);

    for (idx, inst_logs) in instruction_logs.iter().enumerate() {
        if idx > 0 {
            println!();
        }
        // Print instruction header
        let program_name = get_program_display_name(&inst_logs.program);
        println!(
            "{} {} instruction",
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
                ">".custom_color(DIM_GRAY),
                format!("Program log: {}", message).custom_color(DIM_GRAY)
            );
        }
        LogEntry::Data { data } => {
            println!(
                "{}{} {}",
                indent,
                ">".custom_color(DIM_GRAY),
                format!("Program data: {}", truncate_display(data, 60)).custom_color(DIM_GRAY)
            );
        }
        LogEntry::Consumed { _program: _, used, total } => {
            println!(
                "{}{} {}",
                indent,
                ">".custom_color(DIM_GRAY),
                format!("Program consumed: {} of {} compute units", used, total)
                    .custom_color(DIM_GRAY)
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
                ">".custom_color(DIM_GRAY),
                format!("Program return: {}", truncate_display(data, 60)).custom_color(DIM_GRAY)
            );
        }
        LogEntry::Other(msg) => {
            if !msg.is_empty() {
                println!("{}{} {}", indent, ">".custom_color(DIM_GRAY), msg.custom_color(DIM_GRAY));
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

struct OrderedFields<'a>(&'a [ParsedField]);

impl Serialize for OrderedFields<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
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

fn render_instruction_data_text(indent: &str, data: &[u8]) {
    println!("{}0x{} {} bytes", indent, hex::encode(data), data.len());
}
