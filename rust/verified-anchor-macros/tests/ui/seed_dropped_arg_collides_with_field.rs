// The residual `unexpected_cfgs` warning comes from the `inventory::submit!` item inside the
// `VerifiedAccounts` expansion, where `#[allow]` does not reach. Silenced here so the expected
// stderr does not carry rustc's version-dependent list of known `target_os` values.
#![allow(unexpected_cfgs)]

use verified_anchor::VerifiedAccounts;

verified_anchor::solana_program::declare_id!("VADrop1111111111111111111111111111111111111");

// THE SILENT-WRONG-ADDRESS CASE. `f32` is unmappable, so the cutoff drops `authority` — and
// `authority` is ALSO an account field here. Falling back to the field would emit
// `SeedSpec.fieldKey "authority"` (the ACCOUNT's key bytes) with no diagnostic, while real
// Anchor evaluates this seed with the ARGUMENT in scope and derives from its 32 bytes.
// Different address, no error, and our own tests plus the Lean model would agree with the wrong
// answer. `#[instruction(params: SomeStruct, authority: Pubkey)]` beside an `authority`
// account is ordinary Anchor, so this shape is not exotic.
#[derive(VerifiedAccounts)]
#[instruction(rate: f32, authority: verified_anchor::solana_program::pubkey::Pubkey)]
struct DroppedArgCollides<'info> {
    #[account(seeds = [b"vault", authority.as_ref()], bump)]
    pda: verified_anchor::UncheckedAccount<'info>,
    authority: verified_anchor::UncheckedAccount<'info>,
}

fn main() {}
