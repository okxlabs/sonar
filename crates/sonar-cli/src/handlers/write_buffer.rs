//! `write-buffer`: upload a program ELF into a BPF Loader Upgradeable buffer
//! account using v1 transactions.
//!
//! The flow follows `solana program write-buffer`:
//!
//! 1. Create and initialize the buffer (or validate an existing one and top up
//!    its rent), in one transaction confirmed before any write.
//! 2. Split the program into `Write` chunks, skipping chunks an existing buffer
//!    already holds, pack as many chunks as fit into each transaction, and send
//!    the transactions in parallel.
//!
//! Every transaction is v1 (SIMD-0385), which may be 4096 bytes instead of
//! legacy's 1232. The loader still deserializes each instruction's data with a
//! 1232-byte limit, so a single `Write` cannot grow with the transaction; the
//! gain comes from carrying several full-size `Write` instructions per
//! transaction (three, about 3.6 KB of program data, against roughly 1 KB for a
//! legacy write), so a program needs about a quarter of the transactions.
//!
//! v1 replaces `ComputeBudget` instructions with header config fields, and an
//! absent field means the *minimum*, so the compute unit limit and the loaded
//! accounts data size limit are always set explicitly.

use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use solana_account::Account;
use solana_commitment_config::CommitmentConfig;
use solana_instruction::Instruction;
use solana_keypair::{Keypair, read_keypair_file};
use solana_loader_v3_interface::instruction as loader_instruction;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_message::v1::TransactionConfig;
use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;
use solana_signer::Signer;
use solana_system_interface::instruction as system_instruction;

use crate::cli::WriteBufferArgs;
use crate::core::rpc_client::RpcClient;
use crate::core::v1_sender::{
    self, LAMPORTS_PER_SIGNATURE, MAX_V1_TRANSACTION_SIZE, UnsignedV1Transaction, V1SendOptions,
};
use crate::output::stream::{errln, outln};
use crate::utils::config::expand_tilde;
use crate::utils::progress::Progress;

/// Leading bytes of every ELF file.
const ELF_MAGIC: &[u8; 4] = b"\x7fELF";

/// Largest instruction data the loader accepts. BPF Loader Upgradeable
/// deserializes instruction data with a `PACKET_DATA_SIZE` (1232-byte) limit,
/// independent of the transaction format.
const MAX_LOADER_INSTRUCTION_DATA: usize = 1232;

/// Room in the loaded accounts data size limit beyond the buffer itself, for
/// the fee payer, the authority, and the loader program account.
const LOADED_ACCOUNTS_DATA_MARGIN: usize = 1024 * 1024;

/// Largest loaded accounts data size a transaction may request (64 MiB).
const MAX_LOADED_ACCOUNTS_DATA_SIZE: usize = 64 * 1024 * 1024;

const MICRO_LAMPORTS_PER_LAMPORT: u128 = 1_000_000;

const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

#[derive(Serialize)]
struct WriteBufferOutput {
    buffer: String,
    buffer_authority: String,
    program_len: usize,
    max_len: usize,
    created: bool,
    /// Largest program data payload of one `Write` instruction.
    chunk_size: usize,
    write_instructions: usize,
    write_transactions: usize,
    skipped_chunks: usize,
}

/// The `--buffer` argument: a keypair can create the buffer, an address can
/// only resume an existing one.
enum BufferTarget {
    Signer(Keypair),
    Address(Pubkey),
}

impl BufferTarget {
    fn resolve(arg: Option<&str>) -> Result<Self> {
        match arg {
            None => Ok(Self::Signer(Keypair::new())),
            Some(raw) => match Pubkey::from_str(raw.trim()) {
                Ok(address) => Ok(Self::Address(address)),
                Err(_) => Ok(Self::Signer(read_keypair(raw, "--buffer")?)),
            },
        }
    }

    fn pubkey(&self) -> Pubkey {
        match self {
            Self::Signer(keypair) => keypair.pubkey(),
            Self::Address(address) => *address,
        }
    }
}

