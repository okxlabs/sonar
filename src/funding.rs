use anyhow::{Context, Result, anyhow};
use litesvm::LiteSVM;
use log::info;
use solana_account::{Account, AccountSharedData, WritableAccount};
use solana_pubkey::Pubkey;
use spl_token::solana_program::program_pack::Pack;
use spl_token_2022::extension::{BaseStateWithExtensions, BaseStateWithExtensionsMut};

use crate::{
    account_loader::{AccountLoader, ResolvedAccounts},
    cli::{Funding, TokenAmount, TokenFunding},
    progress::Progress,
};

const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

#[derive(Clone, Debug)]
pub struct PreparedTokenFunding {
    pub account: Pubkey,
    pub mint: Pubkey,
    pub decimals: u8,
    pub amount_raw: u64,
    pub ui_amount: f64,
}

pub fn apply_sol_fundings(svm: &mut LiteSVM, fundings: &[Funding]) -> Result<()> {
    for funding in fundings {
        apply_single_sol_funding(svm, funding)?;
    }
    Ok(())
}

fn apply_single_sol_funding(svm: &mut LiteSVM, funding: &Funding) -> Result<()> {
    let lamports = funding.amount_lamports;
    let sol = lamports as f64 / LAMPORTS_PER_SOL as f64;
    info!("Funding account {} with {} lamports ({:.9} SOL)", funding.pubkey, lamports, sol);

    if let Some(existing_account) = svm.get_account(&funding.pubkey) {
        let mut updated = existing_account.clone();
        updated.set_lamports(lamports);
        svm.set_account(funding.pubkey, updated)?;
    } else {
        let system_program_id = solana_sdk_ids::system_program::id();
        let new_account = AccountSharedData::new(lamports, 0, &system_program_id);
        svm.set_account(funding.pubkey, new_account.into())?;
    }

    Ok(())
}

pub fn prepare_token_fundings(
    loader: &AccountLoader,
    resolved: &mut ResolvedAccounts,
    requests: &[TokenFunding],
    progress: Option<&Progress>,
) -> Result<Vec<PreparedTokenFunding>> {
    let mut prepared = Vec::new();
    if requests.is_empty() {
        return Ok(prepared);
    }

    let total = requests.len();
    for (index, request) in requests.iter().enumerate() {
        if let Some(progress) = progress {
            progress.set_message(format!("Preparing token fundings... ({}/{})", index + 1, total));
        }
        let summary = process_single(loader, resolved, request)
            .with_context(|| format!("Failed to prepare token funding for {}", request.account))?;
        prepared.push(summary);
    }

    Ok(prepared)
}

fn process_single(
    loader: &AccountLoader,
    resolved: &mut ResolvedAccounts,
    request: &TokenFunding,
) -> Result<PreparedTokenFunding> {
    // 1. Load the token account first (may or may not exist on-chain)
    ensure_account_loaded(loader, resolved, &request.account).with_context(|| {
        format!("Failed to load token account {} required for funding", request.account)
    })?;

    // 2. Determine the mint: auto-detect from on-chain data or use CLI-provided value
    let mint = if let Some(account) = resolved.accounts.get(&request.account) {
        // Token account exists — read mint from its data
        let detected_mint = detect_mint_from_token_account(account).with_context(|| {
            format!("Failed to detect mint from existing token account {}", request.account)
        })?;
        if let Some(requested_mint) = request.mint {
            if detected_mint != requested_mint {
                return Err(anyhow!(
                    "Token account {} is associated with mint {}, but CLI requested mint {}",
                    request.account,
                    detected_mint,
                    requested_mint
                ));
            }
        }
        detected_mint
    } else {
        // Token account does not exist — mint must be specified
        request.mint.ok_or_else(|| {
            anyhow!(
                "Token account {} does not exist on-chain; \
                 you must specify the mint using <ACCOUNT>:<MINT>=<AMOUNT> format",
                request.account
            )
        })?
    };

    // 3. Load and validate the mint account
    ensure_account_loaded(loader, resolved, &mint)
        .with_context(|| format!("Failed to load mint account {}", mint))?;
    let mint_account = resolved
        .accounts
        .get(&mint)
        .ok_or_else(|| anyhow!("Mint account {} missing after load", mint))?
        .clone();

    let program_kind = TokenProgramKind::from_owner(&mint_account.owner).ok_or_else(|| {
        anyhow!(
            "Mint account {} is not owned by the SPL Token programs; cannot prepare funding",
            mint
        )
    })?;

    let decimals = match program_kind {
        TokenProgramKind::Legacy => read_legacy_mint_decimals(&mint_account)?,
        TokenProgramKind::Token2022 => read_token2022_mint_decimals(&mint_account)?,
    };

    // 3b. Resolve the token amount to raw u64 using mint decimals
    let amount_raw = resolve_token_amount(&request.amount, decimals)
        .with_context(|| format!("Failed to resolve token amount for {}", request.account))?;

    // 4. Create the token account if it does not exist
    if !resolved.accounts.contains_key(&request.account) {
        create_missing_token_account(
            resolved,
            &request.account,
            &mint,
            program_kind,
            &mint_account,
        )?;
    }

    // 5. Update the token account amount
    match program_kind {
        TokenProgramKind::Legacy => {
            update_legacy_account(resolved, &request.account, &mint, amount_raw, decimals)
        }
        TokenProgramKind::Token2022 => {
            update_token2022_account(resolved, &request.account, &mint, amount_raw, decimals)
        }
    }
}

