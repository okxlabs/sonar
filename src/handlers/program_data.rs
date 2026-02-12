use std::str::FromStr;

use anyhow::{Context, Result};
use solana_pubkey::Pubkey;

use crate::cli::ProgramDataArgs;

pub(crate) fn handle(args: ProgramDataArgs) -> Result<()> {
    use sha2::{Digest, Sha256};
    use solana_client::rpc_client::RpcClient;
    use solana_loader_v3_interface::state::UpgradeableLoaderState;
    use solana_sdk_ids::bpf_loader_upgradeable;
    use std::io::Write;

    // Header sizes for different account types
    const PROGRAM_DATA_HEADER_SIZE: usize = 45;
    const BUFFER_HEADER_SIZE: usize = 37;

    let address = Pubkey::from_str(&args.address)
        .with_context(|| format!("Invalid address: {}", args.address))?;

    let client = RpcClient::new(&args.rpc.rpc_url);

    let elf_data = if args.buffer {
        // Buffer mode: fetch buffer account directly
        let account = client
            .get_account(&address)
            .with_context(|| format!("Failed to fetch buffer account: {}", address))?;

        if account.owner != bpf_loader_upgradeable::id() {
            return Err(anyhow::anyhow!(
                "Account {} is not owned by BPF Loader Upgradeable (owner: {})",
                address,
                account.owner
            ));
        }

        // Verify it's a Buffer account
        let state: UpgradeableLoaderState = bincode::deserialize(&account.data)
            .with_context(|| "Failed to deserialize buffer account state")?;

        match state {
            UpgradeableLoaderState::Buffer { .. } => {}
            _ => {
                return Err(anyhow::anyhow!("Account {} is not a Buffer account", address));
            }
        }

        if account.data.len() <= BUFFER_HEADER_SIZE {
            return Err(anyhow::anyhow!(
                "Buffer account data too short: {} bytes",
                account.data.len()
            ));
        }

        account.data[BUFFER_HEADER_SIZE..].to_vec()
    } else {
        // Program mode: fetch program account and then program data account
        let account = client
            .get_account(&address)
            .with_context(|| format!("Failed to fetch program account: {}", address))?;

        if account.owner != bpf_loader_upgradeable::id() {
            return Err(anyhow::anyhow!(
                "Account {} is not owned by BPF Loader Upgradeable (owner: {})",
                address,
                account.owner
            ));
        }

        // Parse program account to get programdata address
        let state: UpgradeableLoaderState = bincode::deserialize(&account.data)
            .with_context(|| "Failed to deserialize program account state")?;

        let programdata_address = match state {
            UpgradeableLoaderState::Program { programdata_address } => {
                Pubkey::new_from_array(programdata_address.to_bytes())
            }
            _ => {
                return Err(anyhow::anyhow!("Account {} is not a Program account", address));
            }
        };

        // Fetch program data account
        let programdata_account = client.get_account(&programdata_address).with_context(|| {
            format!("Failed to fetch program data account: {}", programdata_address)
        })?;

        if programdata_account.data.len() <= PROGRAM_DATA_HEADER_SIZE {
            return Err(anyhow::anyhow!(
                "Program data account too short: {} bytes",
                programdata_account.data.len()
            ));
        }

        programdata_account.data[PROGRAM_DATA_HEADER_SIZE..].to_vec()
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
            std::process::exit(1);
        }
    } else if let Some(output_path) = args.output {
        // Write to specified file
        std::fs::write(&output_path, &elf_data).with_context(|| {
            format!("Failed to write program data to {}", output_path.display())
        })?;
        eprintln!("Wrote {} bytes to {}", elf_data.len(), output_path.display());
        Ok(())
    } else {
        // Output raw binary data to stdout
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(&elf_data).with_context(|| "Failed to write program data to stdout")?;
        Ok(())
    }
}
