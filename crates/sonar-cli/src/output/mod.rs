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
use sonar_sim::{
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
    pub account_overrides: &'a [AccountOverride],
    pub fundings: &'a [SolFunding],
    pub token_fundings: &'a [PreparedTokenFunding],
}

// ── Simulation rendering ──

/// Which simulation result to render. Each variant carries exactly the data
/// its shape needs; [`render_simulation`] owns report construction and the
/// JSON/text fork for all of them.
pub enum SimulationKind<'a> {
    /// A single simulated transaction.
    Single {
        parsed: &'a ParsedTransaction,
        simulation: &'a ExecutionResult,
        ctx: SimulationContext<'a>,
    },
    /// A replayed transaction whose balance changes were computed from RPC
    /// metadata rather than from simulation.
    Replay {
        parsed: &'a ParsedTransaction,
        simulation: &'a ExecutionResult,
        sol_balance_changes: Vec<SolBalanceChangeSection>,
        token_balance_changes: Vec<TokenBalanceChangeSection>,
    },
    /// A bundle of transactions executed sequentially.
    Bundle {
        parsed_txs: &'a [ParsedTransaction],
        /// Total transactions in the bundle, including any skipped after a
        /// prior failure (may exceed `simulations.len()`).
        total_tx_count: usize,
        simulations: &'a [ExecutionResult],
        ctx: SimulationContext<'a>,
    },
}

/// Grouped inputs for [`render_simulation`].
pub struct SimulationRender<'a> {
    pub resolved: &'a ResolvedAccounts,
    pub registry: &'a mut ParserRegistry,
    pub opts: &'a RenderOptions,
    pub kind: SimulationKind<'a>,
}

/// Render a simulation result — single, replay, or bundle — to stdout. Builds
/// the intermediate report and dispatches to the JSON or text writer based on
/// [`RenderOptions::json`].
pub fn render_simulation(req: SimulationRender) -> Result<()> {
    let SimulationRender { resolved, registry, opts, kind } = req;
    match kind {
        SimulationKind::Single { parsed, simulation, ctx } => {
            let report = Report::from_sources(
                parsed,
                resolved,
                simulation,
                &ctx,
                registry,
                opts.verify_signatures,
                opts.balance_opts,
            );
            emit_report(&report, resolved, registry, opts)
        }
        SimulationKind::Replay {
            parsed,
            simulation,
            sol_balance_changes,
            token_balance_changes,
        } => {
            let report = Report::from_replay(
                parsed,
                resolved,
                simulation,
                registry,
                sol_balance_changes,
                token_balance_changes,
            );
            emit_report(&report, resolved, registry, opts)
        }
        SimulationKind::Bundle { parsed_txs, total_tx_count, simulations, ctx } => {
            let report = BundleReport::from_sources(
                parsed_txs,
                resolved,
                simulations,
                &ctx,
                registry,
                opts.verify_signatures,
                opts.balance_opts,
            );
            emit_bundle(&report, total_tx_count, resolved, opts)
        }
    }
}

/// JSON/text fork for a single-transaction report (simulate and replay).
fn emit_report(
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

/// JSON/text fork for a bundle report.
fn emit_bundle(
    report: &BundleReport,
    total_tx_count: usize,
    resolved: &ResolvedAccounts,
    opts: &RenderOptions,
) -> Result<()> {
    if opts.json {
        json::render_bundle_json(report)
    } else {
        let mut stdout = std::io::stdout().lock();
        text::render_bundle_text(
            report,
            total_tx_count,
            resolved,
            opts.show_ix_data,
            opts.show_ix_detail,
            opts.log_opts,
            &mut stdout,
        )
    }
}

// ── Decode rendering ──

/// Grouped inputs for [`render_decode`].
pub struct DecodeRender<'a> {
    pub parsed_txs: &'a [ParsedTransaction],
    pub resolved: &'a ResolvedAccounts,
    pub registry: &'a mut ParserRegistry,
    pub show_ix_data: bool,
    pub json: bool,
}

/// Render decoded transactions (no execution) to stdout. Handles a single
/// transaction or a bundle, JSON or text, behind one entry point: JSON renders
/// a lone transaction as an object and a bundle as an array (`[{...}, {...}]`,
/// parseable by jq); text renders each transaction in turn, tagging bundle
/// members with their position.
pub fn render_decode(req: DecodeRender) -> Result<()> {
    let DecodeRender { parsed_txs, resolved, registry, show_ix_data, json } = req;
    let resolver = LookupResolver::new(resolved.lookup_details());
    let sections: Vec<TransactionSection> = parsed_txs
        .iter()
        .map(|parsed| {
            TransactionSection::from_sources(parsed, resolved, &resolver, registry, false)
        })
        .collect();

    if json {
        match sections.as_slice() {
            [single] => {
                let json = serde_json::to_string_pretty(single)?;
                println!("{json}");
                Ok(())
            }
            _ => json::render_json_array(&sections),
        }
    } else {
        let total = sections.len();
        let is_bundle = total > 1;
        let mut stdout = std::io::stdout().lock();
        for (i, section) in sections.iter().enumerate() {
            let bundle_info = is_bundle.then_some((i + 1, total));
            text::render_transaction_section_text(
                section,
                resolved,
                registry,
                show_ix_data,
                bundle_info,
                &mut stdout,
            )?;
        }
        Ok(())
    }
}