/// Detect the mint pubkey from an existing token account's data.
/// Both legacy SPL Token and Token-2022 share the same base layout — mint is the first 32 bytes.
fn detect_mint_from_token_account(account: &Account) -> Result<Pubkey> {
    const MINT_OFFSET: usize = 0;
    const MINT_LEN: usize = 32;

    if TokenProgramKind::from_owner(&account.owner).is_none() {
        return Err(anyhow!(
            "Token account is not owned by any known SPL Token program (owner: {})",
            account.owner
        ));
    }
    if account.data.len() < MINT_OFFSET + MINT_LEN {
        return Err(anyhow!(
            "Token account data too small to read mint: {} < {}",
            account.data.len(),
            MINT_OFFSET + MINT_LEN
        ));
    }
    let mint_bytes: [u8; 32] =
        account.data[MINT_OFFSET..MINT_OFFSET + MINT_LEN].try_into().expect("slice length is 32");
    Ok(Pubkey::new_from_array(mint_bytes))
}

fn read_legacy_mint_decimals(account: &solana_account::Account) -> Result<u8> {
    use spl_token::state::Mint as SplMint;

    if account.data.len() < SplMint::LEN {
        return Err(anyhow!(
            "Mint account data is smaller than expected: {} < {}",
            account.data.len(),
            SplMint::LEN
        ));
    }
    let parsed = SplMint::unpack(&account.data[..SplMint::LEN])
        .map_err(|err| anyhow!("Failed to unpack SPL mint account: {err}"))?;
    Ok(parsed.decimals)
}

fn read_token2022_mint_decimals(account: &solana_account::Account) -> Result<u8> {
    use spl_token_2022::state::Mint as Token2022Mint;

    if account.data.len() < Token2022Mint::LEN {
        return Err(anyhow!(
            "Mint account data is smaller than expected: {} < {}",
            account.data.len(),
            Token2022Mint::LEN
        ));
    }
    let parsed = Token2022Mint::unpack(&account.data[..Token2022Mint::LEN])
        .map_err(|err| anyhow!("Failed to unpack token-2022 mint account: {err}"))?;
    Ok(parsed.decimals)
}

