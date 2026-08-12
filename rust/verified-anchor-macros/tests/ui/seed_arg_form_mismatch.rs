// The residual `unexpected_cfgs` warning comes from the `inventory::submit!` item inside the
// `VerifiedAccounts` expansion, where `#[allow]` does not reach. Silenced here so the expected
// stderr does not carry rustc's version-dependent list of known `target_os` values.
#![allow(unexpected_cfgs)]

use verified_anchor::VerifiedAccounts;

// The seed bytes come from the DECLARED TYPE, never from the method name. `amount.as_bytes()`
// would therefore quietly succeed and yield the 8-byte little-endian encoding — bytes the
// spelling does not describe, and source rustc would reject if it were ever emitted as Rust.
// The seed list must read the way equivalent Anchor source reads.
#[derive(VerifiedAccounts)]
#[instruction(amount: u64)]
struct MismatchedSeedForm<'info> {
    #[account(seeds = [b"vault", amount.as_bytes()], bump)]
    vault: verified_anchor::UncheckedAccount<'info>,
}

fn main() {}
