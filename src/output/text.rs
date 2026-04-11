use std::io::Write;
use std::str::FromStr;

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;
use serde::ser::SerializeMap;
use serde_json::Value;
use solana_account::ReadableAccount;
use solana_pubkey::Pubkey;
use unicode_width::UnicodeWidthStr;

use crate::parsers::{
    instruction::{ParsedField, ParsedFieldValue, ParsedInstructionFields, ParserRegistry},
    log_parser::{LogEntry, LogEntryWithDepth, parse_logs_by_instruction},
};
use sonar_sim::internals::ResolvedAccounts;

use super::fmt::{format_with_commas, truncate_display, truncate_sig};
use super::LogDisplayOptions;
use super::report::{
    BundleReport, InstructionAccountEntry, Report, SimulationSection,
    SimulationStatusReport, SolBalanceChangeSection, TokenBalanceChangeSection, TransactionSection,
};
use super::terminal::{terminal_width, write_section_title};
use super::theme::{COLOR_BLUE, COLOR_GOLD, COLOR_GREEN, COLOR_RED, DIM_GRAY, RAW_HEX_AMBER};

/// Single indentation unit (2 spaces).
const INDENT: &str = "  ";
/// Indentation for outer items (level 1 = 2 spaces).
const INDENT_L1: &str = INDENT;
/// Indentation for inner items (level 2 = 4 spaces).
const INDENT_L2: &str = "    ";

pub(super) fn render_text(
    report: &Report,
    resolved: &ResolvedAccounts,
    _parser_registry: &mut ParserRegistry,
    show_ix_data: bool,
    show_ix_detail: bool,
    log_opts: LogDisplayOptions,
    w: &mut impl Write,
) -> Result<()> {
    render_summary_header(&report.simulation, &report.transaction, w);

    if !report.simulation.logs.is_empty()
        || matches!(&report.simulation.status, SimulationStatusReport::Failed { .. })
    {
        write_section_title(w, "Execution Trace");
        render_execution_trace_section(&report.simulation, log_opts, w);
    }

    if show_ix_detail {
        write_section_title(w, "Instruction Details");
        render_instruction_details_text(&report.transaction, resolved, show_ix_data, w);
    }

    if !report.sol_balance_changes.is_empty() {
        write_section_title(w, "SOL Balance Changes");
        render_sol_balance_changes(&report.sol_balance_changes, "", w);
    }

    if !report.token_balance_changes.is_empty() {
        write_section_title(w, "Token Balance Changes");
        render_token_balance_changes(&report.token_balance_changes, "", w);
    }

    let _ = writeln!(w);
    w.flush()?;
    Ok(())
}

pub(super) fn render_bundle_text(
    bundle: &BundleReport,
    total_count: usize,
    resolved: &ResolvedAccounts,
    show_ix_data: bool,
    show_ix_detail: bool,
    log_opts: LogDisplayOptions,
    w: &mut impl Write,
) -> Result<()> {
    render_bundle_summary_header(bundle, total_count, w);

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
            write_section_title(w, &format!("Execution Trace: TX {}/{}", i + 1, total_count));
            render_execution_trace_section(&tx_report.simulation, log_opts, w);
        }
    }

    if show_ix_detail {
        for (i, tx_report) in bundle.transactions.iter().enumerate() {
            write_section_title(w, &format!("Instruction Details: TX {}/{}", i + 1, total_count));
            render_instruction_details_text(&tx_report.transaction, resolved, show_ix_data, w);
        }
    }

    if !bundle.sol_balance_changes.is_empty() {
        write_section_title(w, "SOL Balance Changes");
        render_sol_balance_changes(&bundle.sol_balance_changes, INDENT_L1, w);
    }

    if !bundle.token_balance_changes.is_empty() {
        write_section_title(w, "Token Balance Changes");
        render_token_balance_changes(&bundle.token_balance_changes, INDENT_L1, w);
    }

    let _ = writeln!(w);
    w.flush()?;
    Ok(())
}

