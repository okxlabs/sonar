//! Build, sign, send, and confirm batches of v1 (SIMD-0385) transactions.
//!
//! The sender mirrors the Solana CLI's parallel send loop: every pending
//! transaction is signed with one blockhash and submitted, then signature
//! statuses are polled (resending periodically) until each is confirmed or the
//! blockhash expires. Unconfirmed transactions are re-signed with a fresh
//! blockhash, up to a bounded number of sign attempts. A transaction that lands
//! with an error aborts the batch.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use solana_commitment_config::CommitmentConfig;
use solana_hash::Hash;
use solana_instruction::Instruction;
use solana_message::VersionedMessage;
use solana_message::v1::{self, TransactionConfig};
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;

use crate::core::rpc_client::{RpcClient, SendTransactionConfig};
use crate::utils::progress::Progress;

/// Maximum serialized size of a v1 transaction.
pub const MAX_V1_TRANSACTION_SIZE: usize = v1::MAX_TRANSACTION_SIZE;

/// Base fee charged per signature, in lamports.
pub const LAMPORTS_PER_SIGNATURE: u64 = 5_000;

/// Maximum signatures accepted by one `getSignatureStatuses` call.
const MAX_STATUSES_PER_REQUEST: usize = 256;

/// Concurrent `sendTransaction` requests.
const SEND_CONCURRENCY: usize = 8;

/// Delay between signature status polls.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Resend unconfirmed transactions this often while their blockhash is valid.
const RESEND_INTERVAL: Duration = Duration::from_secs(4);

/// One transaction to send: its instructions and the keypairs that sign it.
///
/// The fee payer is the first signer.
pub struct UnsignedV1Transaction<'a> {
    pub instructions: Vec<Instruction>,
    pub signers: Vec<&'a dyn Signer>,
}

/// How a batch is sent and confirmed.
#[derive(Debug, Clone, Copy)]
pub struct V1SendOptions {
    /// v1 header config applied to every transaction in the batch.
    pub tx_config: TransactionConfig,
    /// Times a pending transaction may be signed with a fresh blockhash.
    pub max_sign_attempts: u32,
    pub skip_preflight: bool,
    pub commitment: CommitmentConfig,
}

/// Compile and sign a v1 transaction, rejecting one that exceeds the v1 size limit.
///
/// Signers are deduplicated by address, so the same keypair may be passed in
/// several roles (fee payer and buffer authority, for example).
pub fn build_v1_transaction(
    fee_payer: &Pubkey,
    instructions: &[Instruction],
    blockhash: Hash,
    config: TransactionConfig,
    signers: &[&dyn Signer],
) -> Result<VersionedTransaction> {
    let message = v1::Message::try_compile_with_config(fee_payer, instructions, blockhash, config)
        .map_err(|e| anyhow!("Failed to compile v1 message: {e}"))?;
    let mut unique: Vec<&dyn Signer> = Vec::with_capacity(signers.len());
    for signer in signers {
        if !unique.iter().any(|known| known.pubkey() == signer.pubkey()) {
            unique.push(*signer);
        }
    }
    let tx = VersionedTransaction::try_new(VersionedMessage::V1(message), unique.as_slice())
        .map_err(|e| anyhow!("Failed to sign v1 transaction: {e}"))?;
    let size = v1_wire_bytes(&tx)?.len();
    if size > MAX_V1_TRANSACTION_SIZE {
        bail!("v1 transaction is {size} bytes, over the {MAX_V1_TRANSACTION_SIZE}-byte limit");
    }
    Ok(tx)
}

/// Wire bytes of a v1 transaction: the message, then the fixed-length signature array.
pub fn v1_wire_bytes(tx: &VersionedTransaction) -> Result<Vec<u8>> {
    wincode::serialize(tx).context("Failed to serialize v1 transaction")
}

/// Serialized size of a v1 transaction with these instructions, without signing it.
pub fn v1_transaction_size(
    fee_payer: &Pubkey,
    instructions: &[Instruction],
    config: TransactionConfig,
) -> Result<usize> {
    let message =
        v1::Message::try_compile_with_config(fee_payer, instructions, Hash::default(), config)
            .map_err(|e| anyhow!("Failed to compile v1 message: {e}"))?;
    let signatures = usize::from(message.header.num_required_signatures);
    // Version byte + message body + trailing signature array.
    Ok(1 + message.size() + signatures * v1::SIGNATURE_SIZE)
}

