use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_signature::Signature;
use solana_transaction_status_client_types::{TransactionConfirmationStatus, TransactionStatus};

use crate::cli::{SendArgs, WaitCommitmentArg};
use crate::core::transaction;

pub(crate) fn handle(args: SendArgs) -> Result<()> {
    let parsed = transaction::parse_raw_transaction(&args.tx)?;
    let client = RpcClient::new(&args.rpc.rpc_url);

    let config =
        RpcSendTransactionConfig { skip_preflight: args.skip_preflight, ..Default::default() };

    let signature = client
        .send_transaction_with_config(&parsed.transaction, config)
        .context("Failed to send transaction")?;

    let signature_text = signature.to_string();
    let explorer_url = build_explorer_url(&signature_text, &args.rpc.rpc_url);
    let wait_info = if args.wait {
        let wait_commitment = args.wait_commitment.unwrap_or(WaitCommitmentArg::Confirmed);
        let wait_timeout = Duration::from_secs(args.wait_timeout_secs.unwrap_or(30));
        Some(wait_for_confirmation(&client, &signature, wait_commitment, wait_timeout)?)
    } else {
        None
    };

    let output = render_send_output(&signature_text, &explorer_url, wait_info.as_deref());
    println!("{}", output);

    Ok(())
}

/// Inferred network cluster from RPC URL. Mainnet uses no query param.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ExplorerCluster {
    Mainnet,
    Devnet,
    Testnet,
}

fn infer_cluster_from_rpc_url(rpc_url: &str) -> ExplorerCluster {
    let lower = rpc_url.to_lowercase();
    // Match host patterns: api.devnet.solana.com, rpc.ankr.com/solana_devnet, etc.
    if lower.contains("devnet") {
        ExplorerCluster::Devnet
    } else if lower.contains("testnet") {
        ExplorerCluster::Testnet
    } else {
        ExplorerCluster::Mainnet
    }
}

fn build_explorer_url(signature: &str, rpc_url: &str) -> String {
    let cluster = infer_cluster_from_rpc_url(rpc_url);
    match cluster {
        ExplorerCluster::Mainnet => format!("https://explorer.solana.com/tx/{signature}"),
        ExplorerCluster::Devnet => {
            format!("https://explorer.solana.com/tx/{signature}?cluster=devnet")
        }
        ExplorerCluster::Testnet => {
            format!("https://explorer.solana.com/tx/{signature}?cluster=testnet")
        }
    }
}

fn render_send_output(signature: &str, explorer_url: &str, wait_info: Option<&str>) -> String {
    let mut out = format!("{signature}\n{explorer_url}");
    if let Some(info) = wait_info {
        out.push('\n');
        out.push_str(info);
    }
    out
}

fn wait_for_confirmation(
    client: &RpcClient,
    signature: &Signature,
    wait_commitment: WaitCommitmentArg,
    wait_timeout: Duration,
) -> Result<String> {
    let started = Instant::now();
    let required_level = wait_commitment_level(wait_commitment);

    loop {
        let statuses = client
            .get_signature_statuses(std::slice::from_ref(signature))
            .context("Failed to fetch transaction confirmation status")?;

        if let Some(status) = statuses.value.into_iter().next().flatten() {
            if let Some(err) = status.err {
                return Err(anyhow!("Transaction failed before confirmation: {err:?}"));
            }

            if status_commitment_level(&status) >= required_level {
                return Ok(format!(
                    "Transaction confirmed at {} commitment.",
                    status_commitment_label(&status)
                ));
            }
        }

        if started.elapsed() >= wait_timeout {
            return Err(anyhow!(
                "Timed out after {}s while waiting for {} commitment confirmation",
                wait_timeout.as_secs(),
                wait_commitment_name(wait_commitment)
            ));
        }

        std::thread::sleep(Duration::from_millis(500));
    }
}

fn wait_commitment_level(wait_commitment: WaitCommitmentArg) -> u8 {
    match wait_commitment {
        WaitCommitmentArg::Processed => 0,
        WaitCommitmentArg::Confirmed => 1,
        WaitCommitmentArg::Finalized => 2,
    }
}

