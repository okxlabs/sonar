use std::collections::HashSet;
use std::sync::LazyLock;

use solana_pubkey::Pubkey;

/// Well-known native program and sysvar IDs that are built into LiteSVM
/// and do not need to be loaded from RPC or local files.
pub(crate) static NATIVE_PROGRAM_IDS: LazyLock<HashSet<Pubkey>> = LazyLock::new(|| {
    use solana_sdk_ids::*;
    HashSet::from([
        system_program::id(),
        bpf_loader::id(),
        bpf_loader_deprecated::id(),
        bpf_loader_upgradeable::id(),
        vote::id(),
        stake::id(),
        config::id(),
        compute_budget::id(),
        address_lookup_table::id(),
        ed25519_program::id(),
        secp256k1_program::id(),
        // sysvar accounts
        solana_sdk_ids::sysvar::clock::id(),
        solana_sdk_ids::sysvar::rent::id(),
        solana_sdk_ids::sysvar::slot_hashes::id(),
        solana_sdk_ids::sysvar::epoch_schedule::id(),
        solana_sdk_ids::sysvar::instructions::id(),
        solana_sdk_ids::sysvar::recent_blockhashes::id(),
    ])
});

/// Returns `true` if the pubkey is a well-known native program or sysvar.
pub(crate) fn is_native_or_sysvar(pubkey: &Pubkey) -> bool {
    NATIVE_PROGRAM_IDS.contains(pubkey)
}
