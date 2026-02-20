use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result};
use colored::Colorize;
use serde_json::Value;
use solana_pubkey::Pubkey;

use crate::cli::AccountArgs;
use crate::{
    core::account_loader, output::terminal::render_section_title,
    parsers::metaplex_metadata_decoder, parsers::token_account_decoder,
};

struct AccountOutput {
    kind: String,
    data: Value,
    json_output: Value,
}


pub(crate) fn handle(args: AccountArgs) -> Result<()> {
    // Parse the account pubkey
    let account_pubkey = Pubkey::from_str(&args.account)
        .with_context(|| format!("Invalid account pubkey: {}", args.account))?;

    // Create RPC client and fetch the account
    use solana_client::rpc_client::RpcClient;
    let client = RpcClient::new(&args.rpc.rpc_url);
    let account = client
        .get_account(&account_pubkey)
        .with_context(|| format!("Failed to fetch account: {}", account_pubkey))?;

    // If --raw is specified, just print raw data and return
    if args.raw {
        print_raw_account_data(&account);
        return Ok(());
    }

    let output = decode_account_output(&args, &client, &account_pubkey, &account)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output.json_output)?);
    } else {
        render_account_text(&account_pubkey, &account, &output.kind, &output.data);
    }

    Ok(())
}

fn decode_account_output(
    args: &AccountArgs,
    client: &solana_client::rpc_client::RpcClient,
    account_pubkey: &Pubkey,
    account: &solana_account::Account,
) -> Result<AccountOutput> {
    use crate::parsers::instruction::anchor_idl::{IdlRegistry, RawAnchorIdl, parse_account_data};
    use solana_address_lookup_table_interface::state::AddressLookupTable;
    use solana_loader_v3_interface::state::UpgradeableLoaderState;
    use solana_sdk_ids::{address_lookup_table, bpf_loader_upgradeable};

    // Detect BPF Loader Upgradeable accounts (Program, ProgramData, Buffer)
    if account.owner == bpf_loader_upgradeable::id() {
        if let Ok(state) = bincode::deserialize::<UpgradeableLoaderState>(account.data.as_slice()) {
            match state {
                UpgradeableLoaderState::Program { programdata_address } => {
                    let programdata_pubkey = Pubkey::new_from_array(programdata_address.to_bytes());
                    let data_json = serde_json::json!({
                        "programdataAddress": programdata_pubkey.to_string()
                    });
                    return Ok(AccountOutput {
                        kind: "Upgradeable Program".to_string(),
                        data: data_json.clone(),
                        json_output: wrap_account_data_output(account, data_json),
                    });
                }
                UpgradeableLoaderState::ProgramData { .. } => {
                    let data_json = build_programdata_json(account)?;
                    return Ok(AccountOutput {
                        kind: "ProgramData".to_string(),
                        data: data_json.clone(),
                        json_output: wrap_account_data_output(account, data_json),
                    });
                }
                UpgradeableLoaderState::Buffer { authority_address, .. } => {
                    const BUFFER_HEADER_SIZE: usize = 37;
                    let data_size = account.data.len().saturating_sub(BUFFER_HEADER_SIZE);
                    let data_json = serde_json::json!({
                        "authority": authority_address
                            .map(|a| Pubkey::new_from_array(a.to_bytes()).to_string()),
                        "dataSize": data_size
                    });
                    return Ok(AccountOutput {
                        kind: "Upgradeable Buffer".to_string(),
                        data: data_json.clone(),
                        json_output: wrap_account_data_output(account, data_json),
                    });
                }
                _ => {} // Uninitialized, fall through to other decoders
            }
        }
    }

    // Detect Address Lookup Table
    if account.owner == address_lookup_table::id() {
        if let Ok(lookup_table) = AddressLookupTable::deserialize(account.data.as_slice()) {
            let authority = lookup_table.meta.authority.map(|a| a.to_string());
            let data_json = serde_json::json!({
                "meta": {
                    "deactivation_slot": lookup_table.meta.deactivation_slot,
                    "last_extended_slot": lookup_table.meta.last_extended_slot,
                    "last_extended_slot_start_index": lookup_table.meta.last_extended_slot_start_index,
                    "authority": authority,
                    "_padding": lookup_table.meta._padding,
                },
                "addresses": lookup_table.addresses.iter().map(|k| k.to_string()).collect::<Vec<_>>()
            });
            return Ok(AccountOutput {
                kind: "Address Lookup Table".to_string(),
                data: data_json.clone(),
                json_output: wrap_account_data_output(account, data_json),
            });
        }
    }

    // Try to decode as SPL Token or Token-2022 account (mint or token account)
    if let Some(token_json) = token_account_decoder::decode_spl_token_account(account) {
        if args.mpl_metadata {
            if !should_enrich_with_metaplex_metadata(account, &token_json) {
                anyhow::bail!("--mpl-metadata requires a SPL Token or Token-2022 mint account");
            }

            let metadata_result = fetch_metadata_for_mint(client, account_pubkey);
            let (output, warning) = resolve_metadata_output(&token_json, metadata_result)?;
            if let Some(message) = warning {
                eprintln!("Warning: {message}");
            }

            let kind = if has_account_meta_fields(&output) {
                infer_token_data_kind(&output)
            } else {
                "Metaplex Metadata".to_string()
            };

            return Ok(AccountOutput {
                kind,
                data: extract_data_for_text(&output),
                json_output: output,
            });
        }

        return Ok(AccountOutput {
            kind: infer_token_data_kind(&token_json),
            data: extract_data_for_text(&token_json),
            json_output: token_json,
        });
    }

    // Try to find IDL: first from local directory, then from chain
    let owner = account.owner;
    let idl_json = try_load_idl_from_dir(&args.idl_dir, &owner).or_else(|| {
        let loader =
            account_loader::AccountLoader::new(args.rpc.rpc_url.clone(), None, false, None).ok()?;
        loader.fetch_idl(&owner).ok().flatten()
    });

    let idl_json = match idl_json {
        Some(json) => json,
        None => {
            // Keep old JSON shape for --json, and show concise data in text mode.
            let raw_json = raw_account_data_json(account);
            let data_json = serde_json::json!({
                "encoding": "base64",
                "data": raw_json.get("data").cloned().unwrap_or(Value::Null)
            });
            return Ok(AccountOutput {
                kind: "Raw Account Data".to_string(),
                data: data_json,
                json_output: raw_json,
            });
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
        Some((type_name, parsed_value)) => {
            let parsed_json = serde_json::to_value(&parsed_value)
                .with_context(|| "Failed to convert parsed value to JSON")?;
            Ok(AccountOutput {
                kind: type_name.to_string(),
                data: parsed_json,
                json_output: wrap_account_data_output(account, &parsed_value),
            })
        }
        None => {
            let data_json = if account.data.len() >= 8 {
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
            };
            let json_output = if account.data.len() >= 8 {
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
            Ok(AccountOutput {
                kind: "Unknown / Unparsed".to_string(),
                data: data_json,
                json_output,
            })
        }
    }
}

fn render_account_text(
    account_pubkey: &Pubkey,
    account: &solana_account::Account,
    data_kind: &str,
    data: &Value,
) {
    render_section_title("Account Summary");
    let balance_sol = account.lamports as f64 / 1_000_000_000.0;
    let balance_text = format!(
        "{} {}",
        format!("{balance_sol:.9} SOL"),
        format!("({})", format_with_commas(account.lamports)).dimmed()
    );
    print_summary_line("Pubkey", account_pubkey.to_string().cyan().to_string());
    print_summary_line("Balance", balance_text);
    print_summary_line("Owner", account.owner.to_string().cyan().to_string());
    print_summary_line("Executable", style_bool(account.executable));
    print_summary_line(
        "Space",
        format!(
            "{} {}",
            account.data.len(),
            "bytes".dimmed()
        ),
    );
    print_summary_line(
        "Rent Epoch",
        account.rent_epoch.to_string(),
    );

    render_section_title(&format!("Account Data ({data_kind})"));
    render_json_as_yaml(data, 1);
    println!();
}

fn print_summary_line(label: &str, value: String) {
    const LABEL_WIDTH: usize = 12;
    println!(
        " {:<width$} {}",
        format!("{label}:").dimmed(),
        value,
        width = LABEL_WIDTH
    );
}

fn render_json_as_yaml(value: &Value, indent: usize) {
    let indent_str = " ".repeat(indent);
    match value {
        Value::Object(map) => {
            if map.is_empty() {
                println!("{indent_str}{}", "{}".dimmed());
                return;
            }

            for (key, child) in map {
                if is_scalar(child) {
                    println!(
                        "{indent_str}{} {}",
                        format!("{key}:").dimmed(),
                        format_scalar(Some(key.as_str()), child)
                    );
                } else {
                    println!("{indent_str}{}", format!("{key}:").dimmed());
                    render_json_as_yaml(child, indent + 2);
                }
            }
        }
        Value::Array(items) => {
            if items.is_empty() {
                println!("{indent_str}{}", "[]".dimmed());
                return;
            }

            for item in items {
                if is_scalar(item) {
                    println!(
                        "{indent_str}{} {}",
                        "-".dimmed(),
                        format_scalar(None, item)
                    );
                } else {
                    println!("{indent_str}{}", "-".dimmed());
                    render_json_as_yaml(item, indent + 2);
                }
            }
        }
        _ => println!("{indent_str}{}", format_scalar(None, value)),
    }
}

fn is_scalar(value: &Value) -> bool {
    matches!(value, Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_))
}

fn format_scalar(key: Option<&str>, value: &Value) -> String {
    match value {
        Value::Null => "null".dimmed().to_string(),
        Value::Bool(v) => style_bool(*v),
        Value::Number(v) => v.to_string(),
        Value::String(v) => {
            let rendered = truncate_for_display(v, 120);
            if key.is_some_and(|k| k.eq_ignore_ascii_case("error")) {
                return rendered.red().to_string();
            }
            if key.is_some_and(looks_like_address_key) || looks_like_base58_pubkey(v) {
                return rendered.cyan().to_string();
            }

            if rendered.is_empty() {
                "\"\"".dimmed().to_string()
            } else {
                rendered.to_string()
            }
        }
        _ => value.to_string(),
    }
}

fn style_bool(value: bool) -> String {
    if value {
        "true".green().to_string()
    } else {
        "false".red().to_string()
    }
}

fn looks_like_address_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower == "pubkey"
        || lower.contains("owner")
        || lower.contains("mint")
        || lower.contains("authority")
        || lower.contains("address")
        || lower.contains("programid")
}

