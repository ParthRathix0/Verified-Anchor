# Verified Anchor — Milestone 2 follow-ups

Non-blocking cosmetic items deferred from M2 (done-bar fully met, build/test green, final
review: SHIP). Surfaced by the final whole-implementation review.

1. **Update the M2 design doc to match the shipped code.** Three small drifts to reconcile in
   `docs/superpowers/specs/2026-05-27-verified-anchor-m2-design.md`:
   - `fn validate(&self, accounts: …)` → the shipped trait/impl is
     `fn validate(accounts: …)` (no `&self`; rationale documented in the bridge doc).
   - `VAError` listed 3 variants → 4 in code (`NotEnoughAccounts` added for the count guard;
     consistent with `WellFormed` row in the bridge table).
   - `genOwner … := a.owner == expected` (BEq) → shipped as
     `decide (a.owner = expected)` (semantically identical, lines up with `satisfies`).

2. **Document the `ownerPlaceholder` pattern in the bridge doc.** When users paste their
   `lean_spec()` output into a Lean file, any `owner = EXPR` in their struct emits
   `Constraint.owner ownerPlaceholder` — so the user must declare a (non-`private`) opaque
   `ownerPlaceholder : Pubkey` in scope, or set it to a concrete value before `#guard`/
   `decide`. The example file declares it `private`, which is intentional for that file but
   not exemplary for user-facing usage.

3. **Silence harmless test warnings.** `tests/behavior.rs` has `dead_code` warnings on spec-
   carrier field types and a `mismatched_lifetime_syntaxes` lint on `AccountInfo<'_>`.
   Adding `#![allow(dead_code, mismatched_lifetime_syntaxes)]` at the crate-root of the test
   file would silence them cleanly. Cosmetic.
