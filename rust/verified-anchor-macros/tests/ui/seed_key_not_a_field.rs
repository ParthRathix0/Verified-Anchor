// The residual `unexpected_cfgs` warning comes from the `inventory::submit!` item inside the
// `VerifiedAccounts` expansion, where `#[allow]` does not reach. Silenced here so the expected
// stderr does not carry rustc's version-dependent list of known `target_os` values.
#![allow(unexpected_cfgs)]

use verified_anchor::VerifiedAccounts;

// A `.key()` seed naming something that is not a field of this struct. This used to be a bare
// `panic!` inside the codegen, which rustc reports as "proc-macro derive panicked" with NO
// source location. It must be a spanned error like every other derive-time guard.
#[derive(VerifiedAccounts)]
struct KeySeedNotAField<'info> {
    #[account(seeds = [b"vault", nosuch.key().as_ref()], bump)]
    pda: verified_anchor::UncheckedAccount<'info>,
    user: verified_anchor::UncheckedAccount<'info>,
}

fn main() {}
