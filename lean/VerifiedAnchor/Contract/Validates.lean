import VerifiedAnchor.Contract.Satisfies

namespace VerifiedAnchor

/-- A field satisfies all its constraints (type-implied ones first, then explicit). -/
def fieldValidates (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) : Prop :=
  ∀ k ∈ (f.ty.impliedConstraints ++ f.constraints), satisfies s c idx f k

instance (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) :
    Decidable (fieldValidates s c idx f) := by
  unfold fieldValidates; infer_instance

/-- THE CONTRACT: a context validates a struct iff it is well-formed and every field
    satisfies all of its constraints. -/
def validates (s : AccountsStruct) (c : Ctx) : Prop :=
  WellFormed s c ∧
    ∀ p ∈ s.fields.zipIdx, fieldValidates s c p.2 p.1

instance (s : AccountsStruct) (c : Ctx) : Decidable (validates s c) := by
  unfold validates; infer_instance

end VerifiedAnchor
