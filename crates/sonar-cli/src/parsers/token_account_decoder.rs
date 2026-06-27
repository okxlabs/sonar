//! Token and Token-2022 account decoder.
//!
//! This module provides functionality to decode SPL Token and Token-2022
//! mint and token account data, including Token-2022 extensions.

use serde_json::{Value, json};
use solana_pubkey::Pubkey;
use spl_token::solana_program::program_option::COption;
use spl_token::solana_program::program_pack::Pack;
use spl_token::state::{Account as LegacyTokenAccount, Mint as LegacyMint};
use spl_token_2022::extension::{
    BaseStateWithExtensions, ExtensionType, StateWithExtensions,
    confidential_mint_burn::ConfidentialMintBurn,
    confidential_transfer::{ConfidentialTransferAccount, ConfidentialTransferMint},
    confidential_transfer_fee::{ConfidentialTransferFeeAmount, ConfidentialTransferFeeConfig},
    cpi_guard::CpiGuard,
    default_account_state::DefaultAccountState,
    group_member_pointer::GroupMemberPointer,
    group_pointer::GroupPointer,
    interest_bearing_mint::InterestBearingConfig,
    memo_transfer::MemoTransfer,
    metadata_pointer::MetadataPointer,
    mint_close_authority::MintCloseAuthority,
    pausable::PausableConfig,
    permanent_delegate::PermanentDelegate,
    scaled_ui_amount::ScaledUiAmountConfig,
    transfer_fee::{TransferFeeAmount, TransferFeeConfig},
    transfer_hook::{TransferHook, TransferHookAccount},
};
use spl_token_2022::state::{Account as Token2022Account, Mint as Token2022Mint};
use spl_token_group_interface::state::{TokenGroup, TokenGroupMember};
use spl_token_metadata_interface::state::TokenMetadata;

/// Token program ID
fn legacy_program_id() -> Pubkey {
    Pubkey::new_from_array(spl_token::ID.to_bytes())
}

/// Token-2022 program ID
fn token2022_program_id() -> Pubkey {
    Pubkey::new_from_array(spl_token_2022::ID.to_bytes())
}

/// Token program kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenProgramKind {
    Legacy,
    Token2022,
}

impl TokenProgramKind {
    fn from_owner(owner: &Pubkey) -> Option<Self> {
        if *owner == legacy_program_id() {
            Some(TokenProgramKind::Legacy)
        } else if *owner == token2022_program_id() {
            Some(TokenProgramKind::Token2022)
        } else {
            None
        }
    }
}

/// Decode a token account (mint or token account) if the owner is SPL Token or Token-2022.
///
/// Returns `Some(json)` if successfully decoded, `None` if the owner is not a token program
/// or if the data cannot be parsed.
pub fn decode_spl_token_account(account: &solana_account::Account) -> Option<Value> {
    let kind = TokenProgramKind::from_owner(&account.owner)?;

    match kind {
        TokenProgramKind::Legacy => decode_legacy_token_account(account),
        TokenProgramKind::Token2022 => decode_token2022_account(account),
    }
}

/// Decode legacy SPL Token account (mint or token account)
fn decode_legacy_token_account(account: &solana_account::Account) -> Option<Value> {
    let data = &account.data;

    // Try to decode as Mint first (82 bytes)
    if data.len() == LegacyMint::LEN {
        if let Ok(mint) = LegacyMint::unpack(data) {
            return Some(build_legacy_mint_json(account, &mint));
        }
    }

    // Try to decode as Token Account (165 bytes)
    if data.len() == LegacyTokenAccount::LEN {
        if let Ok(token_account) = LegacyTokenAccount::unpack(data) {
            return Some(build_legacy_account_json(account, &token_account));
        }
    }

    // If exact length doesn't match, try both
    if data.len() >= LegacyMint::LEN {
        if let Ok(mint) = LegacyMint::unpack(&data[..LegacyMint::LEN]) {
            // Check if it looks like an initialized mint
            if mint.is_initialized {
                return Some(build_legacy_mint_json(account, &mint));
            }
        }
    }

    if data.len() >= LegacyTokenAccount::LEN {
        if let Ok(token_account) = LegacyTokenAccount::unpack(&data[..LegacyTokenAccount::LEN]) {
            return Some(build_legacy_account_json(account, &token_account));
        }
    }

    None
}

