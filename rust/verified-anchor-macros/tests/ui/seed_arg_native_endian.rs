// The residual `unexpected_cfgs` warning comes from the `inventory::submit!` item inside the
// `VerifiedAccounts` expansion, where `#[allow]` does not reach. Silenced here so the expected
// stderr does not carry rustc's version-dependent list of known `target_os` values.
#![allow(unexpected_cfgs)]

use verified_anchor::VerifiedAccounts;

// Native-endian is refused alongside big-endian precisely BECAUSE it would work: it is
// little-endian on BPF, so it would pass every on-chain test and be wrong the moment anything
// evaluates the spec off a big-endian host.
#[derive(VerifiedAccounts)]
#[instruction(amount: u64)]
struct NativeEndianSeed<'info> {
    #[account(seeds = [b"vault", amount.to_ne_bytes().as_ref()], bump)]
    vault: verified_anchor::UncheckedAccount<'info>,
}

fn main() {}
