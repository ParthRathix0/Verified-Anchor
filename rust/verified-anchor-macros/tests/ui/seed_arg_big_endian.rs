// The residual `unexpected_cfgs` warning comes from the `inventory::submit!` item inside the
// `VerifiedAccounts` expansion, where `#[allow]` does not reach. Silenced here so the expected
// stderr does not carry rustc's version-dependent list of known `target_os` values.
#![allow(unexpected_cfgs)]

use verified_anchor::VerifiedAccounts;

// Borsh — and therefore the PDA Anchor derives — is little-endian. A big-endian seed would
// derive a DIFFERENT address than the same source under Anchor, and the mismatch would look
// like "wrong account passed" rather than "wrong seed encoding". There is no correct way to
// honour it, so it must not compile.
#[derive(VerifiedAccounts)]
#[instruction(amount: u64)]
struct BigEndianSeed<'info> {
    #[account(seeds = [b"vault", amount.to_be_bytes().as_ref()], bump)]
    vault: verified_anchor::UncheckedAccount<'info>,
}

fn main() {}
