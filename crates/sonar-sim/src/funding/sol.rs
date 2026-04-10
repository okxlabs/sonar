use log::info;
use solana_account::{AccountSharedData, WritableAccount};

use solana_native_token::LAMPORTS_PER_SOL;

use crate::error::{Result, SonarSimError};
use crate::svm_backend::SvmBackend;
use crate::types::SolFunding;

pub fn apply_sol_fundings<B: SvmBackend + ?Sized>(
    svm: &mut B,
    fundings: &[SolFunding],
) -> Result<()> {
    for funding in fundings {
        apply_single_sol_funding(svm, funding)?;
    }
    Ok(())
}

fn apply_single_sol_funding<B: SvmBackend + ?Sized>(
    svm: &mut B,
    funding: &SolFunding,
) -> Result<()> {
    let lamports = funding.amount_lamports;
    let sol = lamports as f64 / LAMPORTS_PER_SOL as f64;
    info!("Funding account {} with {} lamports ({:.9} SOL)", funding.pubkey, lamports, sol);

    if let Some(existing_account) = svm.get_account(&funding.pubkey) {
        let mut updated = existing_account;
        updated.set_lamports(lamports);
        svm.set_account(funding.pubkey, updated)
            .map_err(|e| SonarSimError::Svm { reason: e.to_string() })?;
    } else {
        let system_program_id = solana_sdk_ids::system_program::id();
        let new_account = AccountSharedData::new(lamports, 0, &system_program_id);
        svm.set_account(funding.pubkey, new_account)
            .map_err(|e| SonarSimError::Svm { reason: e.to_string() })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use litesvm::LiteSVM;
    use solana_account::{AccountSharedData, ReadableAccount};
    use solana_pubkey::Pubkey;

    use super::*;

    #[test]
    fn updates_existing_account() {
        let mut svm = LiteSVM::new();
        let key = Pubkey::new_unique();
        let owner = solana_sdk_ids::system_program::id();
        let template = AccountSharedData::new(0, 0, &owner);
        svm.set_account(key, template.into()).unwrap();

        let funding = SolFunding { pubkey: key, amount_lamports: 1_250_000_000 };
        apply_sol_fundings(&mut svm, &[funding]).expect("funding succeeds");

        let updated = svm.get_account(&key).expect("account exists");
        assert_eq!(updated.lamports(), 1_250_000_000);
    }

    #[test]
    fn creates_account_when_missing() {
        let mut svm = LiteSVM::new();
        let key = Pubkey::new_unique();

        let funding = SolFunding { pubkey: key, amount_lamports: 500_000_000 };
        apply_sol_fundings(&mut svm, &[funding]).expect("funding succeeds");

        let created = svm.get_account(&key).expect("account created");
        assert_eq!(created.lamports(), 500_000_000);
        assert_eq!(created.owner(), &solana_sdk_ids::system_program::id());
    }

    #[test]
    fn funds_multiple_accounts() {
        let mut svm = LiteSVM::new();
        let k1 = Pubkey::new_unique();
        let k2 = Pubkey::new_unique();

        let fundings = vec![
            SolFunding { pubkey: k1, amount_lamports: 1_000_000_000 },
            SolFunding { pubkey: k2, amount_lamports: 2_000_000_000 },
        ];
        apply_sol_fundings(&mut svm, &fundings).expect("funding succeeds");

        assert_eq!(svm.get_account(&k1).unwrap().lamports(), 1_000_000_000);
        assert_eq!(svm.get_account(&k2).unwrap().lamports(), 2_000_000_000);
    }

    #[test]
    fn overwrites_previous_balance() {
        let mut svm = LiteSVM::new();
        let key = Pubkey::new_unique();
        let owner = solana_sdk_ids::system_program::id();
        let template = AccountSharedData::new(999_999_999, 0, &owner);
        svm.set_account(key, template.into()).unwrap();

        let funding = SolFunding { pubkey: key, amount_lamports: 100 };
        apply_sol_fundings(&mut svm, &[funding]).unwrap();

        assert_eq!(svm.get_account(&key).unwrap().lamports(), 100);
    }

    #[test]
    fn empty_fundings_is_noop() {
        let mut svm = LiteSVM::new();
        apply_sol_fundings(&mut svm, &[]).expect("empty list succeeds");
    }
}
