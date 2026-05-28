# Verified Anchor — Milestone 4 follow-ups

Non-blocking items deferred from M4 (done-bar fully met; final whole-implementation review:
SHIP). From the final review.

## Worth doing (strengthens a claim)

1. **Add a `fieldKey` seed test.** The `SeedSpec.fieldKey` / `field.key()` seed path is fully
   wired on both sides — Lean `resolveSeeds` (`Contract/Satisfies.lean`) and the Rust macro's
   `accounts[fi].key.as_ref()` (`verified-anchor-macros/src/lib.rs`) — but is exercised by no
   test or example. M4's tests cover `literal` + `instrArg` only. Add a native `behavior.rs`
   test (and/or a litesvm case) using `seeds = [b"...", other_field.key()]` so all three
   `SeedSpec` variants are empirically covered.

## Minor cleanups

2. **Prune the now-dead M3 subset defs.** `isM3Constraint`, `M3Subset`, and
   `genConstraint_iff_satisfies_M3` (`Codegen/Soundness.lean`) are superseded by their M4
   counterparts and are no longer referenced by any theorem. They still build (harmless) and
   were deliberately kept during M4 to avoid breaking the build mid-task. Either delete them or
   add a doc-comment marking them as kept for milestone provenance.

## Carried over from earlier milestones (still open)

- M3 follow-up #1: prove the literal `satisfies (.init/.close)` as corollaries of the Hoare
  theorems (see `docs/superpowers/m3-followups.md`).
- Tighten `Constraint.discriminator` to `Vector UInt8 8` (from M1/M2 follow-ups).
