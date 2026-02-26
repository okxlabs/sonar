use std::path::PathBuf;

use anyhow::Result;

use crate::cli::{DecodeArgs, TransactionInputArgs};
use crate::output;
use crate::parsers::instruction::ParserRegistry;
use crate::utils::progress::Progress;

use super::{prepare_accounts_and_idls, resolve_inputs_to_txs};

fn cache_read_dir(cache_dir: Option<PathBuf>, refresh_cache: bool) -> Option<PathBuf> {
    if refresh_cache { None } else { cache_dir }
}

fn resolve_decode_cache_state_single(
    cache: bool,
    cache_dir: &Option<PathBuf>,
    refresh_cache: bool,
    input: &str,
    parsed_tx: &crate::core::transaction::ParsedTransaction,
) -> (Option<PathBuf>, bool) {
    let cache_key = crate::core::cache::derive_cache_key_single(input, &parsed_tx.transaction);
    crate::core::cache::resolve_cache_state(cache, cache_dir, refresh_cache, &cache_key)
}

fn resolve_decode_cache_state_bundle(
    cache: bool,
    cache_dir: &Option<PathBuf>,
    refresh_cache: bool,
    inputs: &[String],
    parsed_txs: &[crate::core::transaction::ParsedTransaction],
) -> (Option<PathBuf>, bool) {
    let cache_key = crate::core::cache::derive_cache_key_bundle(inputs, parsed_txs);
    crate::core::cache::resolve_cache_state(cache, cache_dir, refresh_cache, &cache_key)
}

pub(crate) fn handle(args: DecodeArgs) -> Result<()> {
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
    } = args;
    let rpc_url = rpc.rpc_url;
    let resolver_cache_root =
        if refresh_cache { None } else { Some(crate::core::cache::resolve_cache_dir(&cache_dir)) };
    let TransactionInputArgs { tx, json } = transaction;

    // Check if this is a bundle (multiple positional TX arguments)
    if tx.len() > 1 {
        return handle_bundle(
            tx,
            &rpc_url,
            resolver_cache_root,
            !no_cache,
            cache_dir,
            refresh_cache,
            ix_data,
            json,
            no_idl_fetch,
            &mut parser_registry,
            &progress,
        );
    }

    let parsed_inputs = resolve_inputs_to_txs(tx, &rpc_url, resolver_cache_root, &progress, false)?;
    let resolved_tx = parsed_inputs
        .resolved_txs
        .into_iter()
        .next()
        .expect("single input resolve should produce one transaction");
    let original_input = resolved_tx.original_input;
    let parsed_tx = resolved_tx.parsed_tx;
    let (decode_cache_dir, offline) = resolve_decode_cache_state_single(
        !no_cache,
        &cache_dir,
        refresh_cache,
        &original_input,
        &parsed_tx,
    );
    let cache_read_dir_for_load = cache_read_dir(decode_cache_dir, refresh_cache);

    let prepared = prepare_accounts_and_idls(
        &rpc_url,
        cache_read_dir_for_load,
        offline,
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
    resolver_cache_root: Option<PathBuf>,
    cache: bool,
    cache_dir: Option<PathBuf>,
    refresh_cache: bool,
    ix_data: bool,
    json: bool,
    no_idl_fetch: bool,
    parser_registry: &mut ParserRegistry,
    progress: &Progress,
) -> Result<()> {
    log::info!("Bundle decode mode: {} transactions", tx_inputs.len());

    let parsed_inputs =
        resolve_inputs_to_txs(tx_inputs, rpc_url, resolver_cache_root, progress, true)?;
    let resolved_txs = parsed_inputs.resolved_txs;
    let raw_inputs: Vec<_> =
        resolved_txs.iter().map(|entry| entry.original_input.clone()).collect();
    let parsed_txs: Vec<_> = resolved_txs.into_iter().map(|entry| entry.parsed_tx).collect();
    let (decode_cache_dir, offline) = resolve_decode_cache_state_bundle(
        cache,
        &cache_dir,
        refresh_cache,
        &raw_inputs,
        &parsed_txs,
    );
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
