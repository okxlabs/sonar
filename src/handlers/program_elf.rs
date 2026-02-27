use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use sha2::{Digest, Sha256};
use solana_client::rpc_client::RpcClient;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use solana_sdk_ids::bpf_loader_upgradeable;
use std::io::Write;

use crate::cli::ProgramDataArgs;

const PROGRAM_DATA_HEADER_SIZE: usize = 45;
const BUFFER_HEADER_SIZE: usize = 37;

enum UpgradeableAccountKind {
    Program { programdata_address: Pubkey },
    ProgramData,
    Buffer,
}

pub(crate) fn handle(args: ProgramDataArgs) -> Result<()> {
    let address = Pubkey::from_str(&args.address)
        .with_context(|| format!("Invalid address: {}", args.address))?;

    let client = RpcClient::new(&args.rpc.rpc_url);
    let input_account = client
        .get_account(&address)
        .with_context(|| format!("Failed to fetch account: {address}"))?;
    ensure_upgradeable_owner(address, &input_account.owner)?;

    let elf_data = match classify_upgradeable_account(address, &input_account.data)? {
        UpgradeableAccountKind::Program { programdata_address } => {
            let programdata_account =
                client.get_account(&programdata_address).with_context(|| {
                    format!("Failed to fetch program data account: {programdata_address}")
                })?;
            ensure_upgradeable_owner(programdata_address, &programdata_account.owner)?;

            match classify_upgradeable_account(programdata_address, &programdata_account.data)? {
                UpgradeableAccountKind::ProgramData => {
                    extract_elf_from_program_data(programdata_address, &programdata_account.data)?
                }
                _ => {
                    return Err(anyhow!(
                        "Program account {address} points to {programdata_address}, but that account is not ProgramData"
                    ));
                }
            }
        }
        UpgradeableAccountKind::ProgramData => {
            extract_elf_from_program_data(address, &input_account.data)?
        }
        UpgradeableAccountKind::Buffer => extract_elf_from_buffer(address, &input_account.data)?,
    };

    // Handle verification or output
    if let Some(expected_hash) = args.verify_sha256 {
        // Compute SHA256 hash
        let mut hasher = Sha256::new();
        hasher.update(&elf_data);
        let actual_hash = hex::encode(hasher.finalize());

        // Normalize expected hash (remove 0x prefix if present, lowercase)
        let expected_hash =
            expected_hash.strip_prefix("0x").unwrap_or(&expected_hash).to_lowercase();

        if actual_hash == expected_hash {
            println!("true");
            Ok(())
        } else {
            println!("false");
            Err(anyhow!("SHA256 mismatch: expected {}, got {}", expected_hash, actual_hash))
        }
    } else if let Some(output_path) = args.output {
        if is_stdout_output_path(&output_path) {
            write_raw_bytes_to_stdout(&elf_data)
        } else {
            std::fs::write(&output_path, &elf_data).with_context(|| {
                format!("Failed to write program data to {}", output_path.display())
            })?;
            println!("Wrote {} bytes to {}", elf_data.len(), output_path.display());
            Ok(())
        }
    } else {
        unreachable!("clap requires --output when --verify-sha256 is absent");
    }
}

fn ensure_upgradeable_owner(address: Pubkey, owner: &Pubkey) -> Result<()> {
    if *owner != bpf_loader_upgradeable::id() {
        return Err(anyhow!(
            "Account {} is not owned by BPF Loader Upgradeable (owner: {})",
            address,
            owner
        ));
    }

    Ok(())
}

fn classify_upgradeable_account(
    address: Pubkey,
    account_data: &[u8],
) -> Result<UpgradeableAccountKind> {
    let state: UpgradeableLoaderState = bincode::deserialize(account_data).with_context(|| {
        format!("Failed to deserialize upgradeable loader account state: {address}")
    })?;

    match state {
        UpgradeableLoaderState::Program { programdata_address } => {
            Ok(UpgradeableAccountKind::Program {
                programdata_address: Pubkey::new_from_array(programdata_address.to_bytes()),
            })
        }
        UpgradeableLoaderState::ProgramData { .. } => Ok(UpgradeableAccountKind::ProgramData),
        UpgradeableLoaderState::Buffer { .. } => Ok(UpgradeableAccountKind::Buffer),
        _ => Err(anyhow!("Account {} is not a Program, ProgramData, or Buffer account", address)),
    }
}

fn extract_elf_from_program_data(address: Pubkey, data: &[u8]) -> Result<Vec<u8>> {
    if data.len() <= PROGRAM_DATA_HEADER_SIZE {
        return Err(anyhow!("ProgramData account {} is too short: {} bytes", address, data.len()));
    }

    Ok(data[PROGRAM_DATA_HEADER_SIZE..].to_vec())
}

fn extract_elf_from_buffer(address: Pubkey, data: &[u8]) -> Result<Vec<u8>> {
    if data.len() <= BUFFER_HEADER_SIZE {
        return Err(anyhow!("Buffer account {} is too short: {} bytes", address, data.len()));
    }

    Ok(data[BUFFER_HEADER_SIZE..].to_vec())
}

fn is_stdout_output_path(path: &std::path::Path) -> bool {
    path.as_os_str() == "-"
}

fn write_raw_bytes_to_stdout(data: &[u8]) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(data).with_context(|| "Failed to write program data to stdout")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use solana_loader_v3_interface::state::UpgradeableLoaderState;
    use solana_pubkey::Pubkey;

    use super::{classify_upgradeable_account, is_stdout_output_path};

    #[test]
    fn output_dash_path_maps_to_stdout() {
        assert!(is_stdout_output_path(Path::new("-")));
        assert!(!is_stdout_output_path(Path::new("program.so")));
    }

    #[test]
    fn classify_programdata_account_state() {
        let address = Pubkey::new_unique();
        let state =
            UpgradeableLoaderState::ProgramData { slot: 1, upgrade_authority_address: None };
        let serialized = bincode::serialize(&state).expect("serialize ProgramData state");

        let kind =
            classify_upgradeable_account(address, &serialized).expect("classify ProgramData");
        assert!(matches!(kind, super::UpgradeableAccountKind::ProgramData));
    }

    #[test]
    fn classify_buffer_account_state() {
        let address = Pubkey::new_unique();
        let state = UpgradeableLoaderState::Buffer { authority_address: None };
        let serialized = bincode::serialize(&state).expect("serialize Buffer state");

        let kind = classify_upgradeable_account(address, &serialized).expect("classify Buffer");
        assert!(matches!(kind, super::UpgradeableAccountKind::Buffer));
    }
}
