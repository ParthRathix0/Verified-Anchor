import VerifiedAnchor.Codegen.Generated

namespace VerifiedAnchor

/-- Given the account resolves, `genSigner` agrees with the `signer` contract case. -/
theorem genConstraint_signer_iff (s c idx f a) (h : Ctx.atField s c idx = some a) :
    genConstraint a Constraint.signer = true ↔ satisfies s c idx f Constraint.signer := by
  rw [genConstraint, satisfies]; rw [h]; unfold Option.satisfiesSome
  constructor
  · intro hg; exact ⟨a, rfl, by simpa [genSigner] using hg⟩
  · rintro ⟨a', ha', hp⟩; rw [Option.some.injEq] at ha'; subst ha'; simpa [genSigner] using hp

theorem genConstraint_mut_iff (s c idx f a) (h : Ctx.atField s c idx = some a) :
    genConstraint a Constraint.mut = true ↔ satisfies s c idx f Constraint.mut := by
  rw [genConstraint, satisfies]; rw [h]; unfold Option.satisfiesSome
  constructor
  · intro hg; exact ⟨a, rfl, by simpa [genMut] using hg⟩
  · rintro ⟨a', ha', hp⟩; rw [Option.some.injEq] at ha'; subst ha'; simpa [genMut] using hp

theorem genConstraint_owner_iff (s c idx f a e) (h : Ctx.atField s c idx = some a) :
    genConstraint a (Constraint.owner e) = true ↔ satisfies s c idx f (Constraint.owner e) := by
  rw [genConstraint, satisfies]; rw [h]; unfold Option.satisfiesSome
  constructor
  · intro hg
    exact ⟨a, rfl, by simpa [genOwner, decide_eq_true_iff] using hg⟩
  · rintro ⟨a', ha', hp⟩
    rw [Option.some.injEq] at ha'; subst ha'; simpa [genOwner, decide_eq_true_iff] using hp

end VerifiedAnchor
