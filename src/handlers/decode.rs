use anyhow::Result;

use crate::cli::{self, DecodeArgs, TransactionInputArgs};
use crate::instruction_parsers::ParserRegistry;
use crate::{account_loader, output, transaction};

use super::collect_program_ids;

pub(crate) fn handle(args: DecodeArgs) -> Result<()> {
    let idl_dir = args.idl_dir.clone();
    let mut parser_registry = ParserRegistry::new(idl_dir);

    let DecodeArgs { transaction, rpc, ix_data, idl_dir: _ } = args;
    let rpc_url = rpc.rpc_url;
    let TransactionInputArgs { tx, output } = transaction;

    // Check if this is a bundle (multiple positional TX arguments)
    if tx.len() > 1 {
        return handle_bundle(tx, &rpc_url, ix_data, output, &mut parser_registry);
    }

    // Single tx: take the first positional arg, or fall back to stdin
    let tx_single = tx.into_iter().next();
    let raw_input = transaction::read_raw_transaction(tx_single)?;

    // Check if input looks like a transaction signature (works for all input methods)
    let raw_tx = if transaction::is_transaction_signature(&raw_input) {
        log::info!("Input appears to be a transaction signature, attempting to fetch from RPC...");
        transaction::fetch_transaction_from_rpc(&rpc_url, &raw_input)?
    } else {
        raw_input
    };

    let parsed_tx = transaction::parse_raw_transaction(&raw_tx)?;

    let account_loader = account_loader::AccountLoader::new(rpc_url, None, false)?;
    let resolved_accounts = account_loader.load_for_transaction(&parsed_tx.transaction, &[])?;

    let program_ids = collect_program_ids(&resolved_accounts);
    if program_ids.is_empty() {
        log::error!("No executable accounts found after RPC load; skipping IDL parsing");
    } else {
        match parser_registry.load_idl_parsers_for_programs(program_ids) {
            Ok(count) if count > 0 => log::info!("Lazy-loaded {} IDL parsers", count),
            Ok(_) => {}
            Err(err) => log::warn!("Failed to load IDL parsers: {:?}", err),
        }
    }

    output::render_transaction_only(
        &parsed_tx,
        &resolved_accounts,
        &mut parser_registry,
        output,
        ix_data,
        None,
    )?;

    Ok(())
}

/// Handle bundle decode (multiple transactions decoded without simulation).
fn handle_bundle(
    tx_inputs: Vec<String>,
    rpc_url: &str,
    ix_data: bool,
    output_format: cli::OutputFormat,
    parser_registry: &mut ParserRegistry,
) -> Result<()> {
    log::info!("Bundle decode mode: {} transactions", tx_inputs.len());

    let parsed_txs = transaction::parse_multi_raw_transactions(&tx_inputs, rpc_url)?;
    log::info!("Successfully parsed {} transactions", parsed_txs.len());

    let tx_refs: Vec<_> = parsed_txs.iter().map(|p| &p.transaction).collect();

    let account_loader = account_loader::AccountLoader::new(rpc_url.to_string(), None, false)?;
    let resolved_accounts = account_loader.load_for_transactions(&tx_refs, &[])?;

    let program_ids = collect_program_ids(&resolved_accounts);
    if !program_ids.is_empty() {
        match parser_registry.load_idl_parsers_for_programs(program_ids) {
            Ok(count) if count > 0 => log::info!("Lazy-loaded {} IDL parsers", count),
            Ok(_) => {}
            Err(err) => log::warn!("Failed to load IDL parsers: {:?}", err),
        }
    }

    let total = parsed_txs.len();
    for (i, parsed_tx) in parsed_txs.iter().enumerate() {
        output::render_transaction_only(
            parsed_tx,
            &resolved_accounts,
            parser_registry,
            output_format,
            ix_data,
            Some((i + 1, total)),
        )?;
    }

    Ok(())
}
