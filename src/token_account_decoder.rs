//! Token and Token-2022 account decoder.
//!
//! This module provides functionality to decode SPL Token and Token-2022
//! mint and token account data, including Token-2022 extensions.

use serde_json::{Value, json};
use solana_pubkey::Pubkey;
use spl_token::solana_program::program_option::COption;
use spl_token::solana_program::program_pack::Pack;

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

    #[allow(dead_code)]
    fn program_name(&self) -> &'static str {
        match self {
            TokenProgramKind::Legacy => "token",
            TokenProgramKind::Token2022 => "token_2022",
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
    use spl_token::state::{Account as TokenAccount, Mint};

    let data = &account.data;

    // Try to decode as Mint first (82 bytes)
    if data.len() == Mint::LEN {
        if let Ok(mint) = Mint::unpack(data) {
            return Some(build_legacy_mint_json(account, &mint));
        }
    }

    // Try to decode as Token Account (165 bytes)
    if data.len() == TokenAccount::LEN {
        if let Ok(token_account) = TokenAccount::unpack(data) {
            return Some(build_legacy_account_json(account, &token_account));
        }
    }

    // If exact length doesn't match, try both
    if data.len() >= Mint::LEN {
        if let Ok(mint) = Mint::unpack(&data[..Mint::LEN]) {
            // Check if it looks like an initialized mint
            if mint.is_initialized {
                return Some(build_legacy_mint_json(account, &mint));
            }
        }
    }

    if data.len() >= TokenAccount::LEN {
        if let Ok(token_account) = TokenAccount::unpack(&data[..TokenAccount::LEN]) {
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
    json!({
        "lamports": account.lamports,
        "owner": account.owner.to_string(),
        "executable": account.executable,
        "rentEpoch": account.rent_epoch,
        "space": account.data.len(),
        "decimals": mint.decimals,
        "supply": mint.supply.to_string(),
        "mint_authority": coption_pubkey_to_json(&mint.mint_authority),
        "freeze_authority": coption_pubkey_to_json(&mint.freeze_authority),
        "is_initialized": mint.is_initialized
    })
}

/// Build JSON for legacy Token Account
fn build_legacy_account_json(
    account: &solana_account::Account,
    token_account: &spl_token::state::Account,
) -> Value {
    json!({
        "lamports": account.lamports,
        "owner": account.owner.to_string(),
        "executable": account.executable,
        "rentEpoch": account.rent_epoch,
        "space": account.data.len(),
        "mint": token_account.mint.to_string(),
        "token_owner": token_account.owner.to_string(),
        "amount": token_account.amount.to_string(),
        "delegate": coption_pubkey_to_json(&token_account.delegate),
        "state": format!("{:?}", token_account.state),
        "is_native": coption_u64_to_json(&token_account.is_native),
        "delegated_amount": token_account.delegated_amount.to_string(),
        "close_authority": coption_pubkey_to_json(&token_account.close_authority)
    })
}

/// Decode Token-2022 account (mint or token account with extensions)
fn decode_token2022_account(account: &solana_account::Account) -> Option<Value> {
    use spl_token_2022::extension::StateWithExtensions;
    use spl_token_2022::state::{Account as Token2022Account, Mint as Token2022Mint};

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
    use spl_token_2022::extension::BaseStateWithExtensions;

    let mint = &state.base;

    let mut result = json!({
        "lamports": account.lamports,
        "owner": account.owner.to_string(),
        "executable": account.executable,
        "rentEpoch": account.rent_epoch,
        "space": account.data.len(),
        "decimals": mint.decimals,
        "supply": mint.supply.to_string(),
        "mint_authority": coption_pubkey_to_json(&mint.mint_authority),
        "freeze_authority": coption_pubkey_to_json(&mint.freeze_authority),
        "is_initialized": mint.is_initialized
    });

    // Parse extensions
    if let Ok(extension_types) = state.get_extension_types() {
        if !extension_types.is_empty() {
            let extensions = parse_mint_extensions(state, &extension_types);
            result["extensions"] = Value::Array(extensions);
        }
    }

    result
}

/// Build JSON for Token-2022 Token Account with extensions
fn build_token2022_account_json(
    account: &solana_account::Account,
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Account>,
) -> Value {
    use spl_token_2022::extension::BaseStateWithExtensions;

    let token_account = &state.base;

    let mut result = json!({
        "lamports": account.lamports,
        "owner": account.owner.to_string(),
        "executable": account.executable,
        "rentEpoch": account.rent_epoch,
        "space": account.data.len(),
        "mint": token_account.mint.to_string(),
        "token_owner": token_account.owner.to_string(),
        "amount": token_account.amount.to_string(),
        "delegate": coption_pubkey_to_json(&token_account.delegate),
        "state": format!("{:?}", token_account.state),
        "is_native": coption_u64_to_json(&token_account.is_native),
        "delegated_amount": token_account.delegated_amount.to_string(),
        "close_authority": coption_pubkey_to_json(&token_account.close_authority)
    });

    // Parse extensions
    if let Ok(extension_types) = state.get_extension_types() {
        if !extension_types.is_empty() {
            let extensions = parse_account_extensions(state, &extension_types);
            result["extensions"] = Value::Array(extensions);
        }
    }

    result
}

/// Parse mint extensions
fn parse_mint_extensions(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
    extension_types: &[spl_token_2022::extension::ExtensionType],
) -> Vec<Value> {
    use spl_token_2022::extension::ExtensionType;

    let mut extensions = Vec::new();

    for ext_type in extension_types {
        let ext_json = match ext_type {
            ExtensionType::TransferFeeConfig => parse_transfer_fee_config_extension(state),
            ExtensionType::MintCloseAuthority => parse_mint_close_authority_extension(state),
            ExtensionType::InterestBearingConfig => parse_interest_bearing_config_extension(state),
            ExtensionType::PermanentDelegate => parse_permanent_delegate_extension(state),
            ExtensionType::NonTransferable => {
                json!({
                    "type": "NonTransferable",
                    "data": {}
                })
            }
            ExtensionType::DefaultAccountState => parse_default_account_state_extension(state),
            ExtensionType::MetadataPointer => parse_metadata_pointer_extension(state),
            ExtensionType::GroupPointer => parse_group_pointer_extension(state),
            ExtensionType::GroupMemberPointer => parse_group_member_pointer_extension(state),
            ExtensionType::TransferHook => parse_transfer_hook_extension(state),
            ExtensionType::TokenMetadata => parse_token_metadata_extension(state),
            ExtensionType::Pausable => parse_pausable_config_extension(state),
            ExtensionType::ScaledUiAmount => parse_scaled_ui_amount_extension(state),
            ExtensionType::ConfidentialTransferMint => {
                // Complex extension, output raw bytes
                get_extension_raw_or_empty::<spl_token_2022::state::Mint>(
                    state,
                    "ConfidentialTransferMint",
                )
            }
            ExtensionType::ConfidentialTransferFeeConfig => {
                get_extension_raw_or_empty::<spl_token_2022::state::Mint>(
                    state,
                    "ConfidentialTransferFeeConfig",
                )
            }
            ExtensionType::ConfidentialMintBurn => get_extension_raw_or_empty::<
                spl_token_2022::state::Mint,
            >(state, "ConfidentialMintBurn"),
            ExtensionType::TokenGroup => parse_token_group_extension(state),
            ExtensionType::TokenGroupMember => parse_token_group_member_extension(state),
            _ => {
                // Unknown or unhandled extension, output type name only
                json!({
                    "type": format!("{:?}", ext_type),
                    "data": null
                })
            }
        };
        extensions.push(ext_json);
    }

    extensions
}

/// Parse account extensions
fn parse_account_extensions(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Account>,
    extension_types: &[spl_token_2022::extension::ExtensionType],
) -> Vec<Value> {
    use spl_token_2022::extension::ExtensionType;

    let mut extensions = Vec::new();

    for ext_type in extension_types {
        let ext_json = match ext_type {
            ExtensionType::TransferFeeAmount => parse_transfer_fee_amount_extension(state),
            ExtensionType::ImmutableOwner => {
                json!({
                    "type": "ImmutableOwner",
                    "data": {}
                })
            }
            ExtensionType::MemoTransfer => parse_memo_transfer_extension(state),
            ExtensionType::CpiGuard => parse_cpi_guard_extension(state),
            ExtensionType::NonTransferableAccount => {
                json!({
                    "type": "NonTransferableAccount",
                    "data": {}
                })
            }
            ExtensionType::TransferHookAccount => parse_transfer_hook_account_extension(state),
            ExtensionType::PausableAccount => {
                json!({
                    "type": "PausableAccount",
                    "data": {}
                })
            }
            ExtensionType::ConfidentialTransferAccount => {
                // Complex extension, output raw hex
                get_account_extension_raw_or_empty(state, "ConfidentialTransferAccount")
            }
            ExtensionType::ConfidentialTransferFeeAmount => {
                get_account_extension_raw_or_empty(state, "ConfidentialTransferFeeAmount")
            }
            _ => {
                json!({
                    "type": format!("{:?}", ext_type),
                    "data": null
                })
            }
        };
        extensions.push(ext_json);
    }

    extensions
}

// ============================================================================
// Extension parsing helpers
// ============================================================================

fn parse_transfer_fee_config_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
) -> Value {
    use spl_token_2022::extension::{BaseStateWithExtensions, transfer_fee::TransferFeeConfig};

    match state.get_extension::<TransferFeeConfig>() {
        Ok(config) => {
            json!({
                "type": "TransferFeeConfig",
                "data": {
                    "transfer_fee_config_authority": pod_option_pubkey_to_string(&config.transfer_fee_config_authority),
                    "withdraw_withheld_authority": pod_option_pubkey_to_string(&config.withdraw_withheld_authority),
                    "withheld_amount": u64::from(config.withheld_amount).to_string(),
                    "older_transfer_fee": {
                        "epoch": u64::from(config.older_transfer_fee.epoch).to_string(),
                        "maximum_fee": u64::from(config.older_transfer_fee.maximum_fee).to_string(),
                        "transfer_fee_basis_points": u16::from(config.older_transfer_fee.transfer_fee_basis_points)
                    },
                    "newer_transfer_fee": {
                        "epoch": u64::from(config.newer_transfer_fee.epoch).to_string(),
                        "maximum_fee": u64::from(config.newer_transfer_fee.maximum_fee).to_string(),
                        "transfer_fee_basis_points": u16::from(config.newer_transfer_fee.transfer_fee_basis_points)
                    }
                }
            })
        }
        Err(_) => json!({
            "type": "TransferFeeConfig",
            "data": null
        }),
    }
}

fn parse_transfer_fee_amount_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Account>,
) -> Value {
    use spl_token_2022::extension::{BaseStateWithExtensions, transfer_fee::TransferFeeAmount};

    match state.get_extension::<TransferFeeAmount>() {
        Ok(amount) => {
            json!({
                "type": "TransferFeeAmount",
                "data": {
                    "withheld_amount": u64::from(amount.withheld_amount).to_string()
                }
            })
        }
        Err(_) => json!({
            "type": "TransferFeeAmount",
            "data": null
        }),
    }
}