/// Build JSON for legacy Mint
fn build_legacy_mint_json(
    account: &solana_account::Account,
    mint: &spl_token::state::Mint,
) -> Value {
    let mut result = base_account_json(account);
    let obj = result.as_object_mut().unwrap();
    obj.insert(
        "data".into(),
        json!({
            "mint_authority": coption_pubkey_to_json(&mint.mint_authority),
            "supply": mint.supply.to_string(),
            "decimals": mint.decimals,
            "is_initialized": mint.is_initialized,
            "freeze_authority": coption_pubkey_to_json(&mint.freeze_authority)
        }),
    );
    result
}

/// Build JSON for legacy Token Account
fn build_legacy_account_json(
    account: &solana_account::Account,
    token_account: &spl_token::state::Account,
) -> Value {
    let mut result = base_account_json(account);
    let obj = result.as_object_mut().unwrap();
    obj.insert(
        "data".into(),
        json!({
            "mint": token_account.mint.to_string(),
            "token_owner": token_account.owner.to_string(),
            "amount": token_account.amount.to_string(),
            "delegate": coption_pubkey_to_json(&token_account.delegate),
            "state": format!("{:?}", token_account.state),
            "is_native": coption_u64_to_json(&token_account.is_native),
            "delegated_amount": token_account.delegated_amount.to_string(),
            "close_authority": coption_pubkey_to_json(&token_account.close_authority)
        }),
    );
    result
}

/// Decode Token-2022 account (mint or token account with extensions)
fn decode_token2022_account(account: &solana_account::Account) -> Option<Value> {
    let data = &account.data;

    // Token-2022 uses StateWithExtensions to parse accounts with extensions
    // Try Mint first, then Account

    // Try to decode as Mint
    if let Ok(mint_state) = StateWithExtensions::<Token2022Mint>::unpack(data) {
        return Some(build_token2022_mint_json(account, &mint_state));
    }

    // Try to decode as Token Account
    if let Ok(account_state) = StateWithExtensions::<Token2022Account>::unpack(data) {
        return Some(build_token2022_account_json(account, &account_state));
    }

    None
}

/// Build JSON for Token-2022 Mint with extensions
fn build_token2022_mint_json(
    account: &solana_account::Account,
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
) -> Value {
    let mint = &state.base;
    let mut result = base_account_json(account);

    let mut data = serde_json::Map::new();
    data.insert("mint_authority".into(), coption_pubkey_to_json(&mint.mint_authority));
    data.insert("supply".into(), json!(mint.supply.to_string()));
    data.insert("decimals".into(), json!(mint.decimals));
    data.insert("is_initialized".into(), json!(mint.is_initialized));
    data.insert("freeze_authority".into(), coption_pubkey_to_json(&mint.freeze_authority));

    // Parse extensions
    if let Ok(extension_types) = state.get_extension_types() {
        if !extension_types.is_empty() {
            let extensions = parse_mint_extensions(state, &extension_types);
            data.insert("extensions".into(), Value::Array(extensions));
        }
    }

    let obj = result.as_object_mut().unwrap();
    obj.insert("data".into(), Value::Object(data));
    result
}

/// Build JSON for Token-2022 Token Account with extensions
fn build_token2022_account_json(
    account: &solana_account::Account,
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Account>,
) -> Value {
    let token_account = &state.base;
    let mut result = base_account_json(account);

    let mut data = serde_json::Map::new();
    data.insert("mint".into(), json!(token_account.mint.to_string()));
    data.insert("token_owner".into(), json!(token_account.owner.to_string()));
    data.insert("amount".into(), json!(token_account.amount.to_string()));
    data.insert("delegate".into(), coption_pubkey_to_json(&token_account.delegate));
    data.insert("state".into(), json!(format!("{:?}", token_account.state)));
    data.insert("is_native".into(), coption_u64_to_json(&token_account.is_native));
    data.insert("delegated_amount".into(), json!(token_account.delegated_amount.to_string()));
    data.insert("close_authority".into(), coption_pubkey_to_json(&token_account.close_authority));

    // Parse extensions
    if let Ok(extension_types) = state.get_extension_types() {
        if !extension_types.is_empty() {
            let extensions = parse_account_extensions(state, &extension_types);
            data.insert("extensions".into(), Value::Array(extensions));
        }
    }

    let obj = result.as_object_mut().unwrap();
    obj.insert("data".into(), Value::Object(data));
    result
}

