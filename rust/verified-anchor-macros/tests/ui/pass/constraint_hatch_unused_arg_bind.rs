// I2 REGRESSION FIXTURE. `#![deny(unused_variables)]` is the whole test: it turns the warning
// this fixture used to raise into a build failure, which is exactly what it was for any user
// crate carrying `#![deny(warnings)]` — a `compile_error!` in all but name, on a program real
// Anchor builds, which the prime directive forbids.
//
// The shape: `#[instruction(amount: u64)]` declares an ARGUMENT named `amount`, and the
// escape-hatch expression `at_least(vault.amount)` mentions the identifier `amount` only as a
// DATA FIELD of `vault`. `idents_in` walks raw tokens and cannot tell the two apart (it must
// over-approximate — missing a name would be a hard build failure), so it reports `amount` as
// used and `instr_arg_binds` emits `let amount: u64 = ..;` that nothing reads. The warning is
// attributed to the USER'S STRUCT SPAN, so it shows up on every build of their crate and cannot
// be silenced from outside the macro.
#![allow(unexpected_cfgs)]
#![deny(unused_variables)]

use verified_anchor::VerifiedAccounts;

verified_anchor::solana_program::declare_id!("VACnUn2222222222222222222222222222222222222");

#[verified_anchor::account]
pub struct ArgVault {
    pub amount: u64,
}

// Outside the sublanguage (a call), so the expression runs verbatim through the escape hatch.
fn at_least(n: u64) -> bool {
    n >= 1000
}

#[derive(VerifiedAccounts)]
#[instruction(amount: u64)]
struct HatchUnusedArg<'info> {
    #[account(constraint = at_least(vault.amount))]
    vault: verified_anchor::Account<'info, ArgVault>,
}

fn main() {
    assert_eq!(HatchUnusedArg::UNPROVEN_CHECKS, &["at_least(vault.amount)"]);
}
