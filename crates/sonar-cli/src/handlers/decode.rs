use anyhow::Result;

use crate::cli::{DecodeArgs, TransactionInputArgs};
use crate::output;
use crate::parsers::instruction::ParserRegistry;
use crate::utils::progress::Progress;

use super::common::{CachePrepareArgs, resolve_and_derive_cache_key, resolve_cache_and_prepare};

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
    let rpc_batch_size = rpc.rpc_batch_size;
    let rpc_url = rpc.rpc_url;
    let resolver_cache_location =
        if refresh_cache { None } else { Some(super::common::build_cache_location(&cache_dir)) };
    let TransactionInputArgs { tx } = transaction;

    let resolved =
        resolve_and_derive_cache_key(tx, &rpc_url, resolver_cache_location, &progress, history_slot)?;
    let is_bundle = resolved.resolved_txs.len() > 1;
    let parsed_txs: Vec<_> =
        resolved.resolved_txs.into_iter().map(|entry| entry.parsed_tx).collect();

    let cache_args = CachePrepareArgs {
        rpc_url: &rpc_url,
        cache_enabled: !no_cache,
        cache_dir: &cache_dir,
        refresh_cache,
        no_idl_fetch,
        rpc_batch_size,
        history_slot,
    };
    let cached = resolve_cache_and_prepare(
        &cache_args,
        &resolved.cache_key,
        &parsed_txs,
        &mut parser_registry,
        &progress,
    )?;

    progress.finish();

    if is_bundle {
        log::info!("Bundle decode mode: {} transactions", parsed_txs.len());
        if json {
            output::render_decode_bundle_json(
                &parsed_txs,
                &cached.prepared.resolved_accounts,
                &mut parser_registry,
            )?;
        } else {
            for (i, parsed_tx) in parsed_txs.iter().enumerate() {
                output::render_transaction_only(
                    parsed_tx,
                    &cached.prepared.resolved_accounts,
                    &mut parser_registry,
                    false,
                    ix_data,
                    Some((i + 1, parsed_txs.len())),
                )?;
            }
        }
    } else {
        output::render_transaction_only(
            &parsed_txs[0],
            &cached.prepared.resolved_accounts,
            &mut parser_registry,
            json,
            ix_data,
            None,
        )?;
    }

    Ok(())
}
