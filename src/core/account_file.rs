use std::path::Path;
use std::str::FromStr;

use serde::Deserialize;
use solana_account::Account;
use solana_pubkey::Pubkey;

/// JSON structure for deserializing an account file (flat format).
/// Supports the simple `{ "lamports": ..., "data": ... }` format.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountJsonFlat {
    lamports: u64,
    data: AccountDataJson,
    owner: String,
    #[serde(default)]
    executable: bool,
    #[serde(default)]
    rent_epoch: u64,
}

/// JSON structure for deserializing a Solana CLI style account file (nested format).
/// Supports `{ "pubkey": "...", "account": { "lamports": ..., "data": ... } }`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountJsonNested {
    #[allow(dead_code)]
    pubkey: String,
    account: AccountJsonFlat,
}

/// Account data can be either a plain base64 string or a tuple `["base64data", "base64"]`.
#[derive(Deserialize)]
#[serde(untagged)]
enum AccountDataJson {
    Plain(String),
    Tuple(String, String),
}

pub fn parse_account_json(path: &Path) -> Result<Account, String> {
    use base64::Engine;

    let contents = std::fs::read_to_string(path)
        .map_err(|err| format!("Failed to read account file `{}`: {err}", path.display()))?;

    let json: AccountJsonFlat = if let Ok(nested) =
        serde_json::from_str::<AccountJsonNested>(&contents)
    {
        nested.account
    } else {
        serde_json::from_str(&contents)
            .map_err(|err| format!("Failed to parse account JSON `{}`: {err}", path.display()))?
    };

    let data_b64 = match &json.data {
        AccountDataJson::Plain(s) => s.clone(),
        AccountDataJson::Tuple(data, _encoding) => data.clone(),
    };

    let data = base64::engine::general_purpose::STANDARD
        .decode(&data_b64)
        .map_err(|err| format!("Failed to decode base64 data in `{}`: {err}", path.display()))?;

    let owner = Pubkey::from_str(&json.owner)
        .map_err(|err| format!("Failed to parse owner `{}`: {err}", json.owner))?;

    Ok(Account {
        lamports: json.lamports,
        data,
        owner,
        executable: json.executable,
        rent_epoch: json.rent_epoch,
    })
}