fn parse_mint_close_authority_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
) -> Value {
    use spl_token_2022::extension::{
        BaseStateWithExtensions, mint_close_authority::MintCloseAuthority,
    };

    match state.get_extension::<MintCloseAuthority>() {
        Ok(ext) => {
            json!({
                "type": "MintCloseAuthority",
                "data": {
                    "close_authority": pod_option_pubkey_to_string(&ext.close_authority)
                }
            })
        }
        Err(_) => json!({
            "type": "MintCloseAuthority",
            "data": null
        }),
    }
}

fn parse_interest_bearing_config_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
) -> Value {
    use spl_token_2022::extension::{
        BaseStateWithExtensions, interest_bearing_mint::InterestBearingConfig,
    };

    match state.get_extension::<InterestBearingConfig>() {
        Ok(config) => {
            json!({
                "type": "InterestBearingConfig",
                "data": {
                    "rate_authority": pod_option_pubkey_to_string(&config.rate_authority),
                    "initialization_timestamp": i64::from(config.initialization_timestamp).to_string(),
                    "pre_update_average_rate": i16::from(config.pre_update_average_rate),
                    "last_update_timestamp": i64::from(config.last_update_timestamp).to_string(),
                    "current_rate": i16::from(config.current_rate)
                }
            })
        }
        Err(_) => json!({
            "type": "InterestBearingConfig",
            "data": null
        }),
    }
}