fn looks_like_base58_pubkey(value: &str) -> bool {
    const BASE58_ALPHABET: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let len = value.len();
    (32..=44).contains(&len) && value.chars().all(|c| BASE58_ALPHABET.contains(c))
}

fn truncate_for_display(input: &str, max_chars: usize) -> String {
    let input_len = input.chars().count();
    if input_len <= max_chars {
        return input.to_string();
    }

    let prefix: String = input.chars().take(max_chars).collect();
    format!("{prefix}… ({input_len} chars)")
}

fn infer_token_data_kind(token_json: &Value) -> String {
    let data = token_json.get("data");
    let owner_bytes = token_json
        .get("owner")
        .and_then(Value::as_str)
        .and_then(|owner| Pubkey::from_str(owner).ok())
        .map(|owner| owner.to_bytes());
    let program_label = if owner_bytes == Some(spl_token::ID.to_bytes()) {
        "SPL Token"
    } else if owner_bytes == Some(spl_token_2022::ID.to_bytes()) {
        "Token-2022"
    } else {
        "Token"
    };

    if data
        .and_then(Value::as_object)
        .is_some_and(|obj| obj.contains_key("decimals") && obj.contains_key("supply"))
    {
        format!("{program_label} Mint")
    } else if data
        .and_then(Value::as_object)
        .is_some_and(|obj| obj.contains_key("mint") && obj.contains_key("token_owner"))
    {
        format!("{program_label} Account")
    } else {
        format!("{program_label} Data")
    }
}

