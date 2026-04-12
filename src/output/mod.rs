mod account_text;
mod fmt;
mod json;
pub(crate) mod report;
#[cfg(test)]
mod snapshot_tests;
mod table;
pub(crate) mod terminal;
mod text;
mod theme;

pub(crate) use account_text::render_account_text;
pub(crate) use json::print_json;

use anyhow::Result;

use solana_pubkey::Pubkey;

use crate::{core::transaction::ParsedTransaction, parsers::instruction::ParserRegistry};
use sonar_sim::internals::{
    AccountOverride, ExecutionResult, PreparedTokenFunding, ResolvedAccounts, SolFunding,
};

use report::{
    BundleReport, LookupResolver, Report, SolBalanceChangeSection, TokenBalanceChangeSection,
    TransactionSection,
};

/// Balance change display options.
#[derive(Debug, Clone, Copy, Default)]
pub struct BalanceChangeOptions {
    pub show_balance_change: bool,
}

/// Log display options.
#[derive(Debug, Clone, Copy, Default)]
pub struct LogDisplayOptions {
    /// If true, print raw logs; otherwise print structured execution trace.
    pub raw_log: bool,
}

/// Rendering configuration options.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderOptions {
    pub json: bool,
    pub show_ix_data: bool,
    pub show_ix_detail: bool,
    pub verify_signatures: bool,
    pub balance_opts: BalanceChangeOptions,
    pub log_opts: LogDisplayOptions,
}

/// Grouped simulation context arguments that are always passed together.
pub struct SimulationContext<'a> {
    pub account_closures: &'a [Pubkey],
    pub overrides: &'a [AccountOverride],
    pub fundings: &'a [SolFunding],
    pub token_fundings: &'a [PreparedTokenFunding],
}

pub fn render(
    parsed: &ParsedTransaction,
    resolved: &ResolvedAccounts,
    simulation: &ExecutionResult,
    ctx: &SimulationContext,
    parser_registry: &mut ParserRegistry,
    opts: &RenderOptions,
) -> Result<()> {
    let report = Report::from_sources(
        parsed,
        resolved,
        simulation,
        ctx,
        parser_registry,
        opts.verify_signatures,
        opts.balance_opts,
    );
    render_report(&report, resolved, parser_registry, opts)
}

/// Render a trace report with pre-computed balance changes from RPC metadata.
pub fn render_trace(
    parsed: &ParsedTransaction,
    resolved: &ResolvedAccounts,
    simulation: &ExecutionResult,
    parser_registry: &mut ParserRegistry,
    opts: &RenderOptions,
    sol_balance_changes: Vec<SolBalanceChangeSection>,
    token_balance_changes: Vec<TokenBalanceChangeSection>,
) -> Result<()> {
    let report = Report::from_trace(
        parsed,
        resolved,
        simulation,
        parser_registry,
        sol_balance_changes,
        token_balance_changes,
    );
    render_report(&report, resolved, parser_registry, opts)
}

fn render_report(
    report: &Report,
    resolved: &ResolvedAccounts,
    parser_registry: &mut ParserRegistry,
    opts: &RenderOptions,
) -> Result<()> {
    if opts.json {
        json::render_json(report)
    } else {
        let mut stdout = std::io::stdout().lock();
        text::render_text(
            report,
            resolved,
            parser_registry,
            opts.show_ix_data,
            opts.show_ix_detail,
            opts.log_opts,
            &mut stdout,
        )
    }
}

pub fn render_transaction_only(
    parsed: &ParsedTransaction,
    resolved: &ResolvedAccounts,
    parser_registry: &mut ParserRegistry,
    json: bool,
    show_ix_data: bool,
    bundle_info: Option<(usize, usize)>,
) -> Result<()> {
    let resolver = LookupResolver::new(resolved.lookup_details());
    let transaction =
        TransactionSection::from_sources(parsed, resolved, &resolver, parser_registry, false);
    if json {
        let json = serde_json::to_string_pretty(&transaction)?;
        println!("{json}");
        Ok(())
    } else {
        let mut stdout = std::io::stdout().lock();
        text::render_transaction_section_text(
            &transaction,
            resolved,
            parser_registry,
            show_ix_data,
            bundle_info,
            &mut stdout,
        )
    }
}

/// Render multiple decoded transactions as a single JSON array `[{...}, {...}]`.
/// Used for bundle decode with `--json`; output is a valid JSON document parseable by jq.
pub fn render_decode_bundle_json(
    parsed_txs: &[ParsedTransaction],
    resolved: &ResolvedAccounts,
    parser_registry: &mut ParserRegistry,
) -> Result<()> {
    let resolver = LookupResolver::new(resolved.lookup_details());
    let sections: Vec<TransactionSection> = parsed_txs
        .iter()
        .map(|parsed| {
            TransactionSection::from_sources(parsed, resolved, &resolver, parser_registry, false)
        })
        .collect();
    json::render_json_array(&sections)
}

/// Render multiple transaction simulation results (bundle simulation).
pub fn render_bundle(
    parsed_txs: &[ParsedTransaction],
    total_tx_count: usize,
    resolved: &ResolvedAccounts,
    simulations: &[ExecutionResult],
    ctx: &SimulationContext,
    parser_registry: &mut ParserRegistry,
    opts: &RenderOptions,
) -> Result<()> {
    let bundle_report = BundleReport::from_sources(
        parsed_txs,
        resolved,
        simulations,
        ctx,
        parser_registry,
        opts.verify_signatures,
        opts.balance_opts,
    );

    if opts.json {
        json::render_bundle_json(&bundle_report)
    } else {
        let mut stdout = std::io::stdout().lock();
        text::render_bundle_text(
            &bundle_report,
            total_tx_count,
            resolved,
            opts.show_ix_data,
            opts.show_ix_detail,
            opts.log_opts,
            &mut stdout,
        )
    }
}