/// Macro to reduce boilerplate for extension parsing
macro_rules! ext_json {
    ($state:expr, $type_name:literal, $ext_type:ty, |$ext:ident| $data:expr) => {{
        match $state.get_extension::<$ext_type>() {
            Ok($ext) => json!({ "type": $type_name, "data": $data }),
            Err(_) => null_extension($type_name),
        }
    }};
}

/// Macro for variable-length extensions (like TokenMetadata)
macro_rules! ext_json_varlen {
    ($state:expr, $type_name:literal, $ext_type:ty, |$ext:ident| $data:expr) => {{
        match $state.get_variable_len_extension::<$ext_type>() {
            Ok($ext) => json!({ "type": $type_name, "data": $data }),
            Err(_) => null_extension($type_name),
        }
    }};
}

/// Parse mint extensions
fn parse_mint_extensions(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
    extension_types: &[ExtensionType],
) -> Vec<Value> {
    extension_types
        .iter()
        .map(|ext_type| match ext_type {
            ExtensionType::TransferFeeConfig => ext_json!(state, "TransferFeeConfig", TransferFeeConfig, |c| {
                json!({
                    "transfer_fee_config_authority": pod_option_pubkey_to_string(&c.transfer_fee_config_authority),
                    "withdraw_withheld_authority": pod_option_pubkey_to_string(&c.withdraw_withheld_authority),
                    "withheld_amount": u64::from(c.withheld_amount).to_string(),
                    "older_transfer_fee": {
                        "epoch": u64::from(c.older_transfer_fee.epoch).to_string(),
                        "maximum_fee": u64::from(c.older_transfer_fee.maximum_fee).to_string(),
                        "transfer_fee_basis_points": u16::from(c.older_transfer_fee.transfer_fee_basis_points)
                    },
                    "newer_transfer_fee": {
                        "epoch": u64::from(c.newer_transfer_fee.epoch).to_string(),
                        "maximum_fee": u64::from(c.newer_transfer_fee.maximum_fee).to_string(),
                        "transfer_fee_basis_points": u16::from(c.newer_transfer_fee.transfer_fee_basis_points)
                    }
                })
            }),
            ExtensionType::MintCloseAuthority => ext_json!(state, "MintCloseAuthority", MintCloseAuthority, |e| {
                json!({ "close_authority": pod_option_pubkey_to_string(&e.close_authority) })
            }),
            ExtensionType::InterestBearingConfig => ext_json!(state, "InterestBearingConfig", InterestBearingConfig, |c| {
                json!({
                    "rate_authority": pod_option_pubkey_to_string(&c.rate_authority),
                    "initialization_timestamp": i64::from(c.initialization_timestamp).to_string(),
                    "pre_update_average_rate": i16::from(c.pre_update_average_rate),
                    "last_update_timestamp": i64::from(c.last_update_timestamp).to_string(),
                    "current_rate": i16::from(c.current_rate)
                })
            }),
            ExtensionType::PermanentDelegate => ext_json!(state, "PermanentDelegate", PermanentDelegate, |e| {
                json!({ "delegate": pod_option_pubkey_to_string(&e.delegate) })
            }),
            ExtensionType::DefaultAccountState => ext_json!(state, "DefaultAccountState", DefaultAccountState, |e| {
                let state_str = match e.state {
                    0 => "Uninitialized", 1 => "Initialized", 2 => "Frozen", _ => "Unknown"
                };
                json!({ "state": state_str })
            }),
            ExtensionType::MetadataPointer => ext_json!(state, "MetadataPointer", MetadataPointer, |e| {
                json!({
                    "authority": pod_option_pubkey_to_string(&e.authority),
                    "metadata_address": pod_option_pubkey_to_string(&e.metadata_address)
                })
            }),
            ExtensionType::GroupPointer => ext_json!(state, "GroupPointer", GroupPointer, |e| {
                json!({
                    "authority": pod_option_pubkey_to_string(&e.authority),
                    "group_address": pod_option_pubkey_to_string(&e.group_address)
                })
            }),
            ExtensionType::GroupMemberPointer => ext_json!(state, "GroupMemberPointer", GroupMemberPointer, |e| {
                json!({
                    "authority": pod_option_pubkey_to_string(&e.authority),
                    "member_address": pod_option_pubkey_to_string(&e.member_address)
                })
            }),
            ExtensionType::TransferHook => ext_json!(state, "TransferHook", TransferHook, |e| {
                json!({
                    "authority": pod_option_pubkey_to_string(&e.authority),
                    "program_id": pod_option_pubkey_to_string(&e.program_id)
                })
            }),
            ExtensionType::Pausable => ext_json!(state, "Pausable", PausableConfig, |c| {
                json!({
                    "authority": pod_option_pubkey_to_string(&c.authority),
                    "paused": bool::from(c.paused)
                })
            }),
            ExtensionType::ScaledUiAmount => ext_json!(state, "ScaledUiAmount", ScaledUiAmountConfig, |c| {
                json!({
                    "authority": pod_option_pubkey_to_string(&c.authority),
                    "multiplier": f64::from(c.multiplier),
                    "new_multiplier_effective_timestamp": i64::from(c.new_multiplier_effective_timestamp).to_string(),
                    "new_multiplier": f64::from(c.new_multiplier)
                })
            }),
            ExtensionType::TokenMetadata => ext_json_varlen!(state, "TokenMetadata", TokenMetadata, |m| {
                json!({
                    "update_authority": pod_option_pubkey_to_string(&m.update_authority),
                    "mint": m.mint.to_string(),
                    "name": m.name,
                    "symbol": m.symbol,
                    "uri": m.uri,
                    "additional_metadata": m.additional_metadata.iter()
                        .map(|(k, v)| json!({"key": k, "value": v}))
                        .collect::<Vec<_>>()
                })
            }),
            ExtensionType::TokenGroup => ext_json!(state, "TokenGroup", TokenGroup, |g| {
                json!({
                    "update_authority": pod_option_pubkey_to_string(&g.update_authority),
                    "mint": g.mint.to_string(),
                    "size": u64::from(g.size).to_string(),
                    "max_size": u64::from(g.max_size).to_string()
                })
            }),
            ExtensionType::TokenGroupMember => ext_json!(state, "TokenGroupMember", TokenGroupMember, |m| {
                json!({
                    "mint": m.mint.to_string(),
                    "group": m.group.to_string(),
                    "member_number": u64::from(m.member_number).to_string()
                })
            }),
            // Marker extensions (no data)
            ExtensionType::NonTransferable => marker_extension("NonTransferable"),
            ExtensionType::ConfidentialTransferMint => {
                ext_json!(state, "ConfidentialTransferMint", ConfidentialTransferMint, |c| {
                    json!({
                        "authority": pod_option_pubkey_to_string(&c.authority),
                        "auto_approve_new_accounts": bool::from(c.auto_approve_new_accounts),
                        "auditor_elgamal_pubkey": pod_option_elgamal_to_string(&c.auditor_elgamal_pubkey)
                    })
                })
            }
            ExtensionType::ConfidentialTransferFeeConfig => {
                ext_json!(state, "ConfidentialTransferFeeConfig", ConfidentialTransferFeeConfig, |c| {
                    json!({
                        "authority": pod_option_pubkey_to_string(&c.authority),
                        "withdraw_withheld_authority_elgamal_pubkey": c.withdraw_withheld_authority_elgamal_pubkey.to_string(),
                        "harvest_to_mint_enabled": bool::from(c.harvest_to_mint_enabled),
                        "withheld_amount": c.withheld_amount.to_string()
                    })
                })
            }
            ExtensionType::ConfidentialMintBurn => {
                ext_json!(state, "ConfidentialMintBurn", ConfidentialMintBurn, |c| {
                    json!({
                        "confidential_supply": c.confidential_supply.to_string(),
                        "decryptable_supply": c.decryptable_supply.to_string(),
                        "supply_elgamal_pubkey": c.supply_elgamal_pubkey.to_string(),
                        "pending_burn": c.pending_burn.to_string()
                    })
                })
            }
            // Unknown extensions
            _ => null_extension(&format!("{:?}", ext_type)),
        })
        .collect()
}

