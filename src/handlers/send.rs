use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_signature::Signature;
use solana_transaction_status_client_types::{TransactionConfirmationStatus, TransactionStatus};

use crate::cli::{SendArgs, WaitCommitmentArg};
use crate::transaction;

pub(crate) fn handle(args: SendArgs) -> Result<()> {
    let parsed = transaction::parse_raw_transaction(&args.tx)?;
    let client = RpcClient::new(&args.rpc.rpc_url);

    let config =
        RpcSendTransactionConfig { skip_preflight: args.skip_preflight, ..Default::default() };

    let signature = client
        .send_transaction_with_config(&parsed.transaction, config)
        .context("Failed to send transaction")?;

    let signature_text = signature.to_string();
    let explorer_url = build_explorer_url(&signature_text);
    let wait_info = if args.wait {
        let wait_commitment = args.wait_commitment.unwrap_or(WaitCommitmentArg::Confirmed);
        let wait_timeout = Duration::from_secs(args.wait_timeout_secs.unwrap_or(30));
        Some(wait_for_confirmation(&client, &signature, wait_commitment, wait_timeout)?)
    } else {
        None
    };

    let output = render_send_output(&signature_text, &explorer_url, wait_info.as_deref());
    println!("{}", output.stdout);
    if let Some(stderr) = output.stderr {
        eprintln!("{stderr}");
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SendOutput {
    stdout: String,
    stderr: Option<String>,
}

fn build_explorer_url(signature: &str) -> String {
    format!("https://explorer.solana.com/tx/{signature}")
}

fn render_send_output(signature: &str, explorer_url: &str, wait_info: Option<&str>) -> SendOutput {
    SendOutput {
        stdout: format!("{signature}\n{explorer_url}"),
        stderr: wait_info.map(ToOwned::to_owned),
    }
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
    use super::{build_explorer_url, render_send_output};

    #[test]
    fn build_explorer_url_returns_solana_explorer_tx_link() {
        let signature = "4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ";
        let explorer_url = build_explorer_url(signature);
        assert_eq!(explorer_url, "https://explorer.solana.com/tx/4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ");
    }

    #[test]
    fn render_send_output_default_mode_keeps_stderr_empty() {
        let rendered = render_send_output(
            "4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ",
            "https://explorer.solana.com/tx/4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ",
            None,
        );

        assert_eq!(
            rendered.stdout,
            "4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ\nhttps://explorer.solana.com/tx/4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ"
        );
        assert_eq!(rendered.stderr, None);
    }

    #[test]
    fn render_send_output_wait_mode_puts_confirmation_into_stderr() {
        let rendered = render_send_output(
            "4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ",
            "https://explorer.solana.com/tx/4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ",
            Some("Transaction confirmed at finalized commitment."),
        );

        assert_eq!(
            rendered.stdout,
            "4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ\nhttps://explorer.solana.com/tx/4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZ"
        );
        assert_eq!(
            rendered.stderr,
            Some("Transaction confirmed at finalized commitment.".to_string())
        );
    }
}
