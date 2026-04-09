// crates/sonar-sim/src/mutations.rs

//! Consolidated mutation configuration for the simulation pipeline.

use solana_pubkey::Pubkey;

use crate::types::{
    AccountDataPatch, AccountOverride, InstructionAccountAppend,
    InstructionAccountPatch, InstructionDataPatch, SolFunding, TokenFunding,
};

/// All mutations to apply before simulation execution.
///
/// Combines both transaction-level mutations (instruction patches) and
/// state-level mutations (account overrides, funding, closures).
/// Construct via [`Mutations::builder()`] or [`Mutations::default()`].
#[derive(Debug, Clone, Default)]
pub struct Mutations {
    pub(crate) account_overrides: Vec<AccountOverride>,
    pub(crate) account_closures: Vec<Pubkey>,
    pub(crate) sol_fundings: Vec<SolFunding>,
    pub(crate) token_fundings: Vec<TokenFunding>,
    pub(crate) account_data_patches: Vec<AccountDataPatch>,
    pub(crate) ix_account_patches: Vec<InstructionAccountPatch>,
    pub(crate) ix_account_appends: Vec<InstructionAccountAppend>,
    pub(crate) ix_data_patches: Vec<InstructionDataPatch>,
}

impl Mutations {
    pub fn builder() -> MutationsBuilder {
        MutationsBuilder::default()
    }

    pub fn is_empty(&self) -> bool {
        self.account_overrides.is_empty()
            && self.account_closures.is_empty()
            && self.sol_fundings.is_empty()
            && self.token_fundings.is_empty()
            && self.account_data_patches.is_empty()
            && self.ix_account_patches.is_empty()
            && self.ix_account_appends.is_empty()
            && self.ix_data_patches.is_empty()
    }
}

/// Builder for [`Mutations`].
#[derive(Debug, Clone, Default)]
pub struct MutationsBuilder {
    inner: Mutations,
}

impl MutationsBuilder {
    pub fn add_override(mut self, account_override: AccountOverride) -> Self {
        self.inner.account_overrides.push(account_override);
        self
    }

    pub fn close_account(mut self, pubkey: Pubkey) -> Self {
        self.inner.account_closures.push(pubkey);
        self
    }

    pub fn fund_sol(mut self, funding: SolFunding) -> Self {
        self.inner.sol_fundings.push(funding);
        self
    }

    pub fn fund_token(mut self, funding: TokenFunding) -> Self {
        self.inner.token_fundings.push(funding);
        self
    }

    pub fn patch_account_data(mut self, patch: AccountDataPatch) -> Self {
        self.inner.account_data_patches.push(patch);
        self
    }

    pub fn patch_ix_account(mut self, patch: InstructionAccountPatch) -> Self {
        self.inner.ix_account_patches.push(patch);
        self
    }

    pub fn append_ix_account(mut self, append: InstructionAccountAppend) -> Self {
        self.inner.ix_account_appends.push(append);
        self
    }

    pub fn patch_ix_data(mut self, patch: InstructionDataPatch) -> Self {
        self.inner.ix_data_patches.push(patch);
        self
    }

    pub fn build(self) -> Mutations {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mutations_are_empty() {
        let m = Mutations::default();
        assert!(m.is_empty());
        assert!(m.account_overrides.is_empty());
        assert!(m.account_closures.is_empty());
        assert!(m.sol_fundings.is_empty());
        assert!(m.token_fundings.is_empty());
        assert!(m.account_data_patches.is_empty());
        assert!(m.ix_account_patches.is_empty());
        assert!(m.ix_account_appends.is_empty());
        assert!(m.ix_data_patches.is_empty());
    }

    #[test]
    fn builder_close_account() {
        let pk = Pubkey::new_unique();
        let m = Mutations::builder().close_account(pk).build();
        assert_eq!(m.account_closures, vec![pk]);
    }

    #[test]
    fn builder_fund_sol() {
        let pk = Pubkey::new_unique();
        let m = Mutations::builder()
            .fund_sol(SolFunding { pubkey: pk, amount_lamports: 1_000_000 })
            .build();
        assert_eq!(m.sol_fundings.len(), 1);
        assert_eq!(m.sol_fundings[0].pubkey, pk);
        assert_eq!(m.sol_fundings[0].amount_lamports, 1_000_000);
    }

    #[test]
    fn builder_chains_multiple_types() {
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let m = Mutations::builder()
            .close_account(pk1)
            .fund_sol(SolFunding { pubkey: pk2, amount_lamports: 500 })
            .close_account(pk2)
            .build();
        assert_eq!(m.account_closures.len(), 2);
        assert_eq!(m.sol_fundings.len(), 1);
        assert!(!m.is_empty());
    }
}
