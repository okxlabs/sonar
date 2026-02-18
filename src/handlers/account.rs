use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result};
use serde_json::Value;
use solana_pubkey::Pubkey;

use crate::cli::AccountArgs;
use crate::{account_loader, metaplex_metadata_decoder, token_account_decoder};

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
        if args.mpl_metadata {
            if !should_enrich_with_metaplex_metadata(&account, &token_json) {
                anyhow::bail!("--mpl-metadata requires a SPL Token or Token-2022 mint account");
            }

            let metadata_result = fetch_metadata_for_mint(&client, &account_pubkey);
            let (output, warning) =
                resolve_metadata_output(&token_json, metadata_result, args.no_account_meta)?;
            if let Some(message) = warning {
                eprintln!("Warning: {message}");
            }
            println!("{}", serde_json::to_string_pretty(&output)?);
            return Ok(());
        }

        let output = token_output_value(&token_json, args.no_account_meta);
        println!("{}", serde_json::to_string_pretty(&output)?);
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

/// Token mint JSON shape check:
/// - owner is SPL Token legacy or Token-2022
/// - parsed token data contains mint fields (decimals/supply)
fn should_enrich_with_metaplex_metadata(
    account: &solana_account::Account,
    token_json: &Value,
) -> bool {
    let owner_bytes = account.owner.to_bytes();
    if owner_bytes != spl_token::ID.to_bytes() && owner_bytes != spl_token_2022::ID.to_bytes() {
        return false;
    }

    token_json
        .get("data")
        .and_then(Value::as_object)
        .map(|data| data.contains_key("decimals") && data.contains_key("supply"))
        .unwrap_or(false)
}

fn token_output_value(token_json: &Value, no_account_meta: bool) -> Value {
    if no_account_meta {
        token_json.get("data").cloned().unwrap_or_else(|| token_json.clone())
    } else {
        token_json.clone()
    }
}

fn resolve_metadata_output(
    token_json: &Value,
    metadata_result: Result<Value>,
    no_account_meta: bool,
) -> Result<(Value, Option<String>)> {
    match metadata_result {
        Ok(metadata_json) => Ok((metadata_json, None)),
        Err(error) => {
            let fallback = token_output_value(token_json, no_account_meta);
            let warning = format!(
                "--mpl-metadata enrichment failed ({error}). Falling back to parsed mint account data."
            );
            Ok((fallback, Some(warning)))
        }
    }
}

