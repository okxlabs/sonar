mod json;
mod report;
mod text;

use anyhow::Result;

use crate::{
    account_loader::ResolvedAccounts,
    cli::{Funding, OutputFormat, Replacement},
    executor::SimulationResult,
    funding::PreparedTokenFunding,
    instruction_parsers::ParserRegistry,
    transaction::ParsedTransaction,
};

use report::{BundleReport, LookupResolver, Report, TransactionSection};

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
    pub format: OutputFormat,
    pub show_ix_data: bool,
    pub show_ix_detail: bool,
    pub verify_signatures: bool,
    pub balance_opts: BalanceChangeOptions,
    pub log_opts: LogDisplayOptions,
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    parsed: &ParsedTransaction,
    resolved: &ResolvedAccounts,
    simulation: &SimulationResult,
    replacements: &[Replacement],
    fundings: &[Funding],
    token_fundings: &[PreparedTokenFunding],
    parser_registry: &mut ParserRegistry,
    opts: &RenderOptions,
) -> Result<()> {
    let report = Report::from_sources(
        parsed,
        resolved,
        simulation,
        replacements,
        fundings,
        token_fundings,
        parser_registry,
        opts.verify_signatures,
        opts.balance_opts,
    );
    match opts.format {
        OutputFormat::Text => text::render_text(
            &report,
            resolved,
            parser_registry,
            opts.show_ix_data,
            opts.show_ix_detail,
            opts.log_opts,
        ),
        OutputFormat::Json => json::render_json(&report),
    }
}

pub fn render_transaction_only(
    parsed: &ParsedTransaction,
    resolved: &ResolvedAccounts,
    parser_registry: &mut ParserRegistry,
    format: OutputFormat,
    show_ix_data: bool,
    bundle_info: Option<(usize, usize)>,
) -> Result<()> {
    let resolver = LookupResolver::new(resolved.lookup_details());
    let transaction =
        TransactionSection::from_sources(parsed, resolved, &resolver, parser_registry, false);
    match format {
        OutputFormat::Text => {
            text::render_transaction_section_text(
                &transaction,
                resolved,
                parser_registry,
                show_ix_data,
                bundle_info,
            );
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
#[allow(clippy::too_many_arguments)]
pub fn render_bundle(
    parsed_txs: &[ParsedTransaction],
    total_tx_count: usize,
    resolved: &ResolvedAccounts,
    simulations: &[SimulationResult],
    replacements: &[Replacement],
    fundings: &[Funding],
    token_fundings: &[PreparedTokenFunding],
    parser_registry: &mut ParserRegistry,
    opts: &RenderOptions,
) -> Result<()> {
    let bundle_report = BundleReport::from_sources(
        parsed_txs,
        resolved,
        simulations,
        replacements,
        fundings,
        token_fundings,
        parser_registry,
        opts.verify_signatures,
        opts.balance_opts,
    );

    match opts.format {
        OutputFormat::Text => text::render_bundle_text(
            &bundle_report,
            total_tx_count,
            resolved,
            opts.show_ix_data,
            opts.show_ix_detail,
            opts.log_opts,
        ),
        OutputFormat::Json => json::render_bundle_json(&bundle_report),
    }
}
