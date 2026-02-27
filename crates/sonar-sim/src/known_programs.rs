use solana_pubkey::{Pubkey, pubkey};
use solana_sdk_ids::bpf_loader_upgradeable;

// Program set preloaded by default in `LiteSVM::new()` (builtins + default programs).
// These programs do not need to be fetched via RPC or written into the local account cache.
const LITESVM_BUILTIN_PROGRAM_IDS: [Pubkey; 16] = [
    solana_sdk_ids::system_program::id(),
    solana_sdk_ids::bpf_loader::id(),
    solana_sdk_ids::bpf_loader_deprecated::id(),
    bpf_loader_upgradeable::id(),
    solana_sdk_ids::vote::id(),
    solana_sdk_ids::stake::id(),
    solana_sdk_ids::config::id(),
    solana_sdk_ids::compute_budget::id(),
    solana_sdk_ids::address_lookup_table::id(),
    solana_sdk_ids::ed25519_program::id(),
    solana_sdk_ids::secp256k1_program::id(),
    // SPL programs loaded by default in LiteSVM (see litesvm programs/mod.rs).
    pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"), // SPL Token
    pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"), // SPL Token-2022
    pubkey!("Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo"), // SPL Memo v1
    pubkey!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"), // SPL Memo v3
    pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"), // ATA Program
];

// Sysvars are native and should also be considered local-only,
// but they are not part of the LiteSVM builtin program list.
const EXTRA_NATIVE_SYSVAR_IDS: [Pubkey; 6] = [
    solana_sdk_ids::sysvar::clock::id(),
    solana_sdk_ids::sysvar::rent::id(),
    solana_sdk_ids::sysvar::slot_hashes::id(),
    solana_sdk_ids::sysvar::epoch_schedule::id(),
    solana_sdk_ids::sysvar::instructions::id(),
    solana_sdk_ids::sysvar::recent_blockhashes::id(),
];

pub fn is_native_or_sysvar(pubkey: &Pubkey) -> bool {
    is_litesvm_builtin_program(pubkey) || EXTRA_NATIVE_SYSVAR_IDS.contains(pubkey)
}

pub fn is_litesvm_builtin_program(pubkey: &Pubkey) -> bool {
    LITESVM_BUILTIN_PROGRAM_IDS.contains(pubkey)
}
