use anyhow::Result;

use crate::cli::{DecodeArgs, TransactionInputArgs};
use crate::output;
use crate::parsers::instruction::ParserRegistry;
use crate::utils::progress::Progress;

use super::pipeline_prep::{CachePrepareArgs, TxSource, resolve_mutate_prepare};

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
    } = args;
    let rpc_batch_size = rpc.rpc_batch_size;
    let rpc_url = rpc.rpc_url;
    let resolver_cache_location = if refresh_cache {
        None
    } else {
        Some(super::pipeline_prep::build_cache_location(&cache_dir))
    };
    let TransactionInputArgs { tx } = transaction;

    let cache_args = CachePrepareArgs {
        rpc_url: &rpc_url,
        cache_enabled: !no_cache,
        cache_dir: &cache_dir,
        refresh_cache,
        no_idl_fetch,
        rpc_batch_size,
    };
    // Decode does not mutate the transaction, so the mutation hook is a no-op.
    let prepared = resolve_mutate_prepare(
        TxSource::Raw(tx),
        resolver_cache_location,
        &cache_args,
        &mut parser_registry,
        &progress,
        |_| Ok(()),
    )?;
    let parsed_txs = prepared.parsed_txs;
    let resolved_accounts = prepared.resolved_accounts;
    let is_bundle = parsed_txs.len() > 1;

    progress.finish();

    if is_bundle {
        log::info!("Bundle decode mode: {} transactions", parsed_txs.len());
    }

    output::render_decode(output::DecodeRender {
        parsed_txs: &parsed_txs,
        resolved: &resolved_accounts,
        registry: &mut parser_registry,
        show_ix_data: ix_data,
        json,
    })?;

    Ok(())
}
