// The residual `unexpected_cfgs` warning comes from the `inventory::submit!` item inside the
// `VerifiedAccounts` expansion, where `#[allow]` does not reach. Silenced here so the expected
// stderr does not carry rustc's version-dependent list of known `target_os` values.
#![allow(unexpected_cfgs)]

use verified_anchor::VerifiedAccounts;

// A bare seed name that is neither an instruction argument nor an account field. At runtime it
// would resolve to nothing and reject every account forever, so it must be a build error.
#[derive(VerifiedAccounts)]
#[instruction(authority: verified_anchor::solana_program::pubkey::Pubkey)]
struct UnknownSeedName<'info> {
    #[account(seeds = [b"vault", autority.as_ref()], bump)]
    pda: verified_anchor::UncheckedAccount<'info>,
}

fn main() {}
