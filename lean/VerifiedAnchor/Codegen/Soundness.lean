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

/-! ## Milestone-2 headline theorem -/

namespace VerifiedAnchor

/-- The constraint kinds M2's generated code handles. -/
def isM2Constraint : Constraint → Bool
  | .signer => true | .mut => true | .owner _ => true | _ => false

/-- Account types whose implied constraints stay within the M2 subset (everything except
    `.account`, which implies the out-of-subset `discriminator`). -/
def isM2Type : AccountType → Bool
  | .account _ _ _ => false
  | _ => true

/-- A struct is in the M2 subset when every field uses an M2 type and only M2 constraints. -/
def M2Subset (s : AccountsStruct) : Prop :=
  ∀ f ∈ s.fields, isM2Type f.ty = true ∧ ∀ k ∈ f.constraints, isM2Constraint k = true

instance (s : AccountsStruct) : Decidable (M2Subset s) := by
  unfold M2Subset; infer_instance

end VerifiedAnchor

namespace VerifiedAnchor

theorem genConstraint_iff_satisfies (s c idx f a k)
    (h : Ctx.atField s c idx = some a) (hk : isM2Constraint k = true) :
    genConstraint a k = true ↔ satisfies s c idx f k := by
  cases k with
  | signer  => exact genConstraint_signer_iff s c idx f a h
  | «mut»   => exact genConstraint_mut_iff s c idx f a h
  | owner e => exact genConstraint_owner_iff s c idx f a e h
  | _       => simp [isM2Constraint] at hk

theorem genFieldValidate_iff (s c idx f a)
    (h : Ctx.atField s c idx = some a)
    (htype : isM2Type f.ty = true)
    (hcons : ∀ k ∈ f.constraints, isM2Constraint k = true) :
    genFieldValidate s c idx f = true ↔ fieldValidates s c idx f := by
  unfold genFieldValidate fieldValidates
  rw [h, List.all_eq_true]
  -- Goal: (∀ k ∈ impl ++ expl, genConstraint a k = true) ↔ (∀ k ∈ impl ++ expl, satisfies … k)
  -- Prove element-wise, using that every k in impl++expl is an M2 constraint.
  have hmem : ∀ k ∈ (f.ty.impliedConstraints ++ f.constraints), isM2Constraint k = true := by
    intro k hkmem
    rcases List.mem_append.mp hkmem with himpl | hexpl
    · -- implied constraints of an M2 type are signer-only or empty
      cases hf : f.ty with
      | account _ _ _ => rw [hf] at htype; simp [isM2Type] at htype
      | signer => rw [hf] at himpl; simp [AccountType.impliedConstraints] at himpl;
                  subst himpl; rfl
      | program _ => rw [hf] at himpl; simp [AccountType.impliedConstraints] at himpl
      | systemAccount => rw [hf] at himpl; simp [AccountType.impliedConstraints] at himpl
      | uncheckedAccount => rw [hf] at himpl; simp [AccountType.impliedConstraints] at himpl
    · exact hcons k hexpl
  constructor
  · intro hall k hkmem
    exact (genConstraint_iff_satisfies s c idx f a k h (hmem k hkmem)).mp (hall k hkmem)
  · intro hall k hkmem
    exact (genConstraint_iff_satisfies s c idx f a k h (hmem k hkmem)).mpr (hall k hkmem)

end VerifiedAnchor

namespace VerifiedAnchor

/-- THE MILESTONE-2 THEOREM: the generated validator agrees with the M1 contract for every
    struct in the M2 subset. Proved once, parameterized over the user's annotation. -/
theorem genValidate_sound (s : AccountsStruct) (c : Ctx) (h : M2Subset s) :
    genValidate s c = true ↔ validates s c := by
  unfold genValidate validates
  rw [Bool.and_eq_true, decide_eq_true_iff]
  constructor
  · rintro ⟨hwf, hall⟩
    refine ⟨hwf, ?_⟩
    rw [List.all_eq_true] at hall
    intro p hp
    have hmemf : p.1 ∈ s.fields := List.fst_mem_of_mem_zipIdx hp
    obtain ⟨htype, hcons⟩ := h p.1 hmemf
    have hgf := hall p hp
    obtain ⟨a, ha⟩ : ∃ a, Ctx.atField s c p.2 = some a := by
      unfold genFieldValidate at hgf
      cases hr : Ctx.atField s c p.2 with
      | none => rw [hr] at hgf; simp at hgf
      | some a => exact ⟨a, rfl⟩
    exact (genFieldValidate_iff s c p.2 p.1 a ha htype hcons).mp hgf
  · rintro ⟨hwf, hall⟩
    refine ⟨hwf, ?_⟩
    rw [List.all_eq_true]
    intro p hp
    have hmemf : p.1 ∈ s.fields := List.fst_mem_of_mem_zipIdx hp
    obtain ⟨htype, hcons⟩ := h p.1 hmemf
    have hidx : p.2 < s.fields.length := by
      obtain ⟨x, i⟩ := p
      have hz := List.mem_zipIdx hp
      simpa using hz.2.1
    obtain ⟨a, ha⟩ : ∃ a, Ctx.atField s c p.2 = some a := by
      have hlt : p.2 < c.length := by rw [hwf]; exact hidx
      unfold Ctx.atField
      exact ⟨c[p.2], List.getElem?_eq_getElem hlt⟩
    exact (genFieldValidate_iff s c p.2 p.1 a ha htype hcons).mpr (hall p hp)

end VerifiedAnchor
