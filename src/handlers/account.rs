use std::fs;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::core::rpc_client::RpcClient;
use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose};
use serde_json::Value;
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_commitment_config::CommitmentConfig;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use solana_sdk_ids::{address_lookup_table, bpf_loader_upgradeable};

use crate::cli::AccountArgs;
use crate::parsers::instruction::anchor_idl::{RawAnchorIdl, ResolvedIdl, parse_account_data};
use crate::{
    core::idl_fetcher, parsers::metaplex_metadata_decoder, parsers::token_account_decoder,
};

pub(crate) fn handle(args: AccountArgs, json: bool) -> Result<()> {
    let (pubkey_str, account_pubkey, account) = match &args.account {
        Some(input) if input == "-" => load_account_json(std::io::stdin().lock())?,
        Some(input) => {
            let path = Path::new(input);
            if path.is_file() {
                let file = fs::File::open(path)
                    .with_context(|| format!("Failed to open file: {}", path.display()))?;
                load_account_json(file)?
            } else {
                let pk = Pubkey::from_str(input)
                    .with_context(|| format!("Invalid account pubkey: {input}"))?;
                let client = RpcClient::new(&args.rpc.rpc_url);
                let acct = client
                    .get_account(&pk)
                    .with_context(|| format!("Failed to fetch account: {pk}"))?;
                (pk.to_string(), pk, acct)
            }
        }
        None => {
            if std::io::stdin().is_terminal() {
                return Err(anyhow!(
                    "No account specified and stdin is a terminal.\n\
                     Usage: sonar account <PUBKEY_OR_FILE>\n\
                     Or pipe JSON: solana account <PUBKEY> --output json | sonar account"
                ));
            }
            load_account_json(std::io::stdin().lock())?
        }
    };

    if args.raw {
        print_raw_account_data(&account);
        return Ok(());
    }

    let client = RpcClient::new(&args.rpc.rpc_url);

    let (mut output, account_type, metadata_output) =
        decode_account_output(&args, &client, &account_pubkey, &account, json)?;

    if let Some(meta) = &metadata_output {
        output
            .as_object_mut()
            .expect("output must be a JSON object")
            .insert("metaplexMetadata".into(), meta.clone());
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        crate::output::render_account_text(
            &pubkey_str,
            &account,
            &account_type,
            &output,
            metadata_output.as_ref(),
        )?;
    }

    Ok(())
}

/// Parse a Solana account from any `Read` source (file, stdin, etc.).
///
/// Accepts the Solana CLI `--output json` format:
/// ```json
/// {
///   "pubkey": "<base58>",
///   "account": {
///     "lamports": 1141440,
///     "data": ["<base64>", "base64"],
///     "owner": "<base58>",
///     "executable": false,
///     "rentEpoch": 361,
///     "space": 36
///   }
/// }
/// ```
fn load_account_json<R: Read>(mut reader: R) -> Result<(String, Pubkey, solana_account::Account)> {
    let mut buf = String::new();
    reader.read_to_string(&mut buf).context("Failed to read account JSON")?;
    let trimmed = buf.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("No account data received"));
    }
    let json: Value = serde_json::from_str(trimmed).context("Invalid account JSON")?;
    let (pk_str, acct) = parse_solana_account_json(&json)?;
    let pk = Pubkey::from_str(&pk_str).unwrap_or_default();
    Ok((pk_str, pk, acct))
}

/// Parse the Solana CLI JSON format into a pubkey string and Account.
fn parse_solana_account_json(json: &Value) -> Result<(String, solana_account::Account)> {
    let pubkey_str = json
        .get("pubkey")
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| "Unknown".into());

    let acct = json.get("account").unwrap_or(json);

    let lamports = acct
        .get("lamports")
        .and_then(Value::as_u64)
        .with_context(|| "Missing or invalid 'lamports' field")?;

    let owner_str =
        acct.get("owner").and_then(Value::as_str).with_context(|| "Missing 'owner' field")?;
    let owner = Pubkey::from_str(owner_str)
        .with_context(|| format!("Invalid owner pubkey: {owner_str}"))?;

    let executable = acct.get("executable").and_then(Value::as_bool).unwrap_or(false);

    let rent_epoch = acct.get("rentEpoch").and_then(Value::as_u64).unwrap_or(0);

    let data = parse_account_data_field(acct).with_context(|| "Failed to parse 'data' field")?;

    Ok((pubkey_str, solana_account::Account { lamports, data, owner, executable, rent_epoch }))
}