fn create_missing_token_account(
    resolved: &mut ResolvedAccounts,
    account_pubkey: &Pubkey,
    mint: &Pubkey,
    kind: TokenProgramKind,
    mint_account: &Account,
) -> Result<()> {
    match kind {
        TokenProgramKind::Legacy => {
            use spl_token::solana_program::{
                program_option::COption, pubkey::Pubkey as ProgramPubkey,
            };
            use spl_token::state::{Account as SplAccount, AccountState};

            let mut data = vec![0u8; SplAccount::LEN];
            let state = SplAccount {
                mint: ProgramPubkey::new_from_array(mint.to_bytes()),
                owner: ProgramPubkey::new_from_array(account_pubkey.to_bytes()),
                amount: 0,
                delegate: COption::None,
                state: AccountState::Initialized,
                is_native: COption::None,
                delegated_amount: 0,
                close_authority: COption::None,
            };
            SplAccount::pack(state, &mut data)
                .map_err(|err| anyhow!("Failed to pack new SPL token account: {err}"))?;
            resolved.accounts.insert(
                *account_pubkey,
                Account {
                    lamports: 0,
                    data,
                    owner: legacy_program_id(),
                    executable: false,
                    rent_epoch: 0,
                },
            );
        }
        TokenProgramKind::Token2022 => {
            create_token2022_account_with_extensions(resolved, account_pubkey, mint, mint_account)?;
        }
    }

    Ok(())
}

/// Creates a new Token-2022 token account, inferring required extensions from the mint.
fn create_token2022_account_with_extensions(
    resolved: &mut ResolvedAccounts,
    account_pubkey: &Pubkey,
    mint: &Pubkey,
    mint_account: &Account,
) -> Result<()> {
    use spl_token::solana_program::{program_option::COption, pubkey::Pubkey as ProgramPubkey};
    use spl_token_2022::extension::{ExtensionType, StateWithExtensions, StateWithExtensionsMut};
    use spl_token_2022::state::{Account as Token2022Account, AccountState, Mint as Token2022Mint};

    let mint_state = StateWithExtensions::<Token2022Mint>::unpack(&mint_account.data)
        .map_err(|e| anyhow!("Failed to unpack token-2022 mint {}: {}", mint, e))?;
    let mint_extension_types = mint_state
        .get_extension_types()
        .map_err(|e| anyhow!("Failed to get mint extension types: {}", e))?;

    let required_extensions =
        ExtensionType::get_required_init_account_extensions(&mint_extension_types);
    let account_len =
        ExtensionType::try_calculate_account_len::<Token2022Account>(&required_extensions)
            .map_err(|e| anyhow!("Failed to calculate token-2022 account length: {}", e))?;

    let mut data = vec![0u8; account_len];
    let mut state = StateWithExtensionsMut::<Token2022Account>::unpack_uninitialized(&mut data)
        .map_err(|e| anyhow!("Failed to unpack uninitialized token-2022 account buffer: {}", e))?;

    state.base = Token2022Account {
        mint: ProgramPubkey::new_from_array(mint.to_bytes()),
        owner: ProgramPubkey::new_from_array(account_pubkey.to_bytes()),
        amount: 0,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    };

    state.init_account_type().map_err(|e| anyhow!("Failed to init account type: {}", e))?;
    state.pack_base();
    for ext_type in &required_extensions {
        state
            .init_account_extension_from_type(*ext_type)
            .map_err(|e| anyhow!("Failed to init extension {:?}: {}", ext_type, e))?;
    }

    resolved.accounts.insert(
        *account_pubkey,
        Account {
            lamports: 0,
            data,
            owner: token2022_program_id(),
            executable: false,
            rent_epoch: 0,
        },
    );
    Ok(())
}

fn update_legacy_account(
    resolved: &mut ResolvedAccounts,
    account_pubkey: &Pubkey,
    mint: &Pubkey,
    amount_raw: u64,
    decimals: u8,
) -> Result<PreparedTokenFunding> {
    use spl_token::state::Account as SplAccount;

    let account = resolved
        .accounts
        .get_mut(account_pubkey)
        .ok_or_else(|| anyhow!("Token account {} missing for mutation", account_pubkey))?;
    ensure_same_program(TokenProgramKind::Legacy, &account.owner, "token account")?;
    if account.data.len() < SplAccount::LEN {
        return Err(anyhow!(
            "Token account data is smaller than expected: {} < {}",
            account.data.len(),
            SplAccount::LEN
        ));
    }
    let (account_bytes, _) = account.data.split_at_mut(SplAccount::LEN);
    let mut parsed = SplAccount::unpack(account_bytes)
        .map_err(|err| anyhow!("Failed to unpack SPL token account: {err}"))?;
    parsed.amount = amount_raw;
    SplAccount::pack(parsed, account_bytes)
        .map_err(|err| anyhow!("Failed to update SPL token account: {err}"))?;

    Ok(PreparedTokenFunding {
        account: *account_pubkey,
        mint: *mint,
        decimals,
        amount_raw,
        ui_amount: raw_to_ui_amount(amount_raw, decimals),
    })
}

