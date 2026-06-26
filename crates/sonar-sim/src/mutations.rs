// crates/sonar-sim/src/mutations.rs

//! Consolidated mutation configuration for the simulation pipeline.

use solana_pubkey::Pubkey;

use crate::types::{
    AccountDataPatch, AccountOverride, InstructionAccountOp, InstructionDataPatch, InstructionOp,
    SolFunding, TokenFunding,
};

/// Instruction-level mutations applied to the raw transaction before simulation.
#[derive(Debug, Clone, Default)]
pub struct TransactionMutations {
    pub(crate) instruction_ops: Vec<InstructionOp>,
    pub(crate) ix_account_ops: Vec<InstructionAccountOp>,
    pub(crate) ix_data_patches: Vec<InstructionDataPatch>,
}

/// Account-state mutations applied to the SVM before execution.
#[derive(Debug, Clone, Default)]
pub struct StateMutations {
    pub(crate) account_overrides: Vec<AccountOverride>,
    pub(crate) account_closures: Vec<Pubkey>,
    pub(crate) sol_fundings: Vec<SolFunding>,
    pub(crate) token_fundings: Vec<TokenFunding>,
    pub(crate) account_data_patches: Vec<AccountDataPatch>,
}

/// All mutations to apply before simulation execution.
///
/// Groups transaction-level mutations (instruction patches) separately
/// from state-level mutations (account overrides, funding, closures).
/// Construct via [`Mutations::builder()`] or [`Mutations::default()`].
#[derive(Debug, Clone, Default)]
pub struct Mutations {
    /// Instruction patches applied to the raw transaction.
    pub(crate) transaction: TransactionMutations,
    /// Account-state changes applied to the SVM before execution.
    pub(crate) state: StateMutations,
}

impl Mutations {
    pub fn builder() -> MutationsBuilder {
        MutationsBuilder::default()
    }

    pub fn is_empty(&self) -> bool {
        self.transaction.is_empty() && self.state.is_empty()
    }
}

impl TransactionMutations {
    pub fn is_empty(&self) -> bool {
        self.instruction_ops.is_empty()
            && self.ix_account_ops.is_empty()
            && self.ix_data_patches.is_empty()
    }
}

impl StateMutations {
    pub fn is_empty(&self) -> bool {
        self.account_overrides.is_empty()
            && self.account_closures.is_empty()
            && self.sol_fundings.is_empty()
            && self.token_fundings.is_empty()
            && self.account_data_patches.is_empty()
    }
}

/// Builder for [`Mutations`].
#[derive(Debug, Clone, Default)]
pub struct MutationsBuilder {
    inner: Mutations,
}

impl MutationsBuilder {
    pub fn add_override(mut self, account_override: AccountOverride) -> Self {
        self.inner.state.account_overrides.push(account_override);
        self
    }

    pub fn close_account(mut self, pubkey: Pubkey) -> Self {
        self.inner.state.account_closures.push(pubkey);
        self
    }

    pub fn fund_sol(mut self, funding: SolFunding) -> Self {
        self.inner.state.sol_fundings.push(funding);
        self
    }

    pub fn fund_token(mut self, funding: TokenFunding) -> Self {
        self.inner.state.token_fundings.push(funding);
        self
    }

    pub fn patch_account_data(mut self, patch: AccountDataPatch) -> Self {
        self.inner.state.account_data_patches.push(patch);
        self
    }

    /// Append a whole-instruction mutation (insert / remove). Operations apply
    /// in the order added; intended to run before account/data mutations so
    /// that later phases target the post-restructure instruction list.
    pub fn add_instruction_op(mut self, op: InstructionOp) -> Self {
        self.inner.transaction.instruction_ops.push(op);
        self
    }

    /// Append an instruction-account mutation. Operations apply in the order
    /// added; positions are interpreted at apply time.
    pub fn add_ix_account_op(mut self, op: InstructionAccountOp) -> Self {
        self.inner.transaction.ix_account_ops.push(op);
        self
    }

    pub fn patch_ix_data(mut self, patch: InstructionDataPatch) -> Self {
        self.inner.transaction.ix_data_patches.push(patch);
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
        assert!(m.state.is_empty());
        assert!(m.transaction.is_empty());
    }

    #[test]
    fn builder_close_account() {
        let pk = Pubkey::new_unique();
        let m = Mutations::builder().close_account(pk).build();
        assert_eq!(m.state.account_closures, vec![pk]);
    }

    #[test]
    fn builder_fund_sol() {
        let pk = Pubkey::new_unique();
        let m = Mutations::builder()
            .fund_sol(SolFunding { pubkey: pk, amount_lamports: 1_000_000 })
            .build();
        assert_eq!(m.state.sol_fundings.len(), 1);
        assert_eq!(m.state.sol_fundings[0].pubkey, pk);
        assert_eq!(m.state.sol_fundings[0].amount_lamports, 1_000_000);
    }

    #[test]
    fn builder_add_ix_account_op_insert() {
        let key = Pubkey::new_unique();
        let m = Mutations::builder()
            .add_ix_account_op(InstructionAccountOp::Insert {
                instruction_index: 0,
                account_position: 2,
                new_pubkey: key,
                writable: false,
            })
            .build();
        assert_eq!(m.transaction.ix_account_ops.len(), 1);
        assert!(matches!(
            m.transaction.ix_account_ops[0],
            InstructionAccountOp::Insert { account_position: 2, writable: false, .. }
        ));
        assert!(!m.is_empty());
    }

    #[test]
    fn builder_add_ix_account_op_remove_preserves_order() {
        let m = Mutations::builder()
            .add_ix_account_op(InstructionAccountOp::Remove {
                instruction_index: 0,
                account_position: 3,
            })
            .add_ix_account_op(InstructionAccountOp::Remove {
                instruction_index: 0,
                account_position: 1,
            })
            .build();
        assert_eq!(m.transaction.ix_account_ops.len(), 2);
        assert_eq!(m.transaction.ix_account_ops[0].account_position(), 3);
        assert_eq!(m.transaction.ix_account_ops[1].account_position(), 1);
    }

    #[test]
    fn builder_add_instruction_op_remove() {
        let m = Mutations::builder().add_instruction_op(InstructionOp::Remove { index: 2 }).build();
        assert_eq!(m.transaction.instruction_ops.len(), 1);
        assert_eq!(m.transaction.instruction_ops[0].position(), None);
        assert!(!m.is_empty());
    }

    #[test]
    fn builder_instruction_ops_independent_from_account_ops() {
        let m = Mutations::builder()
            .add_instruction_op(InstructionOp::Remove { index: 0 })
            .add_ix_account_op(InstructionAccountOp::Patch {
                instruction_index: 0,
                account_position: 0,
                new_pubkey: Pubkey::new_unique(),
                writable: true,
            })
            .build();
        assert_eq!(m.transaction.instruction_ops.len(), 1);
        assert_eq!(m.transaction.ix_account_ops.len(), 1);
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
        assert_eq!(m.state.account_closures.len(), 2);
        assert_eq!(m.state.sol_fundings.len(), 1);
        assert!(!m.is_empty());
    }
}