/// Parse account extensions
fn parse_account_extensions(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Account>,
    extension_types: &[ExtensionType],
) -> Vec<Value> {
    extension_types
        .iter()
        .map(|ext_type| match ext_type {
            ExtensionType::TransferFeeAmount => ext_json!(state, "TransferFeeAmount", TransferFeeAmount, |a| {
                json!({ "withheld_amount": u64::from(a.withheld_amount).to_string() })
            }),
            ExtensionType::MemoTransfer => ext_json!(state, "MemoTransfer", MemoTransfer, |e| {
                json!({ "require_incoming_transfer_memos": bool::from(e.require_incoming_transfer_memos) })
            }),
            ExtensionType::CpiGuard => ext_json!(state, "CpiGuard", CpiGuard, |e| {
                json!({ "lock_cpi": bool::from(e.lock_cpi) })
            }),
            ExtensionType::TransferHookAccount => ext_json!(state, "TransferHookAccount", TransferHookAccount, |e| {
                json!({ "transferring": bool::from(e.transferring) })
            }),
            // Marker extensions (no data)
            ExtensionType::ImmutableOwner => marker_extension("ImmutableOwner"),
            ExtensionType::NonTransferableAccount => marker_extension("NonTransferableAccount"),
            ExtensionType::PausableAccount => marker_extension("PausableAccount"),
            ExtensionType::ConfidentialTransferAccount => {
                ext_json!(state, "ConfidentialTransferAccount", ConfidentialTransferAccount, |a| {
                    json!({
                        "approved": bool::from(a.approved),
                        "elgamal_pubkey": a.elgamal_pubkey.to_string(),
                        "pending_balance_lo": a.pending_balance_lo.to_string(),
                        "pending_balance_hi": a.pending_balance_hi.to_string(),
                        "available_balance": a.available_balance.to_string(),
                        "decryptable_available_balance": a.decryptable_available_balance.to_string(),
                        "allow_confidential_credits": bool::from(a.allow_confidential_credits),
                        "allow_non_confidential_credits": bool::from(a.allow_non_confidential_credits),
                        "pending_balance_credit_counter": u64::from(a.pending_balance_credit_counter).to_string(),
                        "maximum_pending_balance_credit_counter": u64::from(a.maximum_pending_balance_credit_counter).to_string(),
                        "expected_pending_balance_credit_counter": u64::from(a.expected_pending_balance_credit_counter).to_string(),
                        "actual_pending_balance_credit_counter": u64::from(a.actual_pending_balance_credit_counter).to_string()
                    })
                })
            }
            ExtensionType::ConfidentialTransferFeeAmount => {
                ext_json!(state, "ConfidentialTransferFeeAmount", ConfidentialTransferFeeAmount, |a| {
                    json!({ "withheld_amount": a.withheld_amount.to_string() })
                })
            }
            // Unknown extensions
            _ => null_extension(&format!("{:?}", ext_type)),
        })
        .collect()
}