/// Send every transaction and wait until each one is confirmed.
///
/// `label` prefixes the progress messages.
pub fn send_and_confirm(
    client: &RpcClient,
    transactions: &[UnsignedV1Transaction<'_>],
    options: V1SendOptions,
    progress: &Progress,
    label: &str,
) -> Result<()> {
    let total = transactions.len();
    let mut pending: Vec<usize> = (0..total).collect();
    let mut confirmed = 0usize;

    for attempt in 1..=options.max_sign_attempts {
        if pending.is_empty() {
            break;
        }
        progress.set_message(format!(
            "{label}: preparing {} transaction(s) (attempt {attempt}/{})",
            pending.len(),
            options.max_sign_attempts
        ));
        let latest = client
            .get_latest_blockhash(options.commitment)
            .context("Failed to fetch latest blockhash")?;

        let signed: Vec<SignedTransaction> = pending
            .iter()
            .map(|&index| {
                let tx = &transactions[index];
                let fee_payer = tx
                    .signers
                    .first()
                    .ok_or_else(|| anyhow!("transaction {index} has no fee payer"))?
                    .pubkey();
                let built = build_v1_transaction(
                    &fee_payer,
                    &tx.instructions,
                    latest.blockhash,
                    options.tx_config,
                    &tx.signers,
                )?;
                Ok(SignedTransaction {
                    index,
                    signature: built.signatures[0],
                    wire: v1_wire_bytes(&built)?,
                })
            })
            .collect::<Result<_>>()?;

        progress.set_message(format!(
            "{label}: sending {} transaction(s) ({confirmed}/{total} confirmed, attempt {attempt}/{})",
            signed.len(),
            options.max_sign_attempts
        ));
        let send_errors = send_all(
            client,
            &signed,
            send_config(options.skip_preflight, options.commitment),
            progress,
            label,
        );
        if send_errors.len() == signed.len() {
            // Nothing was accepted: the node is rejecting the transactions
            // themselves, and waiting for the blockhash to expire cannot help.
            let first = send_errors.into_iter().next().expect("at least one error");
            return Err(first).context("Failed to send transaction");
        }
        for err in &send_errors {
            log::warn!("Failed to send transaction: {err:#}");
        }

        let mut unconfirmed: Vec<&SignedTransaction> = signed.iter().collect();
        let mut last_send = Instant::now();
        loop {
            std::thread::sleep(POLL_INTERVAL);
            let mut still_pending = Vec::with_capacity(unconfirmed.len());
            for batch in unconfirmed.chunks(MAX_STATUSES_PER_REQUEST) {
                let signatures: Vec<Signature> = batch.iter().map(|tx| tx.signature).collect();
                let statuses = client
                    .get_signature_statuses(&signatures)
                    .context("Failed to fetch transaction statuses")?
                    .value;
                for (tx, status) in
                    batch.iter().zip(statuses.into_iter().chain(std::iter::repeat(None)))
                {
                    match status {
                        Some(status) if status.err.is_some() => {
                            bail!(
                                "Transaction {} failed: {:?}",
                                tx.signature,
                                status.err.expect("checked above")
                            );
                        }
                        Some(status) if status.satisfies_commitment(options.commitment) => {
                            confirmed += 1;
                        }
                        _ => still_pending.push(*tx),
                    }
                }
            }
            unconfirmed = still_pending;
            progress.set_message(format!(
                "{label}: {confirmed}/{total} transactions confirmed ({}%)",
                confirmed * 100 / total
            ));
            if unconfirmed.is_empty() {
                break;
            }

            let block_height = client
                .get_block_height(options.commitment)
                .context("Failed to fetch block height")?;
            if block_height > latest.last_valid_block_height {
                log::info!(
                    "Blockhash expired with {} transaction(s) unconfirmed; re-signing",
                    unconfirmed.len()
                );
                break;
            }
            if last_send.elapsed() >= RESEND_INTERVAL {
                progress.set_message(format!(
                    "{label}: resending {} unconfirmed transaction(s)",
                    unconfirmed.len()
                ));
                let resend: Vec<SignedTransaction> =
                    unconfirmed.iter().map(|tx| (*tx).clone()).collect();
                for err in send_all(
                    client,
                    &resend,
                    send_config(true, options.commitment),
                    progress,
                    label,
                ) {
                    log::debug!("Resend failed: {err:#}");
                }
                last_send = Instant::now();
            }
        }
        pending = unconfirmed.iter().map(|tx| tx.index).collect();
    }

    if !pending.is_empty() {
        bail!(
            "{} of {total} transaction(s) were not confirmed after {} sign attempt(s)",
            pending.len(),
            options.max_sign_attempts
        );
    }
    Ok(())
}

#[derive(Clone)]
struct SignedTransaction {
    index: usize,
    signature: Signature,
    wire: Vec<u8>,
}

/// `sendTransaction` options for a transaction signed with a blockhash fetched
/// at `commitment`.
///
/// Preflight has to simulate against a bank at least as recent as the one the
/// blockhash came from. The RPC default is `finalized`, which may not know a
/// `confirmed` blockhash yet and would reject the transaction with
/// `BlockhashNotFound`, so the commitment is always set explicitly.
fn send_config(skip_preflight: bool, commitment: CommitmentConfig) -> SendTransactionConfig {
    SendTransactionConfig { skip_preflight, preflight_commitment: Some(commitment) }
}

/// Submit transactions concurrently, returning the errors of those that failed.
fn send_all(
    client: &RpcClient,
    transactions: &[SignedTransaction],
    config: SendTransactionConfig,
    progress: &Progress,
    label: &str,
) -> Vec<anyhow::Error> {
    if transactions.is_empty() {
        return Vec::new();
    }
    let per_worker = transactions.len().div_ceil(SEND_CONCURRENCY);
    let completed = Mutex::new((0usize, 0usize));
    let completed = &completed;
    std::thread::scope(|scope| {
        let workers: Vec<_> = transactions
            .chunks(per_worker)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|tx| {
                            let result = client
                                .send_transaction_with_config(&tx.wire, config)
                                .with_context(|| format!("transaction {}", tx.signature));
                            let mut counts = completed.lock().unwrap();
                            counts.0 += 1;
                            counts.1 += usize::from(result.is_ok());
                            progress.set_message(format!(
                                "{label}: submitted {}/{} transaction(s), {} accepted by RPC",
                                counts.0,
                                transactions.len(),
                                counts.1
                            ));
                            result.err()
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| {
                worker.join().unwrap_or_else(|_| vec![anyhow!("send worker panicked")])
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use solana_instruction::AccountMeta;
    use solana_keypair::Keypair;

    use super::*;

    fn memo_like(data_len: usize, signer: &Pubkey) -> Instruction {
        Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![AccountMeta::new(*signer, true)],
            data: vec![7; data_len],
        }
    }

    #[test]
    fn built_transactions_round_trip_through_the_v1_parser_with_valid_signatures() {
        let payer = Keypair::new();
        let ix = memo_like(100, &payer.pubkey());
        let config = TransactionConfig::empty()
            .with_compute_unit_limit(1_000)
            .with_loaded_accounts_data_size_limit(10_000);
        let tx = build_v1_transaction(
            &payer.pubkey(),
            &[ix],
            Hash::new_unique(),
            config,
            &[&payer, &payer],
        )
        .expect("builds");

        let wire = v1_wire_bytes(&tx).unwrap();
        assert_eq!(wire[0], 0x81, "v1 version byte");
        let parsed = sonar_sim::V1Transaction::parse(&wire).expect("strict v1 parse");
        parsed.verify_signatures().expect("signatures cover the v1 payload");
        assert_eq!(parsed.config.compute_unit_limit, Some(1_000));
        assert_eq!(parsed.config.loaded_accounts_data_size_limit, Some(10_000));
        assert_eq!(parsed.signatures.len(), 1, "duplicate signer collapsed");
    }

    #[test]
    fn size_estimate_matches_the_signed_wire_size() {
        let payer = Keypair::new();
        let other = Keypair::new();
        let ix = Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![
                AccountMeta::new(Pubkey::new_unique(), false),
                AccountMeta::new_readonly(other.pubkey(), true),
            ],
            data: vec![1; 321],
        };
        let config = TransactionConfig::empty().with_priority_fee(5).with_compute_unit_limit(9);
        let estimate = v1_transaction_size(&payer.pubkey(), std::slice::from_ref(&ix), config)
            .expect("estimates");
        let tx = build_v1_transaction(
            &payer.pubkey(),
            &[ix],
            Hash::new_unique(),
            config,
            &[&payer, &other],
        )
        .unwrap();
        assert_eq!(estimate, v1_wire_bytes(&tx).unwrap().len());
    }

    #[test]
    fn oversized_transactions_are_rejected() {
        let payer = Keypair::new();
        let ix = memo_like(MAX_V1_TRANSACTION_SIZE, &payer.pubkey());
        let err = build_v1_transaction(
            &payer.pubkey(),
            &[ix],
            Hash::new_unique(),
            TransactionConfig::empty(),
            &[&payer],
        )
        .expect_err("over the limit");
        assert!(err.to_string().contains("4096-byte limit"), "got: {err}");
    }
}
