use std::path::PathBuf;

use anyhow::Result;

use crate::cli::{DecodeArgs, TransactionInputArgs};
use crate::output;
use crate::parsers::instruction::ParserRegistry;
use crate::utils::progress::Progress;

use super::common::{cache_read_dir, prepare_accounts_and_idls, resolve_inputs_to_txs};

pub(crate) fn handle(args: DecodeArgs, json: bool) -> Result<()> {
    let idl_dir = args.idl_dir.clone();
    let mut parser_registry = ParserRegistry::new(idl_dir);
    let progress = Progress::new();

    let DecodeArgs {
        transaction,
        rpc,
        ix_data,
        idl_dir: _,
        no_idl_fetch,
        no_cache,
        cache_dir,
        refresh_cache,
        history_slot,
    } = args;
    let rpc_url = rpc.rpc_url;
    let resolver_cache_location =
        if refresh_cache { None } else { Some(super::common::build_cache_location(&cache_dir)) };
    let TransactionInputArgs { tx } = transaction;

    // Check if this is a bundle (multiple positional TX arguments)
    if tx.len() > 1 {
        return handle_bundle(
            tx,
            &rpc_url,
            resolver_cache_location,
            !no_cache,
            cache_dir,
            refresh_cache,
            ix_data,
            json,
            no_idl_fetch,
            history_slot,
            &mut parser_registry,
            &progress,
        );
    }

    let parsed_inputs =
        resolve_inputs_to_txs(tx, &rpc_url, resolver_cache_location, &progress, false)?;
    let resolved_tx = parsed_inputs
        .resolved_txs
        .into_iter()
        .next()
        .expect("single input resolve should produce one transaction");
    let original_input = resolved_tx.original_input;
    let parsed_tx = resolved_tx.parsed_tx;
    let cache_key = crate::core::cache::derive_cache_key_single(
        &original_input,
        &parsed_tx.transaction,
        history_slot,
    );
    let (decode_cache_dir, offline) =
        crate::core::cache::resolve_cache_state(!no_cache, &cache_dir, refresh_cache, &cache_key);
    let cache_read_dir_for_load = cache_read_dir(decode_cache_dir, refresh_cache);

    let prepared = prepare_accounts_and_idls(
        &rpc_url,
        cache_read_dir_for_load,
        offline,
        std::slice::from_ref(&parsed_tx),
        &mut parser_registry,
        no_idl_fetch,
        &progress,
        history_slot,
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
#[allow(clippy::too_many_arguments)]
fn handle_bundle(
    tx_inputs: Vec<String>,
    rpc_url: &str,
    resolver_cache_location: Option<crate::core::cache::CacheLocation>,
    cache: bool,
    cache_dir: Option<PathBuf>,
    refresh_cache: bool,
    ix_data: bool,
    json: bool,
    no_idl_fetch: bool,
    history_slot: Option<u64>,
    parser_registry: &mut ParserRegistry,
    progress: &Progress,
) -> Result<()> {
    log::info!("Bundle decode mode: {} transactions", tx_inputs.len());

    let parsed_inputs =
        resolve_inputs_to_txs(tx_inputs, rpc_url, resolver_cache_location, progress, true)?;
    let resolved_txs = parsed_inputs.resolved_txs;
    let raw_inputs: Vec<_> =
        resolved_txs.iter().map(|entry| entry.original_input.clone()).collect();
    let parsed_txs: Vec<_> = resolved_txs.into_iter().map(|entry| entry.parsed_tx).collect();
    let cache_key =
        crate::core::cache::derive_cache_key_bundle(&raw_inputs, &parsed_txs, history_slot);
    let (decode_cache_dir, offline) =
        crate::core::cache::resolve_cache_state(cache, &cache_dir, refresh_cache, &cache_key);
    let cache_read_dir_for_load = cache_read_dir(decode_cache_dir, refresh_cache);
    log::info!("Successfully parsed {} transactions", parsed_txs.len());

    let prepared = prepare_accounts_and_idls(
        rpc_url,
        cache_read_dir_for_load,
        offline,
        &parsed_txs,
        parser_registry,
        no_idl_fetch,
        progress,
        history_slot,
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