pub(crate) fn handle(args: WriteBufferArgs, json: bool) -> Result<()> {
    let program = read_program(&args.program)?;
    let program_len = program.len();
    let max_len = args.max_len.unwrap_or(program_len);
    if max_len < program_len {
        bail!("--max-len ({max_len}) is smaller than the program ({program_len} bytes)");
    }

    // The default signer is only required for the roles it fills.
    let default_signer = if args.fee_payer.is_none() || args.buffer_authority.is_none() {
        Some(read_keypair(&args.keypair, "--keypair")?)
    } else {
        None
    };
    let fee_payer_keypair =
        args.fee_payer.as_deref().map(|path| read_keypair(path, "--fee-payer")).transpose()?;
    let authority_keypair = args
        .buffer_authority
        .as_deref()
        .map(|path| read_keypair(path, "--buffer-authority"))
        .transpose()?;
    let fee_payer: &Keypair = fee_payer_keypair
        .as_ref()
        .or(default_signer.as_ref())
        .expect("default signer is loaded when --fee-payer is absent");
    let authority: &Keypair = authority_keypair
        .as_ref()
        .or(default_signer.as_ref())
        .expect("default signer is loaded when --buffer-authority is absent");
    let buffer = BufferTarget::resolve(args.buffer.as_deref())?;
    let buffer_address = buffer.pubkey();

    let client = RpcClient::new(&args.rpc.rpc_url);
    let commitment = CommitmentConfig::confirmed();
    let progress = Progress::new();
    progress.set_message(format!("Fetching buffer account {buffer_address}..."));

    let existing = client
        .get_account_with_commitment(&buffer_address, commitment)
        .with_context(|| format!("Failed to fetch buffer account {buffer_address}"))?
        .value;

    // Step 1: the transaction that makes the buffer ready to write, if any.
    let (initial, existing_program, rent_needed, buffer_account_len) = match &existing {
        None => {
            let BufferTarget::Signer(buffer_keypair) = &buffer else {
                bail!(
                    "Buffer account {buffer_address} does not exist; pass a buffer keypair file \
                     to --buffer (or omit --buffer) to create it"
                );
            };
            let buffer_account_len = UpgradeableLoaderState::size_of_buffer(max_len);
            let lamports = client
                .get_minimum_balance_for_rent_exemption(buffer_account_len)
                .context("Failed to fetch rent-exempt minimum")?;
            let instructions = loader_instruction::create_buffer(
                &fee_payer.pubkey(),
                &buffer_address,
                &authority.pubkey(),
                lamports,
                max_len,
            )
            .map_err(|e| anyhow!("Failed to build buffer creation instructions: {e}"))?;
            let signers: Vec<&dyn Signer> = vec![fee_payer, buffer_keypair];
            (
                Some(UnsignedV1Transaction { instructions, signers }),
                None,
                lamports,
                buffer_account_len,
            )
        }
        Some(account) => {
            let held =
                existing_program_bytes(buffer_address, account, &authority.pubkey(), program_len)?;
            let minimum = client
                .get_minimum_balance_for_rent_exemption(account.data.len())
                .context("Failed to fetch rent-exempt minimum")?;
            let top_up = minimum.saturating_sub(account.lamports);
            let initial = (top_up > 0).then(|| UnsignedV1Transaction {
                instructions: vec![system_instruction::transfer(
                    &fee_payer.pubkey(),
                    &buffer_address,
                    top_up,
                )],
                signers: vec![fee_payer as &dyn Signer],
            });
            (initial, Some(held), top_up, account.data.len())
        }
    };
    let created = existing.is_none();

    // Step 2: the writes.
    let tx_config = transaction_config(
        args.compute_unit_limit,
        loaded_accounts_data_size_limit(buffer_account_len),
        args.priority_fee,
        args.compute_unit_price,
    );
    let chunk_size = max_write_chunk_size(&buffer_address, &authority.pubkey())?;
    let chunks = plan_write_chunks(&program, existing_program, chunk_size);
    let skipped_chunks = program.chunks(chunk_size).count() - chunks.len();
    let write_instructions: Vec<Instruction> = chunks
        .iter()
        .map(|&(offset, bytes)| {
            loader_instruction::write(&buffer_address, &authority.pubkey(), offset, bytes.to_vec())
        })
        .collect();
    let write_instruction_count = write_instructions.len();
    let writes: Vec<UnsignedV1Transaction<'_>> =
        pack_instructions(&fee_payer.pubkey(), write_instructions, tx_config)?
            .into_iter()
            .map(|instructions| UnsignedV1Transaction {
                instructions,
                signers: vec![fee_payer as &dyn Signer, authority],
            })
            .collect();

    if !args.skip_fee_check {
        let required = initial
            .iter()
            .chain(&writes)
            .map(|tx| transaction_fee(tx, tx_config))
            .fold(rent_needed, u64::saturating_add);
        let balance = client
            .get_balance(&fee_payer.pubkey(), commitment)
            .context("Failed to fetch fee payer balance")?;
        if balance < required {
            bail!(
                "Fee payer {} has insufficient funds: {} SOL required, {} SOL available \
                 (use --skip-fee-check to send anyway)",
                fee_payer.pubkey(),
                format_sol(required),
                format_sol(balance)
            );
        }
    }

    let options = V1SendOptions {
        tx_config,
        max_sign_attempts: args.max_sign_attempts,
        skip_preflight: false,
        commitment,
    };
    if let Some(initial) = initial {
        let label = if created { "Creating buffer" } else { "Funding buffer" };
        v1_sender::send_and_confirm(&client, &[initial], options, &progress, label)
            .with_context(|| format!("Failed to prepare buffer account {buffer_address}"))?;
    }
    // Writes are independent, so preflight only costs a round trip per chunk.
    let write_options = V1SendOptions { skip_preflight: true, ..options };
    v1_sender::send_and_confirm(&client, &writes, write_options, &progress, "Writing buffer")
        .with_context(|| {
            format!(
                "Failed to write buffer {buffer_address}; rerun with `--buffer {buffer_address}` \
                 to resume (chunks already written are skipped)"
            )
        })?;
    progress.finish();

    if json {
        crate::output::print_json(&WriteBufferOutput {
            buffer: buffer_address.to_string(),
            buffer_authority: authority.pubkey().to_string(),
            program_len,
            max_len: buffer_account_len - UpgradeableLoaderState::size_of_buffer_metadata(),
            created,
            chunk_size,
            write_instructions: write_instruction_count,
            write_transactions: writes.len(),
            skipped_chunks,
        })?;
    } else {
        outln!("Buffer: {buffer_address}");
        errln!(
            "Wrote {program_len} bytes with {write_instruction_count} write(s) of up to \
             {chunk_size} bytes in {} v1 transaction(s){}",
            writes.len(),
            if skipped_chunks > 0 {
                format!(" ({skipped_chunks} chunk(s) already up to date)")
            } else {
                String::new()
            }
        );
    }
    Ok(())
}

