// The residual `unexpected_cfgs` warning comes from the `inventory::submit!` item inside the
// `VerifiedAccounts` expansion, where `#[allow]` does not reach. Silenced here so the expected
// stderr does not carry rustc's version-dependent list of known `target_os` values.
#![allow(unexpected_cfgs)]

use verified_anchor::VerifiedAccounts;

// `authority.as_ref()` peels to a bare name, and here that name is BOTH a declared instruction
// argument and an account field. The two readings derive different addresses — the argument's
// 32 bytes versus the account's key bytes — so picking one silently would produce a PDA the
// developer did not mean. Refuse instead.
#[derive(VerifiedAccounts)]
#[instruction(authority: verified_anchor::solana_program::pubkey::Pubkey)]
struct AmbiguousSeed<'info> {
    #[account(seeds = [b"vault", authority.as_ref()], bump)]
    pda: verified_anchor::UncheckedAccount<'info>,
    authority: verified_anchor::UncheckedAccount<'info>,
}

fn main() {}
