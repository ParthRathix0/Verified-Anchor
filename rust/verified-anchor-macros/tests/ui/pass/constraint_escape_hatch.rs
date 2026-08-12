// The PRIME DIRECTIVE, made executable: an expression OUTSIDE the proven sublanguage must
// COMPILE. Real Anchor accepts arbitrary Rust in `constraint = ...`, so rejecting any of it
// would break the drop-in property — a developer's program would stop building on a framework
// that advertises itself as a drop-in replacement.
//
// A `t.pass(...)` fixture, deliberately: `compile_fail` fixtures pin what we reject, and this
// pins what we must NEVER reject. If someone reintroduces a `compile_error!` for
// out-of-sublanguage expressions, this test turns red.
#![allow(unexpected_cfgs)]

use verified_anchor::VerifiedAccounts;
// A real Anchor program always has `use anchor_lang::prelude::*;`, which brings the `Key`
// trait (and therefore `.key()`) into scope. `verified_anchor::prelude` does the same (see
// `rust/verified-anchor/src/prelude.rs`); this fixture imports the trait directly to keep the
// rest of the module minimal, matching how the OTHER fixtures in this directory are written.
use verified_anchor::Key;

verified_anchor::solana_program::declare_id!("VAEscHt111111111111111111111111111111111111");

fn is_blessed(k: &verified_anchor::solana_program::pubkey::Pubkey) -> bool {
    k.as_ref()[0] == 1
}

#[derive(VerifiedAccounts)]
struct EscapeHatch<'info> {
    // A function call: no sublanguage arm can model it.
    #[account(constraint = is_blessed(a.key))]
    // A macro: `Expr::Macro` must route to the hatch, not panic.
    #[account(constraint = matches!(a.lamports(), 1..=u64::MAX))]
    // The two most common real-Anchor idioms, both outside the sublanguage today: a
    // module-qualified path, and a reference to one.
    #[account(constraint = a.key() == crate::ID)]
    #[account(constraint = a.owner == &crate::ID)]
    a: verified_anchor::UncheckedAccount<'info>,
}

fn main() {
    assert_eq!(EscapeHatch::UNPROVEN_CHECKS.len(), 4);
}