fn read_keypair(path: &str, flag: &str) -> Result<Keypair> {
    let expanded = expand_tilde(path);
    read_keypair_file(&expanded)
        .map_err(|e| anyhow!("Failed to read {flag} keypair file {expanded}: {e}"))
}

fn read_program(path: &Path) -> Result<Vec<u8>> {
    let program = std::fs::read(path)
        .with_context(|| format!("Failed to read program file {}", path.display()))?;
    if program.is_empty() {
        bail!("Program file {} is empty", path.display());
    }
    if !program.starts_with(ELF_MAGIC) {
        bail!("Program file {} is not an ELF file", path.display());
    }
    Ok(program)
}

/// Validate an existing buffer and return the program bytes it already holds.
fn existing_program_bytes<'a>(
    address: Pubkey,
    account: &'a Account,
    authority: &Pubkey,
    program_len: usize,
) -> Result<&'a [u8]> {
    if account.owner != bpf_loader_upgradeable::id() {
        bail!(
            "Account {address} is not owned by BPF Loader Upgradeable (owner: {})",
            account.owner
        );
    }
    let state: UpgradeableLoaderState = wincode::deserialize(&account.data)
        .with_context(|| format!("Failed to deserialize upgradeable loader state of {address}"))?;
    let UpgradeableLoaderState::Buffer { authority_address } = state else {
        bail!("Account {address} is not a buffer account");
    };
    match authority_address.map(|key| Pubkey::new_from_array(key.to_bytes())) {
        None => bail!("Buffer {address} is immutable (it has no authority)"),
        Some(current) if current != *authority => bail!(
            "Buffer authority mismatch: buffer {address} has authority {current}, \
             but the provided authority is {authority}"
        ),
        Some(_) => {}
    }
    let minimum = UpgradeableLoaderState::size_of_buffer(program_len);
    if account.data.len() < minimum {
        bail!(
            "Buffer account data size ({}) is smaller than the minimum size ({minimum})",
            account.data.len()
        );
    }
    Ok(&account.data[UpgradeableLoaderState::size_of_buffer_metadata()..])
}

