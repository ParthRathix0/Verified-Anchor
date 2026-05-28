# Verified Anchor — Milestone 3 follow-ups

Non-blocking items deferred from M3 (done-bar fully met; final review: SHIP). From the
final whole-implementation review.

## Worth doing (strengthens a claim)

1. **Prove the literal `satisfies (.init/.close)` as corollaries of the Hoare theorems.**
   `init_establishes_post`/`close_establishes_post` (`lean/VerifiedAnchor/Codegen/Lifecycle.lean`)
   prove the *core* post-state properties (owner+size; lamports-zeroed+marker) but not the full
   M1 `satisfies` proposition, which also bundles preserved preconditions (payer signer+writable
   for init; dest resolves for close). The bridge doc has been corrected to state exactly what is
   proven. To make the "⇒ `satisfies`" arrow a literal theorem, add a corollary per side — easiest
   as a *concrete* instantiation in `ExampleGenerated.lean` (a fixed struct + the `applyClose`/
   `applyInit` post-state, closed by `decide` since all data is concrete), mirroring the M1/M2
   concrete examples. (Review verified the full `satisfies (.close …)` evaluates `true` on a
   concrete post-state, so the corollary is sound and cheap.)

## Minor cleanups

2. **Drop the redundant `hne` hypothesis** on `init_establishes_post`/`close_establishes_post`
   (`Lifecycle.lean:58,86`). `idx ≠ payerIdx` / `idx ≠ destIdx` follows from `applyInit/applyClose
   … = some c'` alone (the def guards `if idx = … then none`); derive it internally via
   `if_pos`/`absurd` and remove the parameter.

3. **`lean_spec_string` hardcodes the Lean type name `"Vault"`** for any `has_one` field
   (`rust/verified-anchor-macros/src/lib.rs` ~line 113), regardless of field name. Inert today
   (the offset-8 check ignores the type name, and no test asserts the has_one `lean_spec`), but it
   would misname multi-struct specs. Derive from the field name (capitalize `spec.name`).

4. **has_one is tested natively, not under litesvm.** The design's done-bar mentioned a litesvm
   has_one test; M3 covers has_one via fast native `behavior.rs` tests instead (init/close get the
   litesvm runtime tests). Optionally add a third program instruction + litesvm has_one test.
