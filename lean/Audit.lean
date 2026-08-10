import VerifiedAnchor

/-
  Audit helper. The headline theorems depend only on `[propext, Quot.sound]`
  (the standard propositional-extensionality and quotient-soundness axioms) —
  no `sorry`, no `Classical.choice`, no `native_decide`.

  Run from the `lean/` directory:

      lake env lean Audit.lean
-/
#print axioms VerifiedAnchor.genValidate_sound
#print axioms VerifiedAnchor.lifecycle_sound
#print axioms VerifiedAnchor.realloc_establishes_post
#print axioms VerifiedAnchor.initIfNeeded_establishes_post