fn has_account_meta_fields(value: &Value) -> bool {
    value.as_object().is_some_and(|obj| {
        obj.contains_key("lamports")
            && obj.contains_key("space")
            && obj.contains_key("owner")
            && obj.contains_key("executable")
            && obj.contains_key("rentEpoch")
    })
}

fn extract_data_for_text(value: &Value) -> Value {
    if has_account_meta_fields(value) {
        value.get("data").cloned().unwrap_or_else(|| value.clone())
    } else {
        value.clone()
    }
}

fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
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

/// Build ProgramData account payload.
/// Deserializes the UpgradeableLoaderState::ProgramData to extract upgrade authority and slot.
fn build_programdata_json(account: &solana_account::Account) -> Result<Value> {
    use solana_loader_v3_interface::state::UpgradeableLoaderState;

    const PROGRAM_DATA_HEADER_SIZE: usize = 45;

    let state: UpgradeableLoaderState = bincode::deserialize(account.data.as_slice())
        .with_context(|| "Failed to deserialize ProgramData account")?;

    if let UpgradeableLoaderState::ProgramData { slot, upgrade_authority_address } = state {
        let authority =
            upgrade_authority_address.map(|a| Pubkey::new_from_array(a.to_bytes()).to_string());
        let elf_size = account.data.len().saturating_sub(PROGRAM_DATA_HEADER_SIZE);

        Ok(serde_json::json!({
            "upgradeAuthority": authority,
            "lastDeployedSlot": slot,
            "elfSize": elf_size
        }))
    } else {
        anyhow::bail!("Account is not a ProgramData account");
    }
}