fn update_token2022_account(
    resolved: &mut ResolvedAccounts,
    account_pubkey: &Pubkey,
    mint: &Pubkey,
    amount_raw: u64,
    decimals: u8,
) -> Result<PreparedTokenFunding> {
    use spl_token_2022::state::Account as Token2022Account;

    let account = resolved
        .accounts
        .get_mut(account_pubkey)
        .ok_or_else(|| anyhow!("Token account {} missing for mutation", account_pubkey))?;
    ensure_same_program(TokenProgramKind::Token2022, &account.owner, "token account")?;
    if account.data.len() < Token2022Account::LEN {
        return Err(anyhow!(
            "Token account data is smaller than expected: {} < {}",
            account.data.len(),
            Token2022Account::LEN
        ));
    }
    let (account_bytes, _) = account.data.split_at_mut(Token2022Account::LEN);
    let mut parsed = Token2022Account::unpack(account_bytes)
        .map_err(|err| anyhow!("Failed to unpack token-2022 account: {err}"))?;
    parsed.amount = amount_raw;
    Token2022Account::pack(parsed, account_bytes)
        .map_err(|err| anyhow!("Failed to update token-2022 account {}: {err}", account_pubkey))?;

    Ok(PreparedTokenFunding {
        account: *account_pubkey,
        mint: *mint,
        decimals,
        amount_raw,
        ui_amount: raw_to_ui_amount(amount_raw, decimals),
    })
}

fn ensure_account_loaded(
    loader: &AccountLoader,
    resolved: &mut ResolvedAccounts,
    pubkey: &Pubkey,
) -> Result<()> {
    if resolved.accounts.contains_key(pubkey) {
        return Ok(());
    }
    loader.append_accounts(resolved, &[*pubkey])
}

/// Convert a [`TokenAmount`] to a raw `u64` value using the mint's decimals.
///
/// - `TokenAmount::Raw(v)` is returned as-is.
/// - `TokenAmount::Decimal(v)` is multiplied by `10^decimals` and rounded.
fn resolve_token_amount(amount: &TokenAmount, decimals: u8) -> Result<u64> {
    match amount {
        TokenAmount::Raw(raw) => Ok(*raw),
        TokenAmount::Decimal(ui) => {
            let factor = 10u64.pow(decimals as u32);
            let raw_f64 = ui * factor as f64;
            if raw_f64 < 0.0 {
                return Err(anyhow!("Token funding amount must be non-negative"));
            }
            if raw_f64 > u64::MAX as f64 {
                return Err(anyhow!(
                    "Token funding amount {ui} with {decimals} decimals overflows u64"
                ));
            }
            Ok(raw_f64.round() as u64)
        }
    }
}

fn raw_to_ui_amount(amount_raw: u64, decimals: u8) -> f64 {
    let factor = 10f64.powi(decimals as i32);
    if factor == 0.0 { amount_raw as f64 } else { (amount_raw as f64) / factor }
}

fn ensure_same_program(kind: TokenProgramKind, owner: &Pubkey, label: &str) -> Result<()> {
    if owner != &kind.program_id() {
        return Err(anyhow!("Provided {label} is not owned by {}", kind.program_name()));
    }
    Ok(())
}

#[derive(Copy, Clone)]
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

    fn program_id(&self) -> Pubkey {
        match self {
            TokenProgramKind::Legacy => legacy_program_id(),
            TokenProgramKind::Token2022 => token2022_program_id(),
        }
    }

    fn program_name(&self) -> &'static str {
        match self {
            TokenProgramKind::Legacy => "SPL Token",
            TokenProgramKind::Token2022 => "SPL Token 2022",
        }
    }
}

fn legacy_program_id() -> Pubkey {
    Pubkey::new_from_array(spl_token::ID.to_bytes())
}

