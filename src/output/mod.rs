mod account_text;
mod json;
mod report;
pub(crate) mod terminal;
mod text;

pub(crate) use account_text::render_account_text;

use anyhow::Result;

use crate::{
    core::{
        account_loader::ResolvedAccounts,
        executor::SimulationResult,
        funding::PreparedTokenFunding,
        transaction::ParsedTransaction,
        types::{AccountReplacement, SolFunding},
    },
    parsers::instruction::ParserRegistry,
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
    pub json: bool,
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
    replacements: &[AccountReplacement],
    fundings: &[SolFunding],
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
    if opts.json {
        json::render_json(&report)
    } else {
        text::render_text(
            &report,
            resolved,
            parser_registry,
            opts.show_ix_data,
            opts.show_ix_detail,
            opts.log_opts,
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
        text::render_transaction_section_text(
            &transaction,
            resolved,
            parser_registry,
            show_ix_data,
            bundle_info,
        );
        Ok(())
    }
}

/// Render multiple decoded transactions as a single JSON array `[{...}, {...}]`.
/// Used for bundle decode with `--json`; output is a valid JSON document parseable by jq.
#[allow(clippy::too_many_arguments)]
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
#[allow(clippy::too_many_arguments)]
pub fn render_bundle(
    parsed_txs: &[ParsedTransaction],
    total_tx_count: usize,
    resolved: &ResolvedAccounts,
    simulations: &[SimulationResult],
    replacements: &[AccountReplacement],
    fundings: &[SolFunding],
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

    if opts.json {
        json::render_bundle_json(&bundle_report)
    } else {
        text::render_bundle_text(
            &bundle_report,
            total_tx_count,
            resolved,
            opts.show_ix_data,
            opts.show_ix_detail,
            opts.log_opts,
        )
    }
}