fn fetch_metadata_for_mint(
    client: &solana_client::rpc_client::RpcClient,
    mint_pubkey: &Pubkey,
) -> Result<Value> {
    use solana_commitment_config::CommitmentConfig;

    let metadata_pda = metaplex_metadata_decoder::derive_metadata_pda(mint_pubkey);
    let response = client
        .get_account_with_commitment(&metadata_pda, CommitmentConfig::processed())
        .with_context(|| {
            format!("Failed to fetch metadata PDA {} for mint {}", metadata_pda, mint_pubkey)
        })?;

    let metadata_account = response.value.with_context(|| {
        format!("Metadata PDA account not found for mint {} (PDA: {})", mint_pubkey, metadata_pda)
    })?;

    if metadata_account.owner != metaplex_metadata_decoder::metadata_program_id() {
        anyhow::bail!(
            "Metadata PDA {} owner mismatch, expected {}, got {}",
            metadata_pda,
            metaplex_metadata_decoder::metadata_program_id(),
            metadata_account.owner
        );
    }

    metaplex_metadata_decoder::decode_metadata_account_data(&metadata_account.data).with_context(
        || {
            format!(
                "Failed to decode metaplex metadata account {} for mint {}",
                metadata_pda, mint_pubkey
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{resolve_metadata_output, should_enrich_with_metaplex_metadata};
    use crate::token_account_decoder;
    use anyhow::anyhow;
    use serde_json::json;
    use solana_pubkey::Pubkey;
    use spl_token::solana_program::program_option::COption;
    use spl_token::solana_program::program_pack::Pack;
    use spl_token::solana_program::pubkey::Pubkey as ProgramPubkey;

    fn legacy_owner_pubkey() -> Pubkey {
        Pubkey::new_from_array(spl_token::ID.to_bytes())
    }

    fn token2022_owner_pubkey() -> Pubkey {
        Pubkey::new_from_array(spl_token_2022::ID.to_bytes())
    }

    #[test]
    fn should_enrich_metadata_for_legacy_mint() {
        use spl_token::state::Mint;

        let mint = Mint {
            mint_authority: COption::Some(ProgramPubkey::new_unique()),
            supply: 1_000_000,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        let mut data = vec![0u8; Mint::LEN];
        Mint::pack(mint, &mut data).unwrap();

        let account = solana_account::Account {
            lamports: 1,
            data,
            owner: legacy_owner_pubkey(),
            executable: false,
            rent_epoch: 0,
        };
        let token_json = token_account_decoder::decode_spl_token_account(&account).unwrap();
        assert!(should_enrich_with_metaplex_metadata(&account, &token_json));
    }

    #[test]
    fn should_enrich_metadata_for_token2022_mint() {
        use spl_token_2022::state::Mint;

        let mint = Mint {
            mint_authority: COption::Some(ProgramPubkey::new_unique()),
            supply: 2_000_000,
            decimals: 9,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        let mut data = vec![0u8; Mint::LEN];
        Mint::pack(mint, &mut data).unwrap();

        let account = solana_account::Account {
            lamports: 1,
            data,
            owner: token2022_owner_pubkey(),
            executable: false,
            rent_epoch: 0,
        };
        let token_json = token_account_decoder::decode_spl_token_account(&account).unwrap();
        assert!(should_enrich_with_metaplex_metadata(&account, &token_json));
    }

    #[test]
    fn should_not_enrich_metadata_for_token_account() {
        use spl_token::state::{Account as TokenAccount, AccountState};

        let token_account = TokenAccount {
            mint: ProgramPubkey::new_unique(),
            owner: ProgramPubkey::new_unique(),
            amount: 123,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        };
        let mut data = vec![0u8; TokenAccount::LEN];
        TokenAccount::pack(token_account, &mut data).unwrap();

        let account = solana_account::Account {
            lamports: 1,
            data,
            owner: legacy_owner_pubkey(),
            executable: false,
            rent_epoch: 0,
        };
        let token_json = token_account_decoder::decode_spl_token_account(&account).unwrap();
        assert!(!should_enrich_with_metaplex_metadata(&account, &token_json));
    }

    #[test]
    fn metadata_missing_is_non_fatal_without_strict_mode() {
        let token_json = json!({
            "lamports": 123,
            "space": 82,
            "owner": spl_token::ID.to_string(),
            "executable": false,
            "rentEpoch": 0,
            "data": {
                "mintAuthority": null,
                "supply": "1000",
                "decimals": 6,
                "isInitialized": true,
                "freezeAuthority": null
            }
        });

        let (output, warning) = resolve_metadata_output(
            &token_json,
            Err(anyhow!("Metadata PDA account not found for mint ...")),
            false,
        )
        .expect("metadata failure should fallback");

        assert_eq!(output, token_json);
        assert!(warning.is_some());
    }

    #[test]
    fn metadata_decode_failure_falls_back_to_data_with_no_account_meta() {
        let token_json = json!({
            "lamports": 123,
            "space": 82,
            "owner": spl_token::ID.to_string(),
            "executable": false,
            "rentEpoch": 0,
            "data": {
                "mintAuthority": null,
                "supply": "1000",
                "decimals": 6,
                "isInitialized": true,
                "freezeAuthority": null
            }
        });

        let (output, warning) = resolve_metadata_output(
            &token_json,
            Err(anyhow!("Failed to decode metaplex metadata account ...")),
            true,
        )
        .expect("metadata decode failure should fallback");

        assert_eq!(output, token_json["data"]);
        assert!(warning.is_some());
    }

    #[test]
    fn metadata_failure_still_falls_back() {
        let token_json = json!({
            "lamports": 123,
            "space": 82,
            "owner": spl_token::ID.to_string(),
            "executable": false,
            "rentEpoch": 0,
            "data": {
                "mintAuthority": null,
                "supply": "1000",
                "decimals": 6,
                "isInitialized": true,
                "freezeAuthority": null
            }
        });

        let (output, warning) = resolve_metadata_output(
            &token_json,
            Err(anyhow!("Metadata PDA account not found for mint ...")),
            false,
        )
        .expect("metadata failure should always fallback");

        assert_eq!(output, token_json);
        assert!(warning.is_some());
    }
}
