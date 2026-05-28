import VerifiedAnchor.Codegen.Generated

namespace VerifiedAnchor

/-- The key bridge: `allB` agrees with `satisfiesSome` of the `= true` predicate. -/
theorem Option.allB_iff {α} (o : Option α) (p : α → Bool) :
    o.allB p = true ↔ o.satisfiesSome (fun a => p a = true) := by
  cases o <;> simp [Option.allB, Option.satisfiesSome]

theorem genConstraint_signer_iff (s c idx f) :
    genConstraint s c idx f Constraint.signer = true ↔ satisfies s c idx f Constraint.signer := by
  simp only [genConstraint, satisfies]; exact Option.allB_iff _ _

theorem genConstraint_mut_iff (s c idx f) :
    genConstraint s c idx f Constraint.mut = true ↔ satisfies s c idx f Constraint.mut := by
  simp only [genConstraint, satisfies]; exact Option.allB_iff _ _

theorem genConstraint_owner_iff (s c idx f e) :
    genConstraint s c idx f (Constraint.owner e) = true ↔ satisfies s c idx f (Constraint.owner e) := by
  simp only [genConstraint, satisfies, Option.allB_iff, decide_eq_true_iff]

theorem genConstraint_discriminator_iff (s c idx f d) :
    genConstraint s c idx f (Constraint.discriminator d) = true ↔ satisfies s c idx f (Constraint.discriminator d) := by
  simp only [genConstraint, satisfies, Option.allB_iff, decide_eq_true_iff]

theorem genConstraint_hasOne_iff (s c idx f field) :
    genConstraint s c idx f (Constraint.hasOne field) = true ↔ satisfies s c idx f (Constraint.hasOne field) := by
  simp only [genConstraint, genHasOne, satisfies, Option.allB_iff, decide_eq_true_iff]

/-- Constraint kinds M3's generated validator handles. -/
def isM3Constraint : Constraint → Bool
  | .signer | .mut | .owner _ | .hasOne _ | .discriminator _ => true
  | _ => false

/-- The M3 subset: every field's (implied ++ explicit) constraints are M3 validation
    constraints. Typed `.account` is allowed (its implied owner+discriminator are M3). -/
def M3Subset (s : AccountsStruct) : Prop :=
  ∀ f ∈ s.fields, ∀ k ∈ (f.ty.impliedConstraints ++ f.constraints), isM3Constraint k = true

instance (s : AccountsStruct) : Decidable (M3Subset s) := by unfold M3Subset; infer_instance

theorem genConstraint_iff_satisfies_M3 (s c idx f k) (hk : isM3Constraint k = true) :
    genConstraint s c idx f k = true ↔ satisfies s c idx f k := by
  cases k with
  | signer        => exact genConstraint_signer_iff s c idx f
  | «mut»         => exact genConstraint_mut_iff s c idx f
  | owner e       => exact genConstraint_owner_iff s c idx f e
  | hasOne field  => exact genConstraint_hasOne_iff s c idx f field
  | discriminator d => exact genConstraint_discriminator_iff s c idx f d
  | _             => simp [isM3Constraint] at hk

theorem genFieldValidate_iff (s c idx f)
    (hcons : ∀ k ∈ (f.ty.impliedConstraints ++ f.constraints), isM3Constraint k = true) :
    genFieldValidate s c idx f = true ↔ fieldValidates s c idx f := by
  unfold genFieldValidate fieldValidates
  rw [List.all_eq_true]
  constructor
  · intro hall k hk; exact (genConstraint_iff_satisfies_M3 s c idx f k (hcons k hk)).mp (hall k hk)
  · intro hall k hk; exact (genConstraint_iff_satisfies_M3 s c idx f k (hcons k hk)).mpr (hall k hk)

/-- THE M3 THEOREM: the generated validator agrees with the M1 contract for every struct in
    the M3 subset. -/
theorem genValidate_sound (s : AccountsStruct) (c : Ctx) (h : M3Subset s) :
    genValidate s c = true ↔ validates s c := by
  unfold genValidate validates
  rw [Bool.and_eq_true, decide_eq_true_iff]
  constructor
  · rintro ⟨hwf, hall⟩
    refine ⟨hwf, ?_⟩
    rw [List.all_eq_true] at hall
    intro p hp
    have hmemf : p.1 ∈ s.fields := List.fst_mem_of_mem_zipIdx hp
    exact (genFieldValidate_iff s c p.2 p.1 (h p.1 hmemf)).mp (hall p hp)
  · rintro ⟨hwf, hall⟩
    refine ⟨hwf, ?_⟩
    rw [List.all_eq_true]
    intro p hp
    have hmemf : p.1 ∈ s.fields := List.fst_mem_of_mem_zipIdx hp
    exact (genFieldValidate_iff s c p.2 p.1 (h p.1 hmemf)).mpr (hall p hp)

end VerifiedAnchor
