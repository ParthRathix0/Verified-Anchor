# Verified Anchor — Milestone 5 follow-ups

Non-blocking items deferred from M5 (done-bar fully met; final whole-implementation review:
SHIP). From the final review.

## Minor cleanups

1. **CLI E2E test soft-skips without `lake`.** `rust/cargo-verified-anchor/tests/cli.rs` returns
   early (reports pass) when `lake` is absent, so it could vacuously "pass" on a Lean-less
   machine. Add a CI job that asserts the Lean toolchain is present so the end-to-end check can
   never silently no-op. (Locally it runs for real — confirmed genuine discharge.)

2. **`seeds` without `bump` silently defaults to `BumpSpec.canonical`.**
   `rust/verified-anchor-macros/src/lib.rs` (`lean_spec_string` bump resolution). Not unsound
   (canonical is the strict choice, and Anchor requires `bump` with `seeds`), but a missing
   `bump` alongside `seeds` would ideally be an explicit `compile_error!` rather than a default.

3. **Assert the `Transfer` struct by name in the CLI test** (currently only `CheckPda` /
   `Lifecycle` are asserted by name; `Transfer` is covered by the exit-status + "discharged"
   summary). One extra `assert!` would improve fast-fail diagnostics.

## Carried over from earlier milestones (still open)

- Tighten `Constraint.discriminator` to `Vector UInt8 8` (from M1/M2 follow-ups).
- Prove the literal `satisfies (.init/.close)` corollaries of the Hoare theorems (M3 follow-up #1).
- Add a `fieldKey` (`field.key()`) seed test — the path is wired but untested (M4 follow-up #1).
- Prune the now-dead M3 subset defs in `Codegen/Soundness.lean` (M4 follow-up #2).