fn transaction_config(
    compute_unit_limit: u32,
    loaded_accounts_data_size_limit: u32,
    priority_fee: Option<u64>,
    compute_unit_price: Option<u64>,
) -> TransactionConfig {
    let config = TransactionConfig::empty()
        .with_compute_unit_limit(compute_unit_limit)
        .with_loaded_accounts_data_size_limit(loaded_accounts_data_size_limit);
    let fee = priority_fee.or_else(|| {
        compute_unit_price.map(|price| priority_fee_from_price(price, compute_unit_limit))
    });
    match fee {
        Some(fee) if fee > 0 => config.with_priority_fee(fee),
        _ => config,
    }
}

/// Convert a legacy compute unit price into the v1 total priority fee, rounding up.
fn priority_fee_from_price(micro_lamports_per_cu: u64, compute_unit_limit: u32) -> u64 {
    let micro_lamports = u128::from(micro_lamports_per_cu) * u128::from(compute_unit_limit);
    u64::try_from(micro_lamports.div_ceil(MICRO_LAMPORTS_PER_LAMPORT)).unwrap_or(u64::MAX)
}

fn loaded_accounts_data_size_limit(buffer_account_len: usize) -> u32 {
    let limit =
        (buffer_account_len + LOADED_ACCOUNTS_DATA_MARGIN).min(MAX_LOADED_ACCOUNTS_DATA_SIZE);
    u32::try_from(limit).expect("64 MiB fits in u32")
}

/// Largest `Write` payload whose instruction data the loader accepts.
fn max_write_chunk_size(buffer: &Pubkey, authority: &Pubkey) -> Result<usize> {
    // The payload length is encoded at a fixed width, so an empty write gives
    // the exact instruction data overhead.
    let overhead = loader_instruction::write(buffer, authority, 0, Vec::new()).data.len();
    MAX_LOADER_INSTRUCTION_DATA
        .checked_sub(overhead)
        .filter(|size| *size > 0)
        .ok_or_else(|| anyhow!("A write instruction leaves no room for program data"))
}

/// Group instructions, in order, into as few v1 transactions as the size limit allows.
fn pack_instructions(
    fee_payer: &Pubkey,
    instructions: Vec<Instruction>,
    config: TransactionConfig,
) -> Result<Vec<Vec<Instruction>>> {
    let mut transactions = Vec::new();
    let mut current: Vec<Instruction> = Vec::new();
    for instruction in instructions {
        current.push(instruction);
        if v1_sender::v1_transaction_size(fee_payer, &current, config)? <= MAX_V1_TRANSACTION_SIZE {
            continue;
        }
        let overflow = current.pop().expect("just pushed");
        if current.is_empty() {
            bail!("A single write instruction does not fit in a v1 transaction");
        }
        transactions.push(std::mem::take(&mut current));
        current.push(overflow);
    }
    if !current.is_empty() {
        transactions.push(current);
    }
    Ok(transactions)
}

/// Chunks to write as `(offset, bytes)`, skipping those `existing` already holds.
fn plan_write_chunks<'a>(
    program: &'a [u8],
    existing: Option<&[u8]>,
    chunk_size: usize,
) -> Vec<(u32, &'a [u8])> {
    program
        .chunks(chunk_size)
        .enumerate()
        .filter_map(|(index, chunk)| {
            let offset = index * chunk_size;
            let held = existing.and_then(|data| data.get(offset..offset + chunk.len()));
            if held == Some(chunk) {
                return None;
            }
            Some((u32::try_from(offset).expect("buffer offsets fit in u32"), chunk))
        })
        .collect()
}

fn transaction_fee(tx: &UnsignedV1Transaction<'_>, config: TransactionConfig) -> u64 {
    let mut signers: Vec<Pubkey> = tx.signers.iter().map(|signer| signer.pubkey()).collect();
    signers.sort();
    signers.dedup();
    (signers.len() as u64)
        .saturating_mul(LAMPORTS_PER_SIGNATURE)
        .saturating_add(config.priority_fee.unwrap_or(0))
}