// ============================================================================
// Helper functions
// ============================================================================

/// Build base account metadata JSON (shared by all account types)
/// Field order follows Solana Account struct: lamports, data(space), owner, executable, rent_epoch
fn base_account_json(account: &solana_account::Account) -> Value {
    json!({
        "lamports": account.lamports,
        "space": account.data.len(),
        "owner": account.owner.to_string(),
        "executable": account.executable,
        "rentEpoch": account.rent_epoch
    })
}

/// Create a marker extension JSON (extensions with no data fields)
fn marker_extension(type_name: &str) -> Value {
    json!({
        "type": type_name,
        "data": {}
    })
}

/// Create an extension JSON with null data (for parse failures or unsupported extensions)
fn null_extension(type_name: &str) -> Value {
    json!({
        "type": type_name,
        "data": null
    })
}

fn coption_pubkey_to_json(opt: &COption<spl_token::solana_program::pubkey::Pubkey>) -> Value {
    match opt {
        COption::Some(pk) => Value::String(pk.to_string()),
        COption::None => Value::Null,
    }
}

fn coption_u64_to_json(opt: &COption<u64>) -> Value {
    match opt {
        COption::Some(v) => Value::String(v.to_string()),
        COption::None => Value::Null,
    }
}

fn pod_option_pubkey_to_string(opt: &spl_pod::optional_keys::OptionalNonZeroPubkey) -> Value {
    // OptionalNonZeroPubkey.0 is a solana_pubkey::Pubkey
    // If all bytes are zero, it represents None
    let pk: solana_pubkey::Pubkey = opt.0;
    if pk == solana_pubkey::Pubkey::default() { Value::Null } else { Value::String(pk.to_string()) }
}

