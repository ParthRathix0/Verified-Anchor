// The residual `unexpected_cfgs` warning comes from the `inventory::submit!` item inside the
// `VerifiedAccounts` expansion, where `#[allow]` does not reach. Silenced here so the expected
// stderr does not carry rustc's version-dependent list of known `target_os` values.
#![allow(unexpected_cfgs)]

use verified_anchor::VerifiedAccounts;

// `Account<'info, T>` implies `owner = crate::ID`, so the fixture needs an id to resolve.
verified_anchor::solana_program::declare_id!("VAUnLo1111111111111111111111111111111111111");

// `[u8; 32]` has no `map_ty` arm yet, so `#[derive(AccountData)]` truncates the descriptor at
// `name` and never records `authority`. `locate` could then never find the target, and the
// generated check would reject every legitimate account at RUNTIME, silently. It must instead
// be a build error naming the cause.
#[verified_anchor::account]
pub struct NameVault {
    pub name: [u8; 32],
    pub authority: verified_anchor::solana_program::pubkey::Pubkey,
}

#[derive(VerifiedAccounts)]
struct BadLayout<'info> {
    #[account(has_one = authority)]
    vault: verified_anchor::Account<'info, NameVault>,
    authority: verified_anchor::UncheckedAccount<'info>,
}

fn main() {}
