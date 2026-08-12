// The residual `unexpected_cfgs` warning comes from the `inventory::submit!` item inside the
// `VerifiedAccounts` expansion, where `#[allow]` does not reach. Silenced here so the expected
// stderr does not carry rustc's version-dependent list of known `target_os` values.
#![allow(unexpected_cfgs)]

use verified_anchor::VerifiedAccounts;

// `Account<'info, T>` implies `owner = crate::ID`, so the fixture needs an id to resolve.
verified_anchor::solana_program::declare_id!("VACnUn1111111111111111111111111111111111111");

// A PASS fixture, and it used to be a compile_fail one — that change is the point of M10
// Task 13. `[u8; 32]` has no `map_ty` arm, so the descriptor stops at `name` and never records
// `amount`; the proven byte-level check would then reject every legitimate account. Real Anchor
// compiles and enforces this program, so refusing to build it violated the prime directive.
//
// It now compiles: the macro const-selects on `has_top_level_field`, and in a build where the
// field is not locatable the check runs in `try_accounts` against the deserialised struct
// instead, and is reported in `BadLayout::UNPROVEN_CHECKS`. Proof lost, enforcement kept.
#[verified_anchor::account]
pub struct NameVault {
    pub name: [u8; 32],
    pub amount: u64,
}

#[derive(VerifiedAccounts)]
struct BadLayout<'info> {
    #[account(constraint = vault.amount >= 1000)]
    vault: verified_anchor::Account<'info, NameVault>,
}

fn main() {
    // The check is not silently dropped: it is reported as running outside the proof.
    assert_eq!(BadLayout::UNPROVEN_CHECKS, &["vault.amount >= 1000"]);
}