pub(super) fn render_transaction_section_text(
    transaction: &TransactionSection,
    resolved: &ResolvedAccounts,
    _parser_registry: &mut ParserRegistry,
    show_ix_data: bool,
    bundle_info: Option<(usize, usize)>,
    w: &mut impl Write,
) -> Result<()> {
    let tx_suffix = match bundle_info {
        Some((index, total)) => format!(": TX {}/{}", index, total),
        None => String::new(),
    };

    write_section_title(w, &format!("Decoded Instructions{}", tx_suffix));
    render_instruction_details_text(transaction, resolved, show_ix_data, w);

    if !transaction.lookups.is_empty() {
        write_section_title(w, &format!("Address Lookup Tables{}", tx_suffix));
        render_lookup_tables_text(transaction, w);
    }

    write_section_title(w, &format!("Account List{}", tx_suffix));
    render_account_list_text(transaction, resolved, w);

    let _ = writeln!(w);
    w.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Bundle summary
// ---------------------------------------------------------------------------

fn render_bundle_summary_header(
    bundle: &BundleReport,
    total_count: usize,
    w: &mut impl Write,
) {
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

    let total_cu: u64 =
        bundle.transactions.iter().map(|t| t.simulation.compute_units_consumed).sum();

    let summary_text = format!(
        "Bundle: {} | TX: {}/{} | CU: {}",
        status_str,
        bundle.transactions.len(),
        total_count,
        format_with_commas(total_cu)
    );
    write_section_title(w, &summary_text);

    let tx_col_width = total_count.to_string().len().max(2);
    const CU_COL_WIDTH: usize = 12;

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

        let _ = writeln!(
            w,
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

    for i in bundle.transactions.len()..total_count {
        let _ = writeln!(
            w,
            "{}TX {:>tx_w$}/{:<tx_w$}  »  SKIPPED",
            INDENT_L1,
            i + 1,
            total_count,
            tx_w = tx_col_width
        );
    }
}

// ---------------------------------------------------------------------------
// Single-TX summary
// ---------------------------------------------------------------------------

fn render_summary_header(
    simulation: &SimulationSection,
    transaction: &TransactionSection,
    w: &mut impl Write,
) {
    let status_str = match &simulation.status {
        SimulationStatusReport::Succeeded => "✓ SUCCESS".to_string(),
        SimulationStatusReport::Failed { .. } => "✗ FAILED".to_string(),
    };

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
    write_section_title(w, &result_text);
}

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
                                if let Value::Number(num) = json {
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

// ---------------------------------------------------------------------------
// Execution trace / logs
// ---------------------------------------------------------------------------

fn render_execution_trace_section(
    simulation: &SimulationSection,
    log_opts: LogDisplayOptions,
    w: &mut impl Write,
) {
    if simulation.logs.is_empty() {
        if let SimulationStatusReport::Failed { error } = &simulation.status {
            let _ = writeln!(w, "{}↳ Failed before program invocation: {}", INDENT_L1, error);
        }
        return;
    }

    if log_opts.raw_log {
        for line in &simulation.logs {
            let _ = writeln!(w, "{}", line);
        }
    } else {
        render_logs_structured(&simulation.logs, w);
    }
}

fn render_logs_structured(logs: &[String], w: &mut impl Write) {
    let instruction_logs = parse_logs_by_instruction(logs);

    for (idx, inst_logs) in instruction_logs.iter().enumerate() {
        if idx > 0 {
            let _ = writeln!(w);
        }
        let program_name = get_program_display_name(&inst_logs.program);
        let _ = writeln!(
            w,
            "{} {} instruction",
            format!("#{}", inst_logs.instruction_index + 1).bold(),
            program_name.bold()
        );

        for entry_with_depth in &inst_logs.entries {
            render_log_entry(entry_with_depth, w);
        }
    }
}

fn get_program_display_name(pubkey: &str) -> &str {
    pubkey
}

fn render_log_entry(entry_with_depth: &LogEntryWithDepth, w: &mut impl Write) {
    let depth = entry_with_depth.depth as usize;
    let indent = INDENT.repeat(depth);

    match &entry_with_depth.entry {
        LogEntry::Invoke { program, depth: invoke_depth } => {
            if *invoke_depth > 1 {
                let program_name = get_program_display_name(program);
                let _ = writeln!(w, "{}{} {}", indent, "> Invoking".cyan(), program_name.cyan());
            }
        }
        LogEntry::Log { message } => {
            let _ = writeln!(
                w,
                "{}{} {}",
                indent,
                ">".custom_color(DIM_GRAY),
                format!("Program log: {}", message).custom_color(DIM_GRAY)
            );
        }
        LogEntry::Data { data } => {
            let _ = writeln!(
                w,
                "{}{} {}",
                indent,
                ">".custom_color(DIM_GRAY),
                format!("Program data: {}", truncate_display(data, 60)).custom_color(DIM_GRAY)
            );
        }
        LogEntry::Consumed { _program: _, used, total } => {
            let _ = writeln!(
                w,
                "{}{} {}",
                indent,
                ">".custom_color(DIM_GRAY),
                format!("Program consumed: {} of {} compute units", used, total)
                    .custom_color(DIM_GRAY)
            );
        }
        LogEntry::Success { _program: _ } => {
            let _ = writeln!(w, "{}{} {}", indent, ">".green(), "Program returned success".green());
        }
        LogEntry::Failed { _program: _, error } => {
            let _ = writeln!(
                w,
                "{}{} {}",
                indent,
                ">".red(),
                format!("Program failed: {}", error).red()
            );
        }
        LogEntry::Return { _program: _, data } => {
            let _ = writeln!(
                w,
                "{}{} {}",
                indent,
                ">".custom_color(DIM_GRAY),
                format!("Program return: {}", truncate_display(data, 60)).custom_color(DIM_GRAY)
            );
        }
        LogEntry::Other(msg) => {
            if !msg.is_empty() {
                let _ =
                    writeln!(w, "{}{} {}", indent, ">".custom_color(DIM_GRAY), msg.custom_color(DIM_GRAY));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Balance change tables
// ---------------------------------------------------------------------------

fn render_sol_balance_changes(
    sol_changes: &[SolBalanceChangeSection],
    indent: &str,
    w: &mut impl Write,
) {
    use super::table::{Align, Cell, TableWriter};

    let prefix = format!("{}{}", indent, INDENT_L1);
    let mut table =
        TableWriter::new(&prefix).column(Align::Left).column(Align::Left).column(Align::Left);
    if let Some(w) = terminal_width() {
        table = table.max_width(w);
    }

    for c in sol_changes {
        let sol_before = c.before as f64 / 1_000_000_000.0;
        let sol_after = c.after as f64 / 1_000_000_000.0;
        let sign = if c.change >= 0 { "+" } else { "" };
        let color = if c.change >= 0 { COLOR_GREEN } else { COLOR_RED };

        table.row(vec![
            Cell::plain(&c.account),
            Cell::colored(&format!("{}{:.9}", sign, c.change_sol), color),
            Cell::colored(&format!("{:.9} → {:.9}", sol_before, sol_after), COLOR_BLUE),
        ]);
    }

    table.print(w);
    let _ = writeln!(
        w,
        "\n{}",
        "  account  ±Δ(SOL)  before → after | (+) increase  (-) decrease".custom_color(DIM_GRAY)
    );
}

fn render_token_balance_changes(
    token_changes: &[TokenBalanceChangeSection],
    indent: &str,
    w: &mut impl Write,
) {
    use super::table::{Align, Cell, TableWriter};

    let prefix = format!("{}{}", indent, INDENT_L1);
    let mut table = TableWriter::new(&prefix)
        .column(Align::Left)
        .column(Align::Left)
        .column(Align::Left)
        .column(Align::Left)
        .column(Align::Left);
    if let Some(w) = terminal_width() {
        table = table.max_width(w);
    }

    for c in token_changes {
        let divisor = 10f64.powi(c.decimals as i32);
        let ui_before = c.before as f64 / divisor;
        let ui_after = c.after as f64 / divisor;
        let prec = c.decimals as usize;
        let sign = if c.change >= 0 { "+" } else { "" };
        let color = if c.change >= 0 { COLOR_GREEN } else { COLOR_RED };

        table.row(vec![
            Cell::plain(&c.token_account),
            Cell::plain(&c.owner),
            Cell::colored(&format!("{}{:.prec$}", sign, c.ui_change, prec = prec), color),
            Cell::colored(
                &format!("{:.prec$} → {:.prec$}", ui_before, ui_after, prec = prec),
                COLOR_BLUE,
            ),
            Cell::plain(&c.mint),
        ]);
    }

    table.print(w);
    let _ = writeln!(
        w,
        "\n{}",
        "  token-account  owner  ±Δ(amount)  before → after  mint | (+) increase  (-) decrease"
            .custom_color(DIM_GRAY)
    );
}

// ---------------------------------------------------------------------------
// Instruction details
// ---------------------------------------------------------------------------

fn render_instruction_details_text(
    transaction: &TransactionSection,
    resolved: &ResolvedAccounts,
    show_ix_data: bool,
    w: &mut impl Write,
) {
    let layout = instruction_account_layout(transaction);

    let data_indent = layout.data_indent(INDENT_L1);
    let inner_data_indent = layout.data_indent(INDENT_L2);

    for (ix_pos, ix) in transaction.instructions.iter().enumerate() {
        if ix_pos > 0 {
            let _ = writeln!(w);
        }
        let program_pubkey = ix.program.pubkey.as_str();
        let outer_number = ix.index + 1;

        // When an instruction has no accounts, indent data relative to "#N "
        // instead of the account column to keep hex flush under the program name.
        let current_data_indent = if ix.accounts.is_empty() {
            " ".repeat(1 + outer_number.to_string().len() + 1)
        } else {
            data_indent.clone()
        };

        if let Some(parsed) = &ix.parsed {
            let _ = writeln!(
                w,
                "#{} {} {}",
                outer_number.to_string().custom_color(COLOR_GOLD),
                program_pubkey.cyan(),
                format!("[{}]", parsed.name).custom_color(COLOR_GREEN)
            );

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
                    w,
                );
            }

            render_instruction_data_and_fields(
                &parsed.fields,
                &current_data_indent,
                &ix.data,
                show_ix_data,
                w,
            );
        } else {
            let _ = writeln!(
                w,
                "#{} {}",
                outer_number.to_string().custom_color(COLOR_GOLD),
                program_pubkey.cyan()
            );

            for account in &ix.accounts {
                render_instruction_account_text(account, resolved, None, INDENT_L1, &layout, w);
            }
            render_instruction_data_text(&current_data_indent, &ix.data, w);
        }

        if !ix.inner_instructions.is_empty() {
            for inner_ix in &ix.inner_instructions {
                let current_inner_data_indent = if inner_ix.accounts.is_empty() {
                    " ".repeat(INDENT_L1.len() + 1 + inner_ix.label.len() + 1)
                } else {
                    inner_data_indent.clone()
                };

                if let Some(parsed_inner) = &inner_ix.parsed {
                    let _ = writeln!(
                        w,
                        "{}{} {} {}",
                        INDENT_L1,
                        format!("#{}", inner_ix.label).custom_color(COLOR_GOLD),
                        inner_ix.program.pubkey.as_str().cyan(),
                        format!("[{}]", parsed_inner.name).custom_color(COLOR_GREEN)
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
                            w,
                        );
                    }

                    render_instruction_data_and_fields(
                        &parsed_inner.fields,
                        &current_inner_data_indent,
                        &inner_ix.data,
                        show_ix_data,
                        w,
                    );
                } else {
                    let _ = writeln!(
                        w,
                        "{}{} {}",
                        INDENT_L1,
                        format!("#{}", inner_ix.label).custom_color(COLOR_GOLD),
                        inner_ix.program.pubkey.as_str().cyan()
                    );

                    for account in &inner_ix.accounts {
                        render_instruction_account_text(
                            account, resolved, None, INDENT_L2, &layout, w,
                        );
                    }
                    render_instruction_data_text(&current_inner_data_indent, &inner_ix.data, w);
                }
            }
        }
    }

    let legend = render_account_legend(transaction);
    let _ = writeln!(w, "\n{}", legend.custom_color(DIM_GRAY));
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

// ---------------------------------------------------------------------------
// Account rendering helpers
// ---------------------------------------------------------------------------

fn render_lookup_tables_text(transaction: &TransactionSection, w: &mut impl Write) {
    if transaction.lookups.is_empty() {
        return;
    }
    let index_width = index_label_width(transaction.lookups.len().saturating_sub(1));

    for (idx, lookup) in transaction.lookups.iter().enumerate() {
        let _ = writeln!(
            w,
            "{}{} {}",
            INDENT_L1,
            render_account_index_label(idx, index_width),
            lookup.account_key
        );
    }
}

fn render_account_list_text(
    transaction: &TransactionSection,
    resolved: &ResolvedAccounts,
    w: &mut impl Write,
) {
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
            w,
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
                w,
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
                w,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_account_entry_text(
    index: usize,
    pubkey_str: &str,
    signer: bool,
    writable: bool,
    layout: &ColumnLayout,
    resolved: &ResolvedAccounts,
    lookup_info: Option<(usize, u8)>,
    w: &mut impl Write,
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
    let _ = writeln!(w, "{}{} {} {}{}", INDENT_L1, index_label, pubkey_display, marker, lookup_suffix);
    index + 1
}

fn render_instruction_account_text(
    account: &InstructionAccountEntry,
    resolved: &ResolvedAccounts,
    name: Option<&str>,
    indent: &str,
    layout: &ColumnLayout,
    w: &mut impl Write,
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
    let _ = writeln!(w, "{}{} {} {}{}", indent, index_label, pubkey_display, source_marker, name_suffix);
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

// ---------------------------------------------------------------------------
// Column layout
// ---------------------------------------------------------------------------

/// Pre-computed column widths for aligned account display.
struct ColumnLayout {
    index_width: usize,
    pubkey_width: usize,
}

impl ColumnLayout {
    /// Indent that aligns with the first character after `[idx] pubkey `.
    /// The index label occupies `index_width + 2` chars (brackets) plus a trailing space.
    fn data_indent(&self, base_indent: &str) -> String {
        format!("{}{}", base_indent, " ".repeat(self.index_width + 3))
    }
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

// ---------------------------------------------------------------------------
// Instruction data / parsed fields
// ---------------------------------------------------------------------------

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

fn render_parsed_fields(fields: &ParsedInstructionFields, indent: &str, w: &mut impl Write) {
    let ParsedInstructionFields::Parsed(fields) = fields else {
        return;
    };

    let ordered = OrderedFields(fields);
    let pretty = serde_json::to_string_pretty(&ordered).unwrap_or_else(|_| "{}".to_string());

    for line in pretty.lines() {
        let _ = writeln!(w, "{}", format!("{}{}", indent, line).custom_color(COLOR_BLUE));
    }
}

fn render_instruction_data_and_fields(
    fields: &ParsedInstructionFields,
    indent: &str,
    data: &[u8],
    show_ix_data: bool,
    w: &mut impl Write,
) {
    if let ParsedInstructionFields::RawHex(raw_hex) = fields {
        if !raw_hex.is_empty() {
            let line = format!("{}0x{} {} bytes", indent, hex::encode(data), data.len());
            let _ = writeln!(w, "{}", line.custom_color(RAW_HEX_AMBER));
            return;
        }
    }

    if show_ix_data {
        render_instruction_data_text(indent, data, w);
    }
    render_parsed_fields(fields, indent, w);
}

fn render_instruction_data_text(indent: &str, data: &[u8], w: &mut impl Write) {
    let _ = writeln!(w, "{}0x{} {} bytes", indent, hex::encode(data), data.len());
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_instruction_data_and_fields_uses_raw_hex_for_unparsed() {
        let fields = ParsedInstructionFields::RawHex("0025a16608ca3c00".to_string());
        let mut buf = Vec::new();
        render_instruction_data_and_fields(&fields, "  ", &[0, 37, 161, 102], false, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("0x"));
    }
}