fn format_sol(lamports: u64) -> String {
    format!("{}.{:09}", lamports / LAMPORTS_PER_SOL, lamports % LAMPORTS_PER_SOL)
}

#[cfg(test)]
mod tests {
    use solana_hash::Hash;

    use super::*;
    use crate::core::v1_sender::{build_v1_transaction, v1_wire_bytes};

    fn buffer_account(authority: Option<Pubkey>, program: &[u8]) -> Account {
        let mut data =
            wincode::serialize(&UpgradeableLoaderState::Buffer { authority_address: authority })
                .unwrap();
        data.resize(UpgradeableLoaderState::size_of_buffer_metadata(), 0);
        data.extend_from_slice(program);
        Account {
            lamports: 1,
            data,
            owner: bpf_loader_upgradeable::id(),
            executable: false,
            rent_epoch: 0,
        }
    }

    #[test]
    fn a_full_write_chunk_hits_the_loader_instruction_data_limit() {
        let buffer = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let chunk = max_write_chunk_size(&buffer, &authority).unwrap();
        // Write { offset: u32, bytes: Vec<u8> }: 4-byte tag, 4-byte offset, 8-byte length.
        assert_eq!(chunk, MAX_LOADER_INSTRUCTION_DATA - 16);
        let full = loader_instruction::write(&buffer, &authority, 0, vec![0; chunk]);
        assert_eq!(full.data.len(), MAX_LOADER_INSTRUCTION_DATA);
    }

    #[test]
    fn full_writes_pack_three_per_v1_transaction() {
        let buffer = Pubkey::new_unique();
        let fee_payer = Keypair::new();
        let separate_authority = Keypair::new();
        let config = transaction_config(10_000, 1_000_000, Some(5_000), None);

        for authority in [&fee_payer, &separate_authority] {
            let chunk = max_write_chunk_size(&buffer, &authority.pubkey()).unwrap();
            let program = vec![0xab; chunk * 10 + 7];
            let writes: Vec<Instruction> = plan_write_chunks(&program, None, chunk)
                .into_iter()
                .map(|(offset, bytes)| {
                    loader_instruction::write(&buffer, &authority.pubkey(), offset, bytes.to_vec())
                })
                .collect();
            let packed = pack_instructions(&fee_payer.pubkey(), writes.clone(), config).unwrap();

            let sizes: Vec<usize> = packed.iter().map(Vec::len).collect();
            assert_eq!(sizes, vec![3, 3, 3, 2], "11 writes, three per transaction");
            assert_eq!(packed.concat(), writes, "order and content are preserved");
            for instructions in &packed {
                let tx = build_v1_transaction(
                    &fee_payer.pubkey(),
                    instructions,
                    Hash::new_unique(),
                    config,
                    &[&fee_payer, authority],
                )
                .expect("each packed transaction fits");
                let wire = v1_wire_bytes(&tx).unwrap();
                assert!(wire.len() <= MAX_V1_TRANSACTION_SIZE);
                let parsed = sonar_sim::V1Transaction::parse(&wire).expect("cluster-valid v1");
                parsed.verify_signatures().unwrap();
            }
        }
    }

    #[test]
    fn packing_rejects_an_instruction_larger_than_a_transaction() {
        let fee_payer = Pubkey::new_unique();
        let huge = loader_instruction::write(
            &Pubkey::new_unique(),
            &fee_payer,
            0,
            vec![0; MAX_V1_TRANSACTION_SIZE],
        );
        let err = pack_instructions(&fee_payer, vec![huge], TransactionConfig::empty())
            .expect_err("cannot fit");
        assert!(err.to_string().contains("does not fit"), "{err}");
    }

    #[test]
    fn plan_covers_every_byte_and_skips_matching_chunks() {
        let program: Vec<u8> = (0..10u8).collect();
        let all = plan_write_chunks(&program, None, 4);
        assert_eq!(all, vec![(0, &program[0..4]), (4, &program[4..8]), (8, &program[8..10])]);

        let mut held = program.clone();
        held[5] = 0xff;
        let partial = plan_write_chunks(&program, Some(&held), 4);
        assert_eq!(partial, vec![(4, &program[4..8])]);

        // A shorter existing buffer cannot hold the tail chunk.
        let short = plan_write_chunks(&program, Some(&program[..9]), 4);
        assert_eq!(short, vec![(8, &program[8..10])]);
    }

