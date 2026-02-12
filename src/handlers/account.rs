use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result};
use solana_pubkey::Pubkey;

use crate::cli::AccountArgs;
use crate::{account_loader, token_account_decoder};

pub(crate) fn handle(args: AccountArgs) -> Result<()> {
    use crate::instruction_parsers::anchor_idl::{IdlRegistry, RawAnchorIdl, parse_account_data};
    use solana_client::rpc_client::RpcClient;
    use solana_loader_v3_interface::state::UpgradeableLoaderState;
    use solana_sdk_ids::bpf_loader_upgradeable;

    // Parse the account pubkey
    let account_pubkey = Pubkey::from_str(&args.account)
        .with_context(|| format!("Invalid account pubkey: {}", args.account))?;

    // Create RPC client and fetch the account
    let client = RpcClient::new(&args.rpc.rpc_url);
    let account = client
        .get_account(&account_pubkey)
        .with_context(|| format!("Failed to fetch account: {}", account_pubkey))?;

    let owner = account.owner;

    // If --raw is specified, just print raw data and return
    if args.raw {
        print_raw_account_data(&account);
        return Ok(());
    }

    // Detect BPF Loader Upgradeable accounts (Program, ProgramData, Buffer)
    if account.owner == bpf_loader_upgradeable::id() {
        if let Ok(state) = bincode::deserialize::<UpgradeableLoaderState>(account.data.as_slice()) {
            match state {
                UpgradeableLoaderState::Program { programdata_address } => {
                    let programdata_pubkey = Pubkey::new_from_array(programdata_address.to_bytes());

                    // Build Program account JSON
                    let program_data_json = serde_json::json!({
                        "programdataAddress": programdata_pubkey.to_string()
                    });

                    if args.no_account_meta {
                        println!("{}", serde_json::to_string_pretty(&program_data_json)?);
                    } else {
                        let output = serde_json::json!({
                            "lamports": account.lamports,
                            "space": account.data.len(),
                            "owner": account.owner.to_string(),
                            "executable": account.executable,
                            "rentEpoch": account.rent_epoch,
                            "data": program_data_json
                        });
                        println!("{}", serde_json::to_string_pretty(&output)?);
                    }

                    return Ok(());
                }
                UpgradeableLoaderState::ProgramData { .. } => {
                    print_programdata_json(&account, args.no_account_meta)?;
                    return Ok(());
                }
                UpgradeableLoaderState::Buffer { authority_address, .. } => {
                    const BUFFER_HEADER_SIZE: usize = 37;
                    let data_size = account.data.len().saturating_sub(BUFFER_HEADER_SIZE);

                    let buffer_data_json = serde_json::json!({
                        "authority": authority_address
                            .map(|a| Pubkey::new_from_array(a.to_bytes()).to_string()),
                        "dataSize": data_size
                    });

                    if args.no_account_meta {
                        println!("{}", serde_json::to_string_pretty(&buffer_data_json)?);
                    } else {
                        let output = serde_json::json!({
                            "lamports": account.lamports,
                            "space": account.data.len(),
                            "owner": account.owner.to_string(),
                            "executable": account.executable,
                            "rentEpoch": account.rent_epoch,
                            "data": buffer_data_json
                        });
                        println!("{}", serde_json::to_string_pretty(&output)?);
                    }

                    return Ok(());
                }
                _ => {} // Uninitialized, fall through to other decoders
            }
        }
    }

    // Try to decode as SPL Token or Token-2022 account (mint or token account)
    if let Some(token_json) = token_account_decoder::decode_spl_token_account(&account) {
        if args.no_account_meta {
            // Only print the parsed data part
            if let Some(data) = token_json.get("data") {
                println!("{}", serde_json::to_string_pretty(data)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&token_json)?);
            }
        } else {
            // Print the complete token_json (already contains account metadata and data)
            println!("{}", serde_json::to_string_pretty(&token_json)?);
        }
        return Ok(());
    }

    // Try to find IDL: first from local directory, then from chain
    let idl_json = try_load_idl_from_dir(&args.idl_dir, &owner).or_else(|| {
        let loader =
            account_loader::AccountLoader::new(args.rpc.rpc_url.clone(), None, false, None).ok()?;
        loader.fetch_idl(&owner).ok().flatten()
    });

    let idl_json = match idl_json {
        Some(json) => json,
        None => {
            // Output raw data in Solana JSON RPC format
            print_raw_account_data(&account);
            return Ok(());
        }
    };

    // Parse the IDL
    let raw_idl: RawAnchorIdl =
        serde_json::from_str(&idl_json).with_context(|| "Failed to parse IDL JSON")?;
    let idl = raw_idl.convert(&owner.to_string());

    // Create an empty registry (we only have one IDL)
    let registry = IdlRegistry::new();

    // Parse the account data
    match parse_account_data(&idl, &account.data, &registry)? {
        Some((_type_name, parsed_value)) => {
            if args.no_account_meta {
                println!("{}", serde_json::to_string_pretty(&parsed_value)?);
            } else {
                let output = serde_json::json!({
                    "lamports": account.lamports,
                    "space": account.data.len(),
                    "owner": account.owner.to_string(),
                    "executable": account.executable,
                    "rentEpoch": account.rent_epoch,
                    "data": parsed_value
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
        }
        None => {
            let output = if args.no_account_meta {
                if account.data.len() >= 8 {
                    serde_json::json!({
                        "error": "No matching account type found",
                        "discriminator": hex::encode(&account.data[..8]),
                        "raw_data": hex::encode(&account.data)
                    })
                } else {
                    serde_json::json!({
                        "error": "Account data too short",
                        "raw_data": hex::encode(&account.data)
                    })
                }
            } else if account.data.len() >= 8 {
                serde_json::json!({
                    "lamports": account.lamports,
                    "space": account.data.len(),
                    "owner": account.owner.to_string(),
                    "executable": account.executable,
                    "rentEpoch": account.rent_epoch,
                    "error": "No matching account type found",
                    "discriminator": hex::encode(&account.data[..8]),
                    "raw_data": hex::encode(&account.data)
                })
            } else {
                serde_json::json!({
                    "lamports": account.lamports,
                    "space": account.data.len(),
                    "owner": account.owner.to_string(),
                    "executable": account.executable,
                    "rentEpoch": account.rent_epoch,
                    "error": "Account data too short",
                    "raw_data": hex::encode(&account.data)
                })
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }

    Ok(())
}

/// Try to load IDL from local directory (if specified).
/// IDL files are expected to be named `<PROGRAM_ID>.json`.
fn try_load_idl_from_dir(idl_dir: &Option<PathBuf>, owner: &Pubkey) -> Option<String> {
    let path = idl_dir.as_ref()?;
    let idl_file = path.join(format!("{}.json", owner));

    if idl_file.exists() {
        match fs::read_to_string(&idl_file) {
            Ok(content) => {
                log::debug!("Loaded IDL from {}", idl_file.display());
                Some(content)
            }
            Err(e) => {
                log::warn!("Failed to read IDL file {}: {}", idl_file.display(), e);
                None
            }
        }
    } else {
        log::debug!("IDL file not found: {}", idl_file.display());
        None
    }
}

/// Print ProgramData account info as JSON.
/// Deserializes the UpgradeableLoaderState::ProgramData to extract upgrade authority and slot.
fn print_programdata_json(account: &solana_account::Account, no_account_meta: bool) -> Result<()> {
    use solana_loader_v3_interface::state::UpgradeableLoaderState;

    const PROGRAM_DATA_HEADER_SIZE: usize = 45;

    let state: UpgradeableLoaderState = bincode::deserialize(account.data.as_slice())
        .with_context(|| "Failed to deserialize ProgramData account")?;

    if let UpgradeableLoaderState::ProgramData { slot, upgrade_authority_address } = state {
        let authority =
            upgrade_authority_address.map(|a| Pubkey::new_from_array(a.to_bytes()).to_string());
        let elf_size = account.data.len().saturating_sub(PROGRAM_DATA_HEADER_SIZE);

        let data_json = serde_json::json!({
            "upgradeAuthority": authority,
            "lastDeployedSlot": slot,
            "elfSize": elf_size
        });

        if no_account_meta {
            println!("{}", serde_json::to_string_pretty(&data_json)?);
        } else {
            let output = serde_json::json!({
                "lamports": account.lamports,
                "space": account.data.len(),
                "owner": account.owner.to_string(),
                "executable": account.executable,
                "rentEpoch": account.rent_epoch,
                "data": data_json
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    } else {
        anyhow::bail!("Account is not a ProgramData account");
    }

    Ok(())
}

/// Print account data in Solana JSON RPC format.
/// Field order follows Solana Account struct: lamports, data, owner, executable, rent_epoch
fn print_raw_account_data(account: &solana_account::Account) {
    use base64::{Engine as _, engine::general_purpose};
    let data_b64 = general_purpose::STANDARD.encode(&account.data);
    let output = serde_json::json!({
        "lamports": account.lamports,
        "data": data_b64,
        "owner": account.owner.to_string(),
        "executable": account.executable,
        "rentEpoch": account.rent_epoch,
        "space": account.data.len()
    });
    println!("{}", serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()));
}
