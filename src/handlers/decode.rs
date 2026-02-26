use anyhow::Result;

use crate::cli::{DecodeArgs, TransactionInputArgs};
use crate::output;
use crate::parsers::instruction::ParserRegistry;
use crate::utils::progress::Progress;

use super::{parse_inputs_to_txs, prepare_accounts_and_idls};

pub(crate) fn handle(args: DecodeArgs) -> Result<()> {
    let idl_dir = args.idl_dir.clone();
    let mut parser_registry = ParserRegistry::new(idl_dir);
    let progress = Progress::new();

    let DecodeArgs { transaction, rpc, ix_data, idl_dir: _, no_idl_fetch } = args;
    let rpc_url = rpc.rpc_url;
    let TransactionInputArgs { tx, json } = transaction;

    // Check if this is a bundle (multiple positional TX arguments)
    if tx.len() > 1 {
        return handle_bundle(
            tx,
            &rpc_url,
            ix_data,
            json,
            no_idl_fetch,
            &mut parser_registry,
            &progress,
        );
    }

    let parsed_inputs = parse_inputs_to_txs(tx, &rpc_url, &progress, false)?;
    let mut parsed_txs = parsed_inputs.parsed_txs;
    let parsed_tx =
        parsed_txs.pop().expect("single input parse should produce one parsed transaction");

    let prepared = prepare_accounts_and_idls(
        &rpc_url,
        None,
        false,
        std::slice::from_ref(&parsed_tx),
        &mut parser_registry,
        no_idl_fetch,
        &progress,
    )?;

    progress.finish();
    output::render_transaction_only(
        &parsed_tx,
        &prepared.resolved_accounts,
        &mut parser_registry,
        json,
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
    json: bool,
    no_idl_fetch: bool,
    parser_registry: &mut ParserRegistry,
    progress: &Progress,
) -> Result<()> {
    log::info!("Bundle decode mode: {} transactions", tx_inputs.len());

    let parsed_inputs = parse_inputs_to_txs(tx_inputs, rpc_url, progress, true)?;
    let parsed_txs = parsed_inputs.parsed_txs;
    log::info!("Successfully parsed {} transactions", parsed_txs.len());

    let prepared = prepare_accounts_and_idls(
        rpc_url,
        None,
        false,
        &parsed_txs,
        parser_registry,
        no_idl_fetch,
        progress,
    )?;

    progress.finish();

    if json {
        output::render_decode_bundle_json(
            &parsed_txs,
            &prepared.resolved_accounts,
            parser_registry,
        )?;
    } else {
        for (i, parsed_tx) in parsed_txs.iter().enumerate() {
            output::render_transaction_only(
                parsed_tx,
                &prepared.resolved_accounts,
                parser_registry,
                false,
                ix_data,
                Some((i + 1, parsed_txs.len())),
            )?;
        }
    }

    Ok(())
}