fn parse_permanent_delegate_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
) -> Value {
    use spl_token_2022::extension::{
        BaseStateWithExtensions, permanent_delegate::PermanentDelegate,
    };

    match state.get_extension::<PermanentDelegate>() {
        Ok(ext) => {
            json!({
                "type": "PermanentDelegate",
                "data": {
                    "delegate": pod_option_pubkey_to_string(&ext.delegate)
                }
            })
        }
        Err(_) => json!({
            "type": "PermanentDelegate",
            "data": null
        }),
    }
}

fn parse_default_account_state_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
) -> Value {
    use spl_token_2022::extension::{
        BaseStateWithExtensions, default_account_state::DefaultAccountState,
    };

    match state.get_extension::<DefaultAccountState>() {
        Ok(ext) => {
            let state_value: u8 = ext.state.into();
            let state_str = match state_value {
                0 => "Uninitialized",
                1 => "Initialized",
                2 => "Frozen",
                _ => "Unknown",
            };
            json!({
                "type": "DefaultAccountState",
                "data": {
                    "state": state_str
                }
            })
        }
        Err(_) => json!({
            "type": "DefaultAccountState",
            "data": null
        }),
    }
}

fn parse_metadata_pointer_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
) -> Value {
    use spl_token_2022::extension::{BaseStateWithExtensions, metadata_pointer::MetadataPointer};

    match state.get_extension::<MetadataPointer>() {
        Ok(ext) => {
            json!({
                "type": "MetadataPointer",
                "data": {
                    "authority": pod_option_pubkey_to_string(&ext.authority),
                    "metadata_address": pod_option_pubkey_to_string(&ext.metadata_address)
                }
            })
        }
        Err(_) => json!({
            "type": "MetadataPointer",
            "data": null
        }),
    }
}

