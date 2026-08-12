// The residual `unexpected_cfgs` warning comes from the `inventory::submit!` item inside the
// `VerifiedAccounts` expansion, where `#[allow]` does not reach. Silenced here so the expected
// stderr does not carry rustc's version-dependent list of known `target_os` values.
#![allow(unexpected_cfgs)]

use verified_anchor::VerifiedAccounts;

// `f32` has no `map_ty` arm, so the `#[instruction(..)]` list is truncated there and `name` is
// never recorded. A seed naming it would then resolve to `None` at RUNTIME and reject every
// account — a silently bricked instruction. It must be a build error naming the cause instead.
#[derive(VerifiedAccounts)]
#[instruction(rate: f32, name: String)]
struct BadArgSeed<'info> {
    #[account(seeds = [b"vault", name.as_bytes()], bump)]
    pda: verified_anchor::UncheckedAccount<'info>,
}

fn main() {}