/// Render an `OptionalNonZeroElGamalPubkey` as a base64 string, or null when
/// unset (all-zero, which encodes `None`).
fn pod_option_elgamal_to_string(
    opt: &spl_pod::optional_keys::OptionalNonZeroElGamalPubkey,
) -> Value {
    // The `From<OptionalNonZeroElGamalPubkey> for Option<PodElGamalPubkey>` impl
    // maps the all-zero sentinel to `None`; `PodElGamalPubkey` Displays as base64.
    let maybe: Option<solana_zk_sdk::encryption::pod::elgamal::PodElGamalPubkey> = (*opt).into();
    match maybe {
        Some(pk) => Value::String(pk.to_string()),
        None => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spl_token::solana_program::program_option::COption;
    use spl_token::solana_program::program_pack::Pack;
    use spl_token::solana_program::pubkey::Pubkey as ProgramPubkey;
    use spl_token::state::{
        Account as LegacyTokenAccount, AccountState as LegacyAccountState, Mint as LegacyMint,
    };
    use spl_token_2022::state::{
        Account as Token2022Account, AccountState as Token2022AccountState, Mint as Token2022Mint,
    };

    #[test]
    fn test_decode_legacy_mint() {
        let mint_authority = ProgramPubkey::new_unique();
        let mint = LegacyMint {
            mint_authority: COption::Some(mint_authority),
            supply: 1_000_000_000,
            decimals: 9,
            is_initialized: true,
            freeze_authority: COption::None,
        };

        let mut data = vec![0u8; LegacyMint::LEN];
        LegacyMint::pack(mint, &mut data).unwrap();

        let account = solana_account::Account {
            lamports: 1_000_000,
            data,
            owner: legacy_program_id(),
            executable: false,
            rent_epoch: 0,
        };

        let result = decode_spl_token_account(&account);
        assert!(result.is_some());

        let json = result.unwrap();
        // Account metadata (top-level)
        assert_eq!(json["lamports"], 1_000_000);
        assert_eq!(json["owner"], legacy_program_id().to_string());
        assert_eq!(json["executable"], false);
        assert_eq!(json["rentEpoch"], 0);
        assert_eq!(json["space"], LegacyMint::LEN);
        // Mint data (nested under "data")
        assert_eq!(json["data"]["decimals"], 9);
        assert_eq!(json["data"]["supply"], "1000000000");
        assert!(json["data"]["is_initialized"].as_bool().unwrap());
    }

    #[test]
    fn test_decode_legacy_token_account() {
        let mint = ProgramPubkey::new_unique();
        let token_owner = ProgramPubkey::new_unique();
        let token_account = LegacyTokenAccount {
            mint,
            owner: token_owner,
            amount: 500_000,
            delegate: COption::None,
            state: LegacyAccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        };

        let mut data = vec![0u8; LegacyTokenAccount::LEN];
        LegacyTokenAccount::pack(token_account, &mut data).unwrap();

        let account = solana_account::Account {
            lamports: 2_000_000,
            data,
            owner: legacy_program_id(),
            executable: false,
            rent_epoch: 0,
        };

        let result = decode_spl_token_account(&account);
        assert!(result.is_some());

        let json = result.unwrap();
        // Account metadata (top-level)
        assert_eq!(json["lamports"], 2_000_000);
        assert_eq!(json["owner"], legacy_program_id().to_string());
        assert_eq!(json["executable"], false);
        assert_eq!(json["space"], LegacyTokenAccount::LEN);
        // Token account data (nested under "data")
        assert_eq!(json["data"]["token_owner"], token_owner.to_string());
        assert_eq!(json["data"]["amount"], "500000");
        assert_eq!(json["data"]["state"], "Initialized");
    }

    #[test]
    fn test_non_token_owner_returns_none() {
        let random_owner = Pubkey::new_unique();
        let account = solana_account::Account {
            lamports: 1_000_000,
            data: vec![0u8; 100],
            owner: random_owner,
            executable: false,
            rent_epoch: 0,
        };

        let result = decode_spl_token_account(&account);
        assert!(result.is_none());
    }

    #[test]
    fn test_invalid_data_returns_none() {
        // Too short data
        let account = solana_account::Account {
            lamports: 1_000_000,
            data: vec![0u8; 10],
            owner: legacy_program_id(),
            executable: false,
            rent_epoch: 0,
        };

        let result = decode_spl_token_account(&account);
        assert!(result.is_none());
    }

    #[test]
    fn test_decode_token2022_mint() {
        let mint_authority = ProgramPubkey::new_unique();
        let mint = Token2022Mint {
            mint_authority: COption::Some(mint_authority),
            supply: 2_000_000_000,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        };

        let mut data = vec![0u8; Token2022Mint::LEN];
        Token2022Mint::pack(mint, &mut data).unwrap();

        let account = solana_account::Account {
            lamports: 3_000_000,
            data,
            owner: token2022_program_id(),
            executable: false,
            rent_epoch: 0,
        };

        let result = decode_spl_token_account(&account);
        assert!(result.is_some());

        let json = result.unwrap();
        // Account metadata (top-level)
        assert_eq!(json["lamports"], 3_000_000);
        assert_eq!(json["owner"], token2022_program_id().to_string());
        assert_eq!(json["executable"], false);
        assert_eq!(json["space"], Token2022Mint::LEN);
        // Mint data (nested under "data")
        assert_eq!(json["data"]["decimals"], 6);
        assert_eq!(json["data"]["supply"], "2000000000");
        assert!(json["data"]["is_initialized"].as_bool().unwrap());
    }

    #[test]
    fn test_decode_token2022_token_account() {
        let mint = ProgramPubkey::new_unique();
        let token_owner = ProgramPubkey::new_unique();
        let token_account = Token2022Account {
            mint,
            owner: token_owner,
            amount: 750_000,
            delegate: COption::None,
            state: Token2022AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        };

        let mut data = vec![0u8; Token2022Account::LEN];
        Token2022Account::pack(token_account, &mut data).unwrap();

        let account = solana_account::Account {
            lamports: 4_000_000,
            data,
            owner: token2022_program_id(),
            executable: false,
            rent_epoch: 0,
        };

        let result = decode_spl_token_account(&account);
        assert!(result.is_some());

        let json = result.unwrap();
        // Account metadata (top-level)
        assert_eq!(json["lamports"], 4_000_000);
        assert_eq!(json["owner"], token2022_program_id().to_string());
        assert_eq!(json["executable"], false);
        assert_eq!(json["space"], Token2022Account::LEN);
        // Token account data (nested under "data")
        assert_eq!(json["data"]["token_owner"], token_owner.to_string());
        assert_eq!(json["data"]["amount"], "750000");
        assert_eq!(json["data"]["state"], "Initialized");
    }

    #[test]
    fn test_decode_token2022_mint_confidential_transfer() {
        use solana_pubkey::Pubkey as SolanaPubkey;
        use spl_pod::optional_keys::OptionalNonZeroPubkey;
        use spl_token_2022::extension::{BaseStateWithExtensionsMut, StateWithExtensionsMut};

        let ct_authority = SolanaPubkey::new_unique();

        // Build a Token-2022 mint that carries a ConfidentialTransferMint extension.
        let account_size = ExtensionType::try_calculate_account_len::<Token2022Mint>(&[
            ExtensionType::ConfidentialTransferMint,
        ])
        .unwrap();
        let mut data = vec![0u8; account_size];
        let mut state =
            StateWithExtensionsMut::<Token2022Mint>::unpack_uninitialized(&mut data).unwrap();

        let ext = state.init_extension::<ConfidentialTransferMint>(true).unwrap();
        ext.authority = OptionalNonZeroPubkey::try_from(Some(ct_authority)).unwrap();
        ext.auto_approve_new_accounts = true.into();
        // auditor_elgamal_pubkey left at its all-zero default (encodes None).

        state.base = Token2022Mint {
            mint_authority: COption::None,
            supply: 0,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        state.pack_base();
        state.init_account_type().unwrap();

        let account = solana_account::Account {
            lamports: 5_000_000,
            data,
            owner: token2022_program_id(),
            executable: false,
            rent_epoch: 0,
        };

        let json = decode_spl_token_account(&account).unwrap();
        let ext = json["data"]["extensions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["type"] == "ConfidentialTransferMint")
            .expect("ConfidentialTransferMint extension should be decoded");

        assert_eq!(ext["data"]["authority"], ct_authority.to_string());
        assert_eq!(ext["data"]["auto_approve_new_accounts"], true);
        assert!(ext["data"]["auditor_elgamal_pubkey"].is_null());
    }

    #[test]
    fn test_decode_token2022_mint_confidential_fee_and_mint_burn() {
        use solana_pubkey::Pubkey as SolanaPubkey;
        use spl_pod::optional_keys::OptionalNonZeroPubkey;
        use spl_token_2022::extension::{BaseStateWithExtensionsMut, StateWithExtensionsMut};

        let fee_authority = SolanaPubkey::new_unique();

        let account_size = ExtensionType::try_calculate_account_len::<Token2022Mint>(&[
            ExtensionType::ConfidentialTransferFeeConfig,
            ExtensionType::ConfidentialMintBurn,
        ])
        .unwrap();
        let mut data = vec![0u8; account_size];
        let mut state =
            StateWithExtensionsMut::<Token2022Mint>::unpack_uninitialized(&mut data).unwrap();

        let fee = state
            .init_extension::<ConfidentialTransferFeeConfig>(true)
            .unwrap();
        fee.authority = OptionalNonZeroPubkey::try_from(Some(fee_authority)).unwrap();
        fee.harvest_to_mint_enabled = true.into();

        state.init_extension::<ConfidentialMintBurn>(true).unwrap();

        state.base = Token2022Mint {
            mint_authority: COption::None,
            supply: 0,
            decimals: 9,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        state.pack_base();
        state.init_account_type().unwrap();

        let account = solana_account::Account {
            lamports: 5_000_000,
            data,
            owner: token2022_program_id(),
            executable: false,
            rent_epoch: 0,
        };

        let json = decode_spl_token_account(&account).unwrap();
        let exts = json["data"]["extensions"].as_array().unwrap();

        let fee = exts
            .iter()
            .find(|e| e["type"] == "ConfidentialTransferFeeConfig")
            .expect("ConfidentialTransferFeeConfig should be decoded");
        assert_eq!(fee["data"]["authority"], fee_authority.to_string());
        assert_eq!(fee["data"]["harvest_to_mint_enabled"], true);
        // Ciphertext fields render as base64 strings (all-zero default here).
        assert!(fee["data"]["withdraw_withheld_authority_elgamal_pubkey"].is_string());
        assert!(fee["data"]["withheld_amount"].is_string());

        let burn = exts
            .iter()
            .find(|e| e["type"] == "ConfidentialMintBurn")
            .expect("ConfidentialMintBurn should be decoded");
        assert!(burn["data"]["confidential_supply"].is_string());
        assert!(burn["data"]["decryptable_supply"].is_string());
        assert!(burn["data"]["supply_elgamal_pubkey"].is_string());
        assert!(burn["data"]["pending_burn"].is_string());
    }

    #[test]
    fn test_decode_token2022_account_confidential_transfer() {
        use spl_token_2022::extension::{BaseStateWithExtensionsMut, StateWithExtensionsMut};

        let mint = ProgramPubkey::new_unique();
        let token_owner = ProgramPubkey::new_unique();

        let account_size = ExtensionType::try_calculate_account_len::<Token2022Account>(&[
            ExtensionType::ConfidentialTransferAccount,
            ExtensionType::ConfidentialTransferFeeAmount,
        ])
        .unwrap();
        let mut data = vec![0u8; account_size];
        let mut state =
            StateWithExtensionsMut::<Token2022Account>::unpack_uninitialized(&mut data).unwrap();

        let ct = state
            .init_extension::<ConfidentialTransferAccount>(true)
            .unwrap();
        ct.approved = true.into();
        ct.allow_confidential_credits = true.into();
        ct.allow_non_confidential_credits = false.into();
        ct.pending_balance_credit_counter = 3u64.into();
        ct.maximum_pending_balance_credit_counter = 65_536u64.into();

        state
            .init_extension::<ConfidentialTransferFeeAmount>(true)
            .unwrap();

        state.base = Token2022Account {
            mint,
            owner: token_owner,
            amount: 0,
            delegate: COption::None,
            state: Token2022AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        };
        state.pack_base();
        state.init_account_type().unwrap();

        let account = solana_account::Account {
            lamports: 6_000_000,
            data,
            owner: token2022_program_id(),
            executable: false,
            rent_epoch: 0,
        };

        let json = decode_spl_token_account(&account).unwrap();
        let exts = json["data"]["extensions"].as_array().unwrap();

        let ct = exts
            .iter()
            .find(|e| e["type"] == "ConfidentialTransferAccount")
            .expect("ConfidentialTransferAccount should be decoded");
        assert_eq!(ct["data"]["approved"], true);
        assert_eq!(ct["data"]["allow_confidential_credits"], true);
        assert_eq!(ct["data"]["allow_non_confidential_credits"], false);
        assert_eq!(ct["data"]["pending_balance_credit_counter"], "3");
        assert_eq!(ct["data"]["maximum_pending_balance_credit_counter"], "65536");
        assert!(ct["data"]["elgamal_pubkey"].is_string());
        assert!(ct["data"]["available_balance"].is_string());
        assert!(ct["data"]["decryptable_available_balance"].is_string());

        let fee = exts
            .iter()
            .find(|e| e["type"] == "ConfidentialTransferFeeAmount")
            .expect("ConfidentialTransferFeeAmount should be decoded");
        assert!(fee["data"]["withheld_amount"].is_string());
    }
}
