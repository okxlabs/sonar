use anyhow::Result;

use crate::cli::{DecodeArgs, TransactionInputArgs};
use crate::parsers::instruction::ParserRegistry;
use crate::utils::progress::Progress;
use crate::{core::account_loader, core::transaction, output};

use super::{auto_fetch_missing_idls, collect_program_ids};

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

    // Single tx: take the first positional arg, or fall back to stdin
    let tx_single = tx.into_iter().next();
    let raw_input = transaction::read_raw_transaction(tx_single)?;

    let parsed_tx = transaction::parse_transaction_input(&raw_input, &rpc_url, Some(&progress))?;

    let mut account_loader =
        account_loader::create_loader(rpc_url, None, false, Some(progress.clone()))?;
    let resolved_accounts = account_loader.load_for_transaction(&parsed_tx.transaction)?;

    let program_ids = collect_program_ids(&resolved_accounts);
    if program_ids.is_empty() {
        log::error!("No executable accounts found after RPC load; skipping IDL parsing");
    } else {
        if !no_idl_fetch {
            let idl_fetcher =
                account_loader::create_idl_fetcher(&account_loader, Some(progress.clone()));
            match auto_fetch_missing_idls(
                &idl_fetcher,
                &parser_registry,
                &program_ids,
                &resolved_accounts,
                Some(&progress),
            ) {
                Ok(count) if count > 0 => log::info!("Auto-fetched {} missing IDLs", count),
                Ok(_) => {}
                Err(err) => log::warn!("Failed to auto-fetch missing IDLs: {:#}", err),
            }
        }

        match parser_registry.load_idl_parsers_for_programs(program_ids) {
            Ok(count) if count > 0 => log::info!("Lazy-loaded {} IDL parsers", count),
            Ok(_) => {}
            Err(err) => log::warn!("Failed to load IDL parsers: {:?}", err),
        }
    }

    progress.finish();
    output::render_transaction_only(
        &parsed_tx,
        &resolved_accounts,
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

    let parsed_txs =
        transaction::parse_multi_raw_transactions(&tx_inputs, rpc_url, Some(progress))?;
    log::info!("Successfully parsed {} transactions", parsed_txs.len());

    let tx_refs: Vec<_> = parsed_txs.iter().map(|p| &p.transaction).collect();

    let mut account_loader =
        account_loader::create_loader(rpc_url.to_string(), None, false, Some(progress.clone()))?;
    let resolved_accounts = account_loader.load_for_transactions(&tx_refs)?;

    let program_ids = collect_program_ids(&resolved_accounts);
    if !program_ids.is_empty() {
        if !no_idl_fetch {
            let idl_fetcher =
                account_loader::create_idl_fetcher(&account_loader, Some(progress.clone()));
            match auto_fetch_missing_idls(
                &idl_fetcher,
                parser_registry,
                &program_ids,
                &resolved_accounts,
                Some(progress),
            ) {
                Ok(count) if count > 0 => log::info!("Auto-fetched {} missing IDLs", count),
                Ok(_) => {}
                Err(err) => log::warn!("Failed to auto-fetch missing IDLs: {:#}", err),
            }
        }

        match parser_registry.load_idl_parsers_for_programs(program_ids) {
            Ok(count) if count > 0 => log::info!("Lazy-loaded {} IDL parsers", count),
            Ok(_) => {}
            Err(err) => log::warn!("Failed to load IDL parsers: {:?}", err),
        }
    }

    progress.finish();

    if json {
        output::render_decode_bundle_json(&parsed_txs, &resolved_accounts, parser_registry)?;
    } else {
        for (i, parsed_tx) in parsed_txs.iter().enumerate() {
            output::render_transaction_only(
                parsed_tx,
                &resolved_accounts,
                parser_registry,
                false,
                ix_data,
                Some((i + 1, parsed_txs.len())),
            )?;
        }
    }

    Ok(())
}