fn token2022_program_id() -> Pubkey {
    Pubkey::new_from_array(spl_token_2022::ID.to_bytes())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use litesvm::LiteSVM;
    use solana_account::{Account, AccountSharedData, ReadableAccount};
    use solana_pubkey::Pubkey;
    use spl_token::solana_program::program_pack::Pack;
    use spl_token_2022::extension::{
        BaseStateWithExtensions, BaseStateWithExtensionsMut, ExtensionType, StateWithExtensions,
        StateWithExtensionsMut,
    };
    use spl_token_2022::state::{Account as Token2022Account, Mint as Token2022Mint};

    use super::*;

    #[test]
    fn prepares_spl_token_funding_and_updates_account_data() {
        let loader = AccountLoader::new("http://localhost:8899".into(), None, false, None).unwrap();
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let (token_account, mint_account) = spl_token_account_and_mint(&mint, &owner);

        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };
        resolved.accounts.insert(token, token_account);
        resolved.accounts.insert(mint, mint_account);

        let funding =
            TokenFunding { account: token, mint: Some(mint), amount: TokenAmount::Raw(1_500_000) };
        let prepared = prepare_token_fundings(&loader, &mut resolved, &[funding], None)
            .expect("prepares funding");
        assert_eq!(prepared.len(), 1);
        let summary = &prepared[0];
        assert_eq!(summary.mint, mint);
        assert_eq!(summary.decimals, 6);
        assert!((summary.ui_amount - 1.5).abs() < f64::EPSILON);
        assert!(summary.amount_raw > 0);

        let updated_account = resolved.accounts.get(&token).unwrap();
        use spl_token::state::Account as SplAccount;
        let parsed = SplAccount::unpack(&updated_account.data[..SplAccount::LEN]).unwrap();
        assert_eq!(parsed.amount, summary.amount_raw);
    }

    #[test]
    fn create_missing_token_account_initializes_data() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        let (_, mint_account) = spl_token_account_and_mint(&mint, &token);
        create_missing_token_account(
            &mut resolved,
            &token,
            &mint,
            TokenProgramKind::Legacy,
            &mint_account,
        )
        .unwrap();

        let account = resolved.accounts.get(&token).expect("account created");
        assert_eq!(account.owner, legacy_program_id());
        use spl_token::state::Account as SplAccount;
        let parsed = SplAccount::unpack(&account.data[..SplAccount::LEN]).unwrap();
        assert_eq!(Pubkey::new_from_array(parsed.mint.to_bytes()), mint);
        assert_eq!(parsed.amount, 0);
    }

    #[test]
    fn create_missing_token2022_account_base_only() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        let mint_account = token2022_mint_account_base_only();
        create_missing_token_account(
            &mut resolved,
            &token,
            &mint,
            TokenProgramKind::Token2022,
            &mint_account,
        )
        .unwrap();

        let account = resolved.accounts.get(&token).expect("account created");
        assert_eq!(account.owner, token2022_program_id());
        assert_eq!(
            account.data.len(),
            Token2022Account::LEN,
            "base Token2022 account has no extensions"
        );
        let state = StateWithExtensions::<Token2022Account>::unpack(&account.data).expect("unpack");
        assert_eq!(state.base.amount, 0);
        assert!(state.get_extension_types().unwrap().is_empty());
    }

    #[test]
    fn create_missing_token2022_account_with_transfer_fee_extension() {
        let mint = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let mut resolved = ResolvedAccounts { accounts: HashMap::new(), lookups: vec![] };

        let mint_account = token2022_mint_account_with_transfer_fee_config();
        create_missing_token_account(
            &mut resolved,
            &token,
            &mint,
            TokenProgramKind::Token2022,
            &mint_account,
        )
        .unwrap();

        let account = resolved.accounts.get(&token).expect("account created");
        assert_eq!(account.owner, token2022_program_id());
        assert!(
            account.data.len() > Token2022Account::LEN,
            "account with TransferFeeAmount extension must be larger than base"
        );
        let state = StateWithExtensions::<Token2022Account>::unpack(&account.data).expect("unpack");
        assert_eq!(state.base.amount, 0);
        let ext_types = state.get_extension_types().unwrap();
        assert!(
            ext_types.contains(&ExtensionType::TransferFeeAmount),
            "expected TransferFeeAmount extension, got {:?}",
            ext_types
        );
    }

    #[test]
    fn apply_sol_funding_updates_existing_account() {
        let mut svm = LiteSVM::new();
        let key = Pubkey::new_unique();
        let owner = solana_sdk_ids::system_program::id();
        let template = AccountSharedData::new(0, 0, &owner);
        svm.set_account(key, template.into()).unwrap();

        let funding = Funding { pubkey: key, amount_lamports: 1_250_000_000 };
        apply_sol_fundings(&mut svm, &[funding]).expect("funding succeeds");

        let updated = svm.get_account(&key).expect("account exists");
        assert_eq!(updated.lamports(), 1_250_000_000);
    }

    #[test]
    fn apply_sol_funding_creates_account_when_missing() {
        let mut svm = LiteSVM::new();
        let key = Pubkey::new_unique();

        let funding = Funding { pubkey: key, amount_lamports: 500_000_000 };
        apply_sol_fundings(&mut svm, &[funding]).expect("funding succeeds");

        let created = svm.get_account(&key).expect("account created");
        assert_eq!(created.lamports(), 500_000_000);
        assert_eq!(created.owner(), &solana_sdk_ids::system_program::id());
    }

    /// Token2022 mint with no extensions (base state only).
    fn token2022_mint_account_base_only() -> Account {
        use spl_token::solana_program::program_option::COption;

        let mint = Token2022Mint {
            mint_authority: COption::None,
            supply: 0,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        let mut data = vec![0u8; Token2022Mint::LEN];
        Token2022Mint::pack(mint, &mut data).unwrap();
        Account {
            lamports: 0,
            data,
            owner: token2022_program_id(),
            executable: false,
            rent_epoch: 0,
        }
    }

    /// Token2022 mint with TransferFeeConfig extension (requires token account to have TransferFeeAmount).
    fn token2022_mint_account_with_transfer_fee_config() -> Account {
        use spl_token::solana_program::program_option::COption;
        use spl_token_2022::extension::transfer_fee::TransferFeeConfig;

        let account_len = ExtensionType::try_calculate_account_len::<Token2022Mint>(&[
            ExtensionType::TransferFeeConfig,
        ])
        .expect("calculate mint len");
        let mut data = vec![0u8; account_len];
        let mut state = StateWithExtensionsMut::<Token2022Mint>::unpack_uninitialized(&mut data)
            .expect("unpack_uninitialized mint");

        state.base = Token2022Mint {
            mint_authority: COption::None,
            supply: 0,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        state.init_account_type().expect("init_account_type");
        state.pack_base();
        state.init_extension::<TransferFeeConfig>(true).expect("init TransferFeeConfig");

        Account {
            lamports: 0,
            data,
            owner: token2022_program_id(),
            executable: false,
            rent_epoch: 0,
        }
    }

    fn spl_token_account_and_mint(mint: &Pubkey, owner: &Pubkey) -> (Account, Account) {
        use spl_token::solana_program::{program_option::COption, pubkey::Pubkey as ProgramPubkey};
        use spl_token::state::{Account as SplAccount, AccountState, Mint as SplMint};

        let token_state = SplAccount {
            mint: ProgramPubkey::new_from_array(mint.to_bytes()),
            owner: ProgramPubkey::new_from_array(owner.to_bytes()),
            amount: 0,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        };

        let mut token_data = vec![0u8; SplAccount::LEN];
        SplAccount::pack(token_state, &mut token_data).unwrap();

        let mint_state = SplMint {
            mint_authority: COption::None,
            supply: 0,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        let mut mint_data = vec![0u8; SplMint::LEN];
        SplMint::pack(mint_state, &mut mint_data).unwrap();

        let token_account = Account {
            lamports: 0,
            data: token_data,
            owner: legacy_program_id(),
            executable: false,
            rent_epoch: 0,
        };
        let mint_account = Account {
            lamports: 0,
            data: mint_data,
            owner: legacy_program_id(),
            executable: false,
            rent_epoch: 0,
        };
        (token_account, mint_account)
    }
}