fn parse_group_pointer_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
) -> Value {
    use spl_token_2022::extension::{BaseStateWithExtensions, group_pointer::GroupPointer};

    match state.get_extension::<GroupPointer>() {
        Ok(ext) => {
            json!({
                "type": "GroupPointer",
                "data": {
                    "authority": pod_option_pubkey_to_string(&ext.authority),
                    "group_address": pod_option_pubkey_to_string(&ext.group_address)
                }
            })
        }
        Err(_) => json!({
            "type": "GroupPointer",
            "data": null
        }),
    }
}

fn parse_group_member_pointer_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
) -> Value {
    use spl_token_2022::extension::{
        BaseStateWithExtensions, group_member_pointer::GroupMemberPointer,
    };

    match state.get_extension::<GroupMemberPointer>() {
        Ok(ext) => {
            json!({
                "type": "GroupMemberPointer",
                "data": {
                    "authority": pod_option_pubkey_to_string(&ext.authority),
                    "member_address": pod_option_pubkey_to_string(&ext.member_address)
                }
            })
        }
        Err(_) => json!({
            "type": "GroupMemberPointer",
            "data": null
        }),
    }
}

fn parse_transfer_hook_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
) -> Value {
    use spl_token_2022::extension::{BaseStateWithExtensions, transfer_hook::TransferHook};

    match state.get_extension::<TransferHook>() {
        Ok(ext) => {
            json!({
                "type": "TransferHook",
                "data": {
                    "authority": pod_option_pubkey_to_string(&ext.authority),
                    "program_id": pod_option_pubkey_to_string(&ext.program_id)
                }
            })
        }
        Err(_) => json!({
            "type": "TransferHook",
            "data": null
        }),
    }
}

fn parse_transfer_hook_account_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Account>,
) -> Value {
    use spl_token_2022::extension::{BaseStateWithExtensions, transfer_hook::TransferHookAccount};

    match state.get_extension::<TransferHookAccount>() {
        Ok(ext) => {
            let transferring: bool = ext.transferring.into();
            json!({
                "type": "TransferHookAccount",
                "data": {
                    "transferring": transferring
                }
            })
        }
        Err(_) => json!({
            "type": "TransferHookAccount",
            "data": null
        }),
    }
}

fn parse_memo_transfer_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Account>,
) -> Value {
    use spl_token_2022::extension::{BaseStateWithExtensions, memo_transfer::MemoTransfer};

    match state.get_extension::<MemoTransfer>() {
        Ok(ext) => {
            let require_incoming_transfer_memos: bool = ext.require_incoming_transfer_memos.into();
            json!({
                "type": "MemoTransfer",
                "data": {
                    "require_incoming_transfer_memos": require_incoming_transfer_memos
                }
            })
        }
        Err(_) => json!({
            "type": "MemoTransfer",
            "data": null
        }),
    }
}