    #[test]
    fn existing_buffer_must_match_authority_and_size() {
        let address = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let account = buffer_account(Some(authority), &[1, 2, 3]);

        assert_eq!(existing_program_bytes(address, &account, &authority, 3).unwrap(), &[1, 2, 3]);

        let err = existing_program_bytes(address, &account, &authority, 4).unwrap_err();
        assert!(err.to_string().contains("smaller than the minimum size"), "{err}");

        let err = existing_program_bytes(address, &account, &Pubkey::new_unique(), 3).unwrap_err();
        assert!(err.to_string().contains("Buffer authority mismatch"), "{err}");

        let immutable = buffer_account(None, &[1, 2, 3]);
        let err = existing_program_bytes(address, &immutable, &authority, 3).unwrap_err();
        assert!(err.to_string().contains("immutable"), "{err}");

        let mut foreign = account.clone();
        foreign.owner = Pubkey::new_unique();
        let err = existing_program_bytes(address, &foreign, &authority, 3).unwrap_err();
        assert!(err.to_string().contains("not owned by BPF Loader Upgradeable"), "{err}");
    }

    #[test]
    fn compute_unit_price_converts_to_a_rounded_up_total_fee() {
        assert_eq!(priority_fee_from_price(1, 10_000), 1);
        assert_eq!(priority_fee_from_price(100_000, 10_000), 1_000);
        assert_eq!(priority_fee_from_price(u64::MAX, u32::MAX), u64::MAX);

        let config = transaction_config(10_000, 7, None, Some(100_000));
        assert_eq!(config.priority_fee, Some(1_000));
        assert_eq!(config.compute_unit_limit, Some(10_000));
        assert_eq!(config.loaded_accounts_data_size_limit, Some(7));
        assert_eq!(transaction_config(1, 1, Some(0), None).priority_fee, None);
    }

    #[test]
    fn transaction_fee_saturates_instead_of_overflowing() {
        let payer = Keypair::new();
        let tx = UnsignedV1Transaction { instructions: Vec::new(), signers: vec![&payer, &payer] };
        let config = TransactionConfig::empty();
        assert_eq!(transaction_fee(&tx, config), LAMPORTS_PER_SIGNATURE, "one unique signer");
        let config = config.with_priority_fee(u64::MAX);
        assert_eq!(transaction_fee(&tx, config), u64::MAX);
    }

    #[test]
    fn loaded_accounts_limit_covers_the_buffer_and_caps_at_64_mib() {
        assert_eq!(loaded_accounts_data_size_limit(37), 37 + 1024 * 1024);
        assert_eq!(loaded_accounts_data_size_limit(usize::MAX / 2), 64 * 1024 * 1024);
    }

    #[test]
    fn non_elf_programs_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("program.so");
        std::fs::write(&path, b"not an elf").unwrap();
        let err = read_program(&path).unwrap_err();
        assert!(err.to_string().contains("is not an ELF file"), "{err}");
    }

    #[test]
    fn buffer_argument_accepts_an_address_or_a_keypair_file() {
        let address = Pubkey::new_unique();
        let BufferTarget::Address(parsed) =
            BufferTarget::resolve(Some(&address.to_string())).unwrap()
        else {
            panic!("an address resolves to an address");
        };
        assert_eq!(parsed, address);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("buffer.json");
        let keypair = Keypair::new();
        solana_keypair::write_keypair_file(&keypair, &path).unwrap();
        let resolved = BufferTarget::resolve(path.to_str()).unwrap();
        assert!(matches!(resolved, BufferTarget::Signer(_)));
        assert_eq!(resolved.pubkey(), keypair.pubkey());

        assert!(matches!(BufferTarget::resolve(None).unwrap(), BufferTarget::Signer(_)));
        let err = BufferTarget::resolve(Some("/nonexistent/buffer.json")).err().unwrap();
        assert!(err.to_string().contains("--buffer keypair file"), "{err}");
    }
}
