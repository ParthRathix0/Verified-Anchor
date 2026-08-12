/-
Axiom audit. Prints the axiom dependencies of every headline theorem so
`scripts/audit-axioms.sh` can assert that none of them depends on anything
beyond `propext` and `Quot.sound`.

Run via: lake env lean scripts/AxiomAudit.lean
-/
import VerifiedAnchor

#print axioms VerifiedAnchor.genValidate_sound
#print axioms VerifiedAnchor.lifecycle_sound
#print axioms VerifiedAnchor.init_establishes_post
#print axioms VerifiedAnchor.close_establishes_post
#print axioms VerifiedAnchor.realloc_establishes_post
#print axioms VerifiedAnchor.initIfNeeded_establishes_post
