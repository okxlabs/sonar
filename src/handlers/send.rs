use anyhow::{Context, Result};

use crate::cli::SendArgs;
use crate::transaction;

pub(crate) fn handle(args: SendArgs) -> Result<()> {
    use solana_client::rpc_client::RpcClient;
    use solana_client::rpc_config::RpcSendTransactionConfig;

    let parsed = transaction::parse_raw_transaction(&args.tx)?;
    let client = RpcClient::new(&args.rpc.rpc_url);

    let config =
        RpcSendTransactionConfig { skip_preflight: args.skip_preflight, ..Default::default() };

    let signature = client
        .send_transaction_with_config(&parsed.transaction, config)
        .context("Failed to send transaction")?;

    println!("{}", signature);
    Ok(())
}