fn wrap_account_data_output<S: serde::Serialize>(
    account: &solana_account::Account,
    data: S,
) -> Value {
    serde_json::json!({
        "lamports": account.lamports,
        "space": account.data.len(),
        "owner": account.owner.to_string(),
        "executable": account.executable,
        "rentEpoch": account.rent_epoch,
        "data": data
    })
}

/// Print account data in Solana JSON RPC format.
/// Field order follows Solana Account struct: lamports, data, owner, executable, rent_epoch
fn print_raw_account_data(account: &solana_account::Account) {
    let output = raw_account_data_json(account);
    println!("{}", serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()));
}

fn raw_account_data_json(account: &solana_account::Account) -> Value {
    use base64::{Engine as _, engine::general_purpose};

    let data_b64 = general_purpose::STANDARD.encode(&account.data);
    serde_json::json!({
        "lamports": account.lamports,
        "data": data_b64,
        "owner": account.owner.to_string(),
        "executable": account.executable,
        "rentEpoch": account.rent_epoch,
        "space": account.data.len()
    })
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

fn resolve_metadata_output(
    token_json: &Value,
    metadata_result: Result<Value>,
) -> Result<(Value, Option<String>)> {
    match metadata_result {
        Ok(metadata_json) => Ok((metadata_json, None)),
        Err(error) => {
            let fallback = token_json.clone();
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
    use crate::parsers::token_account_decoder;
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
        )
        .expect("metadata failure should fallback");

        assert_eq!(output, token_json);
        assert!(warning.is_some());
    }

    #[test]
    fn metadata_decode_failure_falls_back_to_full_token_output() {
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
        )
        .expect("metadata decode failure should fallback");

        assert_eq!(output, token_json);
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
        )
        .expect("metadata failure should always fallback");

        assert_eq!(output, token_json);
        assert!(warning.is_some());
    }
}
