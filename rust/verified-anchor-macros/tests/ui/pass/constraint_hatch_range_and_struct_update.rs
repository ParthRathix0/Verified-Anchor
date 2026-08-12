// REGRESSION FIXTURE for the `idents_in` refinement (M10 final review, item I2).
//
// `idents_in` drops identifiers that directly follow a `.` — field and method names — so that
// `constraint = at_least(vault.amount)` does not claim to use an `#[instruction(amount: u64)]`
// argument it never mentions. The first cut of that filter set its flag on EVERY `.` token,
// including BOTH dots of a range operator, so in `lo..hi` the identifier `hi` looked like a
// field name and was dropped from the used-name set — never bound in the verbatim hatch, and
// therefore `error[E0425]: cannot find value 'hi' in this scope` on a program REAL ANCHOR
// COMPILES. Struct-update syntax (`Foo { ..base }`) is the same shape.
//
// A `t.pass(...)` fixture because the prime directive is "never refuse a construct real Anchor
// accepts": the failure this pins is a BUILD ERROR, not a wrong answer.
#![allow(unexpected_cfgs)]

use verified_anchor::VerifiedAccounts;

verified_anchor::solana_program::declare_id!("VARange111111111111111111111111111111111111");

#[verified_anchor::account]
struct Vault {
    amount: u64,
}

#[derive(Clone)]
struct Bounds {
    lo: u64,
    hi: u64,
}

fn base_bounds() -> Bounds {
    Bounds { lo: 0, hi: u64::MAX }
}

#[derive(VerifiedAccounts)]
#[instruction(lo: u64, hi: u64)]
struct Ranged<'info> {
    // Exclusive range: the SECOND dot is immediately followed by `hi`, a value-position
    // identifier that must still be bound.
    #[account(constraint = (lo..hi).contains(&vault.amount))]
    // Inclusive range: `..=` already cleared the flag (the `=` follows the second dot), kept
    // here so the safe shape is pinned alongside the broken one.
    #[account(constraint = (lo..=hi).contains(&vault.amount))]
    // Struct-update syntax: `..base` is the same class of false positive.
    #[account(constraint = Bounds { lo, ..base_bounds() }.hi >= hi)]
    vault: verified_anchor::Account<'info, Vault>,
}

fn main() {
    assert_eq!(Ranged::UNPROVEN_CHECKS.len(), 3);
}
