# Verified Anchor — Milestone 1 follow-ups

Non-blocking items deferred from M1 (build is green, done-bar met). These are refinements
to address as later milestones build on the constraint AST seam.

## Tracked for M2+ (constraint AST seam)

1. **Tighten `Constraint.discriminator` to `Vector UInt8 8`.** Currently
   `discriminator (expected : ByteArray)` (`Constraints/Ast.lean`), with the 8-byte length
   a convention rather than a type guarantee. The design doc (§4) specified `Vector UInt8 8`.
   Safe today (`hasDiscriminator` only compares the first 8 bytes), but tightening the type
   makes ill-formed discriminators unrepresentable. Touches `accountDiscriminator` (returns
   `ByteArray`), `hasDiscriminator`, and the example's `vaultDisc`. Do this before M2 freezes
   the Rust↔Lean seam.

## Tracked for M4 (PDA verification)

2. **`resolveSeeds` should fail explicitly on an unresolvable `fieldKey`.** Currently
   (`Contract/Satisfies.lean`) a missing field substitutes `ByteArray.empty`. This fails
   *safe* (wrong seeds → wrong PDA → constraint false, never vacuously true), but returning
   `Option (List ByteArray)` so a missing reference fails explicitly is cleaner. The `seeds`
   case and its decidability would thread the `Option`. M4 owns PDA verification, so fold it
   in there.

## Minor cleanups (any time)

3. **Remove or annotate unused helper `AccountInfo.dataPrefix`** (`Solana/Account.lean`) —
   currently unused by the contract/checker/examples. Either drop it or mark `-- exposed for M2`.
4. **Comment the inclusive bump-0 branch** in `findProgramAddress` (`Solana/Crypto.lean`) —
   the `bump = 0` case is the last derivation attempt (matches Solana); a one-line note would
   make the intent obvious.