/// Parse the `data` field which can be either `["<base64>", "base64"]` or a raw base64 string.
fn parse_account_data_field(acct: &Value) -> Result<Vec<u8>> {
    match acct.get("data") {
        Some(Value::Array(arr)) => {
            let b64 = arr
                .first()
                .and_then(Value::as_str)
                .with_context(|| "data array must contain a base64 string as the first element")?;
            general_purpose::STANDARD
                .decode(b64)
                .with_context(|| "Failed to decode base64 account data")
        }
        Some(Value::String(s)) => general_purpose::STANDARD
            .decode(s)
            .with_context(|| "Failed to decode base64 account data"),
        Some(_) => anyhow::bail!("'data' field has unexpected type"),
        None => Ok(Vec::new()),
    }
}

fn decode_account_output(
    args: &AccountArgs,
    client: &RpcClient,
    account_pubkey: &Pubkey,
    account: &solana_account::Account,
    json: bool,
) -> Result<(Value, String, Option<Value>)> {
    // Try each known account type in order; return on first match.
    if let Some(result) = decode_clock_sysvar(account_pubkey, account, json) {
        return Ok(result);
    }
    if let Some(result) = decode_rent_sysvar(account_pubkey, account) {
        return Ok(result);
    }
    if let Some(result) = decode_epoch_schedule_sysvar(account_pubkey, account) {
        return Ok(result);
    }
    if let Some(result) = decode_nonce_account(account) {
        return Ok(result);
    }
    if let Some(result) = decode_bpf_upgradeable(account)? {
        return Ok(result);
    }
    if let Some(result) = decode_address_lookup_table(account) {
        return Ok(result);
    }
    if let Some(result) = decode_spl_token(client, account_pubkey, account) {
        return Ok(result);
    }

    // IDL decode + fallback — depends on args and has complex fallback paths.
    let owner = account.owner;
    let idl_json = try_load_idl_from_dir(&args.idl_dir, &owner).or_else(|| {
        let fetcher = idl_fetcher::IdlFetcher::new(args.rpc.rpc_url.clone(), None).ok()?;
        fetcher.fetch_idl(&owner).ok().flatten()
    });

    let idl_json = match idl_json {
        Some(json) => json,
        None => {
            return Ok((raw_account_data_json(account), "Unknown".into(), None));
        }
    };

    let raw_idl: RawAnchorIdl =
        serde_json::from_str(&idl_json).with_context(|| "Failed to parse IDL JSON")?;
    let idl = ResolvedIdl::new(raw_idl.convert(&owner.to_string()));

    match parse_account_data(&idl, &account.data)? {
        Some((type_name, parsed_value)) => Ok((
            wrap_account_data_output(account, &parsed_value),
            format!("Anchor ({})", type_name),
            None,
        )),
        None => {
            let json = if account.data.len() >= 8 {
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
            Ok((json, "Unknown".into(), None))
        }
    }
}

/// Decode a Clock sysvar account. Only Clock needs `json` to toggle timestamp formatting.
fn decode_clock_sysvar(
    account_pubkey: &Pubkey,
    account: &solana_account::Account,
    json: bool,
) -> Option<(Value, String, Option<Value>)> {
    if *account_pubkey != solana_sdk_ids::sysvar::clock::id() {
        return None;
    }
    let clock = bincode::deserialize::<solana_clock::Clock>(account.data.as_slice()).ok()?;
    let data_json = if json {
        serde_json::json!({
            "slot": clock.slot,
            "epoch": clock.epoch,
            "leaderScheduleEpoch": clock.leader_schedule_epoch,
            "unixTimestamp": clock.unix_timestamp,
            "epochStartTimestamp": clock.epoch_start_timestamp,
        })
    } else {
        serde_json::json!({
            "slot": clock.slot,
            "epoch": clock.epoch,
            "leaderScheduleEpoch": clock.leader_schedule_epoch,
            "unixTimestamp": format_timestamp_with_utc(clock.unix_timestamp),
            "epochStartTimestamp": format_timestamp_with_utc(clock.epoch_start_timestamp),
        })
    };
    Some((wrap_account_data_output(account, data_json), "Sysvar Clock".into(), None))
}

/// Decode a Rent sysvar account.
fn decode_rent_sysvar(
    account_pubkey: &Pubkey,
    account: &solana_account::Account,
) -> Option<(Value, String, Option<Value>)> {
    if *account_pubkey != solana_sdk_ids::sysvar::rent::id() {
        return None;
    }
    let rent = bincode::deserialize::<solana_rent::Rent>(account.data.as_slice()).ok()?;
    let data_json = serde_json::json!({
        "lamportsPerByteYear": rent.lamports_per_byte_year,
        "exemptionThreshold": rent.exemption_threshold,
        "burnPercent": rent.burn_percent,
    });
    Some((wrap_account_data_output(account, data_json), "Sysvar Rent".into(), None))
}

/// Decode an EpochSchedule sysvar account.
fn decode_epoch_schedule_sysvar(
    account_pubkey: &Pubkey,
    account: &solana_account::Account,
) -> Option<(Value, String, Option<Value>)> {
    if *account_pubkey != solana_sdk_ids::sysvar::epoch_schedule::id() {
        return None;
    }
    let schedule =
        bincode::deserialize::<solana_epoch_schedule::EpochSchedule>(account.data.as_slice())
            .ok()?;
    let data_json = serde_json::json!({
        "slotsPerEpoch": schedule.slots_per_epoch,
        "leaderScheduleSlotOffset": schedule.leader_schedule_slot_offset,
        "warmup": schedule.warmup,
        "firstNormalEpoch": schedule.first_normal_epoch,
        "firstNormalSlot": schedule.first_normal_slot,
    });
    Some((wrap_account_data_output(account, data_json), "Sysvar Epoch Schedule".into(), None))
}

/// Decode a Nonce account (system program, 80-byte data).
fn decode_nonce_account(
    account: &solana_account::Account,
) -> Option<(Value, String, Option<Value>)> {
    if account.owner != solana_sdk_ids::system_program::id() || account.data.len() != 80 {
        return None;
    }
    let versions =
        bincode::deserialize::<solana_nonce::versions::Versions>(account.data.as_slice()).ok()?;
    if let solana_nonce::state::State::Initialized(data) = versions.state() {
        let data_json = serde_json::json!({
            "authority": data.authority.to_string(),
            "blockhash": data.blockhash().to_string(),
            "lamportsPerSignature": data.fee_calculator.lamports_per_signature,
        });
        Some((wrap_account_data_output(account, data_json), "Nonce Account".into(), None))
    } else {
        None
    }
}

/// Decode a BPF Upgradeable Loader account (Program, ProgramData, or Buffer).
fn decode_bpf_upgradeable(
    account: &solana_account::Account,
) -> Result<Option<(Value, String, Option<Value>)>> {
    if account.owner != bpf_loader_upgradeable::id() {
        return Ok(None);
    }
    let state = match bincode::deserialize::<UpgradeableLoaderState>(account.data.as_slice()) {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    match state {
        UpgradeableLoaderState::Program { programdata_address } => {
            let programdata_pubkey = Pubkey::new_from_array(programdata_address.to_bytes());
            let data_json = serde_json::json!({
                "programdataAddress": programdata_pubkey.to_string()
            });
            Ok(Some((
                wrap_account_data_output(account, data_json),
                "BPF Upgradeable Program".into(),
                None,
            )))
        }
        UpgradeableLoaderState::ProgramData { .. } => {
            let data_json = build_programdata_json(account)?;
            Ok(Some((wrap_account_data_output(account, data_json), "Program Data".into(), None)))
        }
        UpgradeableLoaderState::Buffer { authority_address, .. } => {
            const BUFFER_HEADER_SIZE: usize = 37;
            let data_size = account.data.len().saturating_sub(BUFFER_HEADER_SIZE);
            let data_json = serde_json::json!({
                "authority": authority_address
                    .map(|a| Pubkey::new_from_array(a.to_bytes()).to_string()),
                "dataSize": data_size
            });
            Ok(Some((wrap_account_data_output(account, data_json), "Buffer".into(), None)))
        }
        _ => Ok(None),
    }
}

/// Decode an Address Lookup Table account.
fn decode_address_lookup_table(
    account: &solana_account::Account,
) -> Option<(Value, String, Option<Value>)> {
    if account.owner != address_lookup_table::id() {
        return None;
    }
    let lookup_table = AddressLookupTable::deserialize(account.data.as_slice()).ok()?;
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
    Some((wrap_account_data_output(account, data_json), "Address Lookup Table".into(), None))
}

/// Decode an SPL Token or Token-2022 account. Returns `token_json` directly
/// (not wrapped through `wrap_account_data_output`), with optional Metaplex metadata.
fn decode_spl_token(
    client: &RpcClient,
    account_pubkey: &Pubkey,
    account: &solana_account::Account,
) -> Option<(Value, String, Option<Value>)> {
    let token_json = token_account_decoder::decode_spl_token_account(account)?;
    let account_type = detect_token_type(account, &token_json);

    let metadata_output = if should_enrich_with_metaplex_metadata(account, &token_json) {
        match fetch_metadata_for_mint(client, account_pubkey) {
            Ok((meta_account, decoded)) => Some(wrap_account_data_output(&meta_account, decoded)),
            Err(error) => {
                log::warn!(
                    "Metaplex metadata enrichment failed ({error}). \
                     Showing mint account data only."
                );
                None
            }
        }
    } else {
        None
    };

    Some((token_json, account_type, metadata_output))
}

fn detect_token_type(account: &solana_account::Account, token_json: &Value) -> String {
    let is_2022 = account.owner.to_bytes() == spl_token_2022::ID.to_bytes();
    let is_mint = token_json.get("data").and_then(|d| d.get("supply")).is_some();

    match (is_2022, is_mint) {
        (false, true) => "SPL Token Mint",
        (false, false) => "SPL Token Account",
        (true, true) => "Token-2022 Mint",
        (true, false) => "Token-2022 Account",
    }
    .to_string()
}

/// Try to load IDL from local directory (if specified).
/// IDL files are expected to be named `<PROGRAM_ID>.json`.
fn try_load_idl_from_dir(idl_dir: &Option<PathBuf>, owner: &Pubkey) -> Option<String> {
    let path = idl_dir.as_ref()?;
    let idl_file = path.join(format!("{}.json", owner));

    match fs::read_to_string(&idl_file) {
        Ok(content) => {
            log::debug!("Loaded IDL from {}", idl_file.display());
            Some(content)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log::debug!("IDL file not found: {}", idl_file.display());
            None
        }
        Err(e) => {
            log::warn!("Failed to read IDL file {}: {}", idl_file.display(), e);
            None
        }
    }
}

/// Build ProgramData account payload.
/// Deserializes the UpgradeableLoaderState::ProgramData to extract upgrade authority and slot.
fn format_timestamp_with_utc(ts: i64) -> String {
    match chrono::DateTime::from_timestamp(ts, 0) {
        Some(dt) => format!("{} ({})", ts, dt.format("%Y-%m-%d %H:%M:%S UTC")),
        None => ts.to_string(),
    }
}

fn build_programdata_json(account: &solana_account::Account) -> Result<Value> {
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

fn fetch_metadata_for_mint(
    client: &RpcClient,
    mint_pubkey: &Pubkey,
) -> Result<(solana_account::Account, Value)> {
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

    let decoded = metaplex_metadata_decoder::decode_metadata_account_data(&metadata_account.data)
        .with_context(|| {
        format!(
            "Failed to decode metaplex metadata account {} for mint {}",
            metadata_pda, mint_pubkey
        )
    })?;

    Ok((metadata_account, decoded))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::{
        load_account_json, parse_account_data_field, parse_solana_account_json,
        should_enrich_with_metaplex_metadata,
    };
    use crate::parsers::token_account_decoder;
    use base64::{Engine as _, engine::general_purpose};
    use solana_pubkey::Pubkey;
    use spl_token::solana_program::program_option::COption;
    use spl_token::solana_program::program_pack::Pack;
    use spl_token::solana_program::pubkey::Pubkey as ProgramPubkey;
    use spl_token::state::{Account as TokenAccount, AccountState, Mint as LegacyMint};
    use spl_token_2022::state::Mint as Token2022Mint;

    fn legacy_owner_pubkey() -> Pubkey {
        Pubkey::new_from_array(spl_token::ID.to_bytes())
    }

    fn token2022_owner_pubkey() -> Pubkey {
        Pubkey::new_from_array(spl_token_2022::ID.to_bytes())
    }

    #[test]
    fn should_enrich_metadata_for_legacy_mint() {
        let mint = LegacyMint {
            mint_authority: COption::Some(ProgramPubkey::new_unique()),
            supply: 1_000_000,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        let mut data = vec![0u8; LegacyMint::LEN];
        LegacyMint::pack(mint, &mut data).unwrap();

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
        let mint = Token2022Mint {
            mint_authority: COption::Some(ProgramPubkey::new_unique()),
            supply: 2_000_000,
            decimals: 9,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        let mut data = vec![0u8; Token2022Mint::LEN];
        Token2022Mint::pack(mint, &mut data).unwrap();

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
    fn load_account_from_solana_cli_json_file() {
        let raw_data = vec![1u8, 2, 3, 4, 5];
        let b64 = general_purpose::STANDARD.encode(&raw_data);
        let owner = "11111111111111111111111111111111";
        let pubkey = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
        let json = serde_json::json!({
            "pubkey": pubkey,
            "account": {
                "lamports": 1_000_000,
                "data": [b64, "base64"],
                "owner": owner,
                "executable": false,
                "rentEpoch": 42,
                "space": 5
            }
        });

        let mut tmp = tempfile::NamedTempFile::with_suffix(".json").unwrap();
        write!(tmp, "{}", serde_json::to_string(&json).unwrap()).unwrap();

        let file = std::fs::File::open(tmp.path()).unwrap();
        let (pk_str, pk, acct) = load_account_json(file).unwrap();
        assert_eq!(pk_str, pubkey);
        assert_eq!(pk.to_string(), pubkey);
        assert_eq!(acct.lamports, 1_000_000);
        assert_eq!(acct.data, raw_data);
        assert_eq!(acct.owner.to_string(), owner);
        assert!(!acct.executable);
        assert_eq!(acct.rent_epoch, 42);
    }

    #[test]
    fn load_account_flat_json_without_pubkey() {
        let raw_data = vec![10u8, 20, 30];
        let b64 = general_purpose::STANDARD.encode(&raw_data);
        let json = serde_json::json!({
            "lamports": 500,
            "data": [b64, "base64"],
            "owner": "11111111111111111111111111111111",
            "executable": true,
            "rentEpoch": 0
        });

        let mut tmp = tempfile::NamedTempFile::with_suffix(".json").unwrap();
        write!(tmp, "{}", serde_json::to_string(&json).unwrap()).unwrap();

        let file = std::fs::File::open(tmp.path()).unwrap();
        let (pk_str, _pk, acct) = load_account_json(file).unwrap();
        assert_eq!(pk_str, "Unknown");
        assert_eq!(acct.lamports, 500);
        assert_eq!(acct.data, raw_data);
        assert!(acct.executable);
    }

    #[test]
    fn parse_data_field_base64_string() {
        let raw = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let b64 = general_purpose::STANDARD.encode(&raw);
        let json = serde_json::json!({ "data": b64 });
        let parsed = parse_account_data_field(&json).unwrap();
        assert_eq!(parsed, raw);
    }

    #[test]
    fn parse_data_field_missing_returns_empty() {
        let json = serde_json::json!({ "lamports": 1 });
        let parsed = parse_account_data_field(&json).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn load_account_json_empty_fails() {
        let result = load_account_json("".as_bytes());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("No account data"), "unexpected error: {msg}");
    }

    #[test]
    fn parse_solana_account_json_flat_format() {
        let raw_data = vec![10u8, 20, 30];
        let b64 = general_purpose::STANDARD.encode(&raw_data);
        let json = serde_json::json!({
            "lamports": 500,
            "data": [b64, "base64"],
            "owner": "11111111111111111111111111111111",
            "executable": true,
            "rentEpoch": 0
        });
        let (pk_str, acct) = parse_solana_account_json(&json).unwrap();
        assert_eq!(pk_str, "Unknown");
        assert_eq!(acct.lamports, 500);
        assert_eq!(acct.data, raw_data);
        assert!(acct.executable);
    }
}