fn parse_cpi_guard_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Account>,
) -> Value {
    use spl_token_2022::extension::{BaseStateWithExtensions, cpi_guard::CpiGuard};

    match state.get_extension::<CpiGuard>() {
        Ok(ext) => {
            let lock_cpi: bool = ext.lock_cpi.into();
            json!({
                "type": "CpiGuard",
                "data": {
                    "lock_cpi": lock_cpi
                }
            })
        }
        Err(_) => json!({
            "type": "CpiGuard",
            "data": null
        }),
    }
}

fn parse_token_metadata_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
) -> Value {
    use spl_token_2022::extension::BaseStateWithExtensions;
    use spl_token_metadata_interface::state::TokenMetadata;

    match state.get_variable_len_extension::<TokenMetadata>() {
        Ok(metadata) => {
            let additional_metadata: Vec<Value> = metadata
                .additional_metadata
                .iter()
                .map(|(k, v)| json!({"key": k, "value": v}))
                .collect();

            json!({
                "type": "TokenMetadata",
                "data": {
                    "update_authority": pod_option_pubkey_to_string(&metadata.update_authority),
                    "mint": metadata.mint.to_string(),
                    "name": metadata.name,
                    "symbol": metadata.symbol,
                    "uri": metadata.uri,
                    "additional_metadata": additional_metadata
                }
            })
        }
        Err(_) => json!({
            "type": "TokenMetadata",
            "data": null
        }),
    }
}

fn parse_token_group_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
) -> Value {
    use spl_token_2022::extension::BaseStateWithExtensions;
    use spl_token_group_interface::state::TokenGroup;

    match state.get_extension::<TokenGroup>() {
        Ok(group) => {
            json!({
                "type": "TokenGroup",
                "data": {
                    "update_authority": pod_option_pubkey_to_string(&group.update_authority),
                    "mint": group.mint.to_string(),
                    "size": u64::from(group.size).to_string(),
                    "max_size": u64::from(group.max_size).to_string()
                }
            })
        }
        Err(_) => json!({
            "type": "TokenGroup",
            "data": null
        }),
    }
}

fn parse_token_group_member_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
) -> Value {
    use spl_token_2022::extension::BaseStateWithExtensions;
    use spl_token_group_interface::state::TokenGroupMember;

    match state.get_extension::<TokenGroupMember>() {
        Ok(member) => {
            json!({
                "type": "TokenGroupMember",
                "data": {
                    "mint": member.mint.to_string(),
                    "group": member.group.to_string(),
                    "member_number": u64::from(member.member_number).to_string()
                }
            })
        }
        Err(_) => json!({
            "type": "TokenGroupMember",
            "data": null
        }),
    }
}

fn parse_pausable_config_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
) -> Value {
    use spl_token_2022::extension::{BaseStateWithExtensions, pausable::PausableConfig};

    match state.get_extension::<PausableConfig>() {
        Ok(config) => {
            let paused: bool = config.paused.into();
            json!({
                "type": "Pausable",
                "data": {
                    "authority": pod_option_pubkey_to_string(&config.authority),
                    "paused": paused
                }
            })
        }
        Err(_) => json!({
            "type": "Pausable",
            "data": null
        }),
    }
}

fn parse_scaled_ui_amount_extension(
    state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Mint>,
) -> Value {
    use spl_token_2022::extension::{
        BaseStateWithExtensions, scaled_ui_amount::ScaledUiAmountConfig,
    };

    match state.get_extension::<ScaledUiAmountConfig>() {
        Ok(config) => {
            json!({
                "type": "ScaledUiAmount",
                "data": {
                    "authority": pod_option_pubkey_to_string(&config.authority),
                    "multiplier": f64::from(config.multiplier),
                    "new_multiplier_effective_timestamp": i64::from(config.new_multiplier_effective_timestamp).to_string(),
                    "new_multiplier": f64::from(config.new_multiplier)
                }
            })
        }
        Err(_) => json!({
            "type": "ScaledUiAmount",
            "data": null
        }),
    }
}

fn get_extension_raw_or_empty<
    S: spl_token_2022::extension::BaseState + spl_token::solana_program::program_pack::Pack,
>(
    _state: &spl_token_2022::extension::StateWithExtensions<S>,
    type_name: &str,
) -> Value {
    // For complex extensions, we just output the type name
    // Full raw bytes parsing would require more complex handling
    json!({
        "type": type_name,
        "data": null
    })
}

fn get_account_extension_raw_or_empty(
    _state: &spl_token_2022::extension::StateWithExtensions<spl_token_2022::state::Account>,
    type_name: &str,
) -> Value {
    json!({
        "type": type_name,
        "data": null
    })
}

