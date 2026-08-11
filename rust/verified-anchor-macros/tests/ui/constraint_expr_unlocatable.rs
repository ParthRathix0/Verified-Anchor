// The residual `unexpected_cfgs` warning comes from the `inventory::submit!` item inside the
// `VerifiedAccounts` expansion, where `#[allow]` does not reach. Silenced here so the expected
// stderr does not carry rustc's version-dependent list of known `target_os` values.
#![allow(unexpected_cfgs)]

use verified_anchor::VerifiedAccounts;

// `Account<'info, T>` implies `owner = crate::ID`, so the fixture needs an id to resolve.
verified_anchor::solana_program::declare_id!("VACnUn1111111111111111111111111111111111111");

// Same truncation trap as `has_one_unlocatable`, reached through `constraint = <expr>`:
// `[u8; 32]` has no `map_ty` arm, so the descriptor stops at `name` and never records `amount`.
// A `constraint` reading `amount` would then reject every legitimate account at RUNTIME,
// silently. It must be a build error naming the cause instead.
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

fn main() {}