fn wait_commitment_name(wait_commitment: WaitCommitmentArg) -> &'static str {
    match wait_commitment {
        WaitCommitmentArg::Processed => "processed",
        WaitCommitmentArg::Confirmed => "confirmed",
        WaitCommitmentArg::Finalized => "finalized",
    }
}

fn status_commitment_level(status: &TransactionStatus) -> u8 {
    match status.confirmation_status {
        Some(TransactionConfirmationStatus::Processed) => 0,
        Some(TransactionConfirmationStatus::Confirmed) => 1,
        Some(TransactionConfirmationStatus::Finalized) => 2,
        None if status.confirmations.is_none() => 2,
        None => 0,
    }
}

fn status_commitment_label(status: &TransactionStatus) -> &'static str {
    match status.confirmation_status {
        Some(TransactionConfirmationStatus::Processed) => "processed",
        Some(TransactionConfirmationStatus::Confirmed) => "confirmed",
        Some(TransactionConfirmationStatus::Finalized) => "finalized",
        None if status.confirmations.is_none() => "finalized",
        None => "processed",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ExplorerCluster, build_explorer_url, infer_cluster_from_rpc_url, render_send_output,
    };

    #[test]
    fn build_explorer_url_mainnet_no_cluster_param() {
        let signature = "4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ";
        let explorer_url = build_explorer_url(signature, "https://api.mainnet-beta.solana.com");
        assert_eq!(explorer_url, "https://explorer.solana.com/tx/4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ");
    }

    #[test]
    fn build_explorer_url_devnet_adds_cluster_param() {
        let signature = "4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ";
        let explorer_url = build_explorer_url(signature, "https://api.devnet.solana.com");
        assert_eq!(
            explorer_url,
            "https://explorer.solana.com/tx/4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ?cluster=devnet"
        );
    }

    #[test]
    fn build_explorer_url_testnet_adds_cluster_param() {
        let signature = "4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ";
        let explorer_url = build_explorer_url(signature, "https://api.testnet.solana.com");
        assert_eq!(
            explorer_url,
            "https://explorer.solana.com/tx/4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ?cluster=testnet"
        );
    }

    #[test]
    fn build_explorer_url_unrecognized_defaults_to_mainnet() {
        let signature = "4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ";
        let explorer_url =
            build_explorer_url(signature, "https://my-custom-rpc.example.com/v1/abc123");
        assert_eq!(explorer_url, "https://explorer.solana.com/tx/4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ");
    }

    #[test]
    fn infer_cluster_devnet() {
        assert_eq!(
            infer_cluster_from_rpc_url("https://api.devnet.solana.com"),
            ExplorerCluster::Devnet
        );
        assert_eq!(
            infer_cluster_from_rpc_url("https://rpc.ankr.com/solana_devnet"),
            ExplorerCluster::Devnet
        );
    }

    #[test]
    fn infer_cluster_testnet() {
        assert_eq!(
            infer_cluster_from_rpc_url("https://api.testnet.solana.com"),
            ExplorerCluster::Testnet
        );
    }

    #[test]
    fn infer_cluster_mainnet() {
        assert_eq!(
            infer_cluster_from_rpc_url("https://api.mainnet-beta.solana.com"),
            ExplorerCluster::Mainnet
        );
        assert_eq!(infer_cluster_from_rpc_url("https://example.com/rpc"), ExplorerCluster::Mainnet);
    }

    #[test]
    fn render_send_output_default_mode_stdout_only() {
        let rendered = render_send_output(
            "4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ",
            "https://explorer.solana.com/tx/4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ",
            None,
        );

        assert_eq!(
            rendered,
            "4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ\nhttps://explorer.solana.com/tx/4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ"
        );
    }

    #[test]
    fn render_send_output_wait_mode_appends_confirmation_to_stdout() {
        let rendered = render_send_output(
            "4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ",
            "https://explorer.solana.com/tx/4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ",
            Some("Transaction confirmed at finalized commitment."),
        );

        assert_eq!(
            rendered,
            "4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ\nhttps://explorer.solana.com/tx/4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ\nTransaction confirmed at finalized commitment."
        );
    }
}