// ============================================================================
// Helper functions for converting COption and Pod types to JSON
// ============================================================================

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

#[cfg(test)]
mod tests {
    use super::*;
    use spl_token::solana_program::program_pack::Pack;

    #[test]
    fn test_decode_legacy_mint() {
        use spl_token::solana_program::program_option::COption;
        use spl_token::solana_program::pubkey::Pubkey as ProgramPubkey;
        use spl_token::state::Mint;

        let mint_authority = ProgramPubkey::new_unique();
        let mint = Mint {
            mint_authority: COption::Some(mint_authority),
            supply: 1_000_000_000,
            decimals: 9,
            is_initialized: true,
            freeze_authority: COption::None,
        };

        let mut data = vec![0u8; Mint::LEN];
        Mint::pack(mint, &mut data).unwrap();

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
        // Account metadata
        assert_eq!(json["lamports"], 1_000_000);
        assert_eq!(json["owner"], legacy_program_id().to_string());
        assert_eq!(json["executable"], false);
        assert_eq!(json["rentEpoch"], 0);
        assert_eq!(json["space"], Mint::LEN);
        // Mint data
        assert_eq!(json["decimals"], 9);
        assert_eq!(json["supply"], "1000000000");
        assert!(json["is_initialized"].as_bool().unwrap());
    }

    #[test]
    fn test_decode_legacy_token_account() {
        use spl_token::solana_program::program_option::COption;
        use spl_token::solana_program::pubkey::Pubkey as ProgramPubkey;
        use spl_token::state::{Account as TokenAccount, AccountState};

        let mint = ProgramPubkey::new_unique();
        let token_owner = ProgramPubkey::new_unique();
        let token_account = TokenAccount {
            mint,
            owner: token_owner,
            amount: 500_000,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        };

        let mut data = vec![0u8; TokenAccount::LEN];
        TokenAccount::pack(token_account, &mut data).unwrap();

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
        // Account metadata
        assert_eq!(json["lamports"], 2_000_000);
        assert_eq!(json["owner"], legacy_program_id().to_string());
        assert_eq!(json["executable"], false);
        assert_eq!(json["space"], TokenAccount::LEN);
        // Token account data
        assert_eq!(json["token_owner"], token_owner.to_string());
        assert_eq!(json["amount"], "500000");
        assert_eq!(json["state"], "Initialized");
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
        use spl_token::solana_program::program_option::COption;
        use spl_token::solana_program::pubkey::Pubkey as ProgramPubkey;
        use spl_token_2022::state::Mint;

        let mint_authority = ProgramPubkey::new_unique();
        let mint = Mint {
            mint_authority: COption::Some(mint_authority),
            supply: 2_000_000_000,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        };

        let mut data = vec![0u8; Mint::LEN];
        Mint::pack(mint, &mut data).unwrap();

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
        // Account metadata
        assert_eq!(json["lamports"], 3_000_000);
        assert_eq!(json["owner"], token2022_program_id().to_string());
        assert_eq!(json["executable"], false);
        assert_eq!(json["space"], Mint::LEN);
        // Mint data
        assert_eq!(json["decimals"], 6);
        assert_eq!(json["supply"], "2000000000");
        assert!(json["is_initialized"].as_bool().unwrap());
    }

    #[test]
    fn test_decode_token2022_token_account() {
        use spl_token::solana_program::program_option::COption;
        use spl_token::solana_program::pubkey::Pubkey as ProgramPubkey;
        use spl_token_2022::state::{Account as TokenAccount, AccountState};

        let mint = ProgramPubkey::new_unique();
        let token_owner = ProgramPubkey::new_unique();
        let token_account = TokenAccount {
            mint,
            owner: token_owner,
            amount: 750_000,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        };

        let mut data = vec![0u8; TokenAccount::LEN];
        TokenAccount::pack(token_account, &mut data).unwrap();

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
        // Account metadata
        assert_eq!(json["lamports"], 4_000_000);
        assert_eq!(json["owner"], token2022_program_id().to_string());
        assert_eq!(json["executable"], false);
        assert_eq!(json["space"], TokenAccount::LEN);
        // Token account data
        assert_eq!(json["token_owner"], token_owner.to_string());
        assert_eq!(json["amount"], "750000");
        assert_eq!(json["state"], "Initialized");
    }
}
