import VerifiedAnchor.Contract.Satisfies

namespace VerifiedAnchor

/-- A field satisfies all its constraints (type-implied ones first, then explicit). -/
def fieldValidates (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) : Prop :=
  ∀ k ∈ (f.ty.impliedConstraints ++ f.constraints), satisfies s c idx f k

instance (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) :
    Decidable (fieldValidates s c idx f) := by
  unfold fieldValidates; infer_instance

/-- A field carries the `mut` constraint (type-implied or explicit). The whole point of the
    distinct-mut-key check is to defend writable accounts, so membership is over the SAME
    `(impliedConstraints ++ constraints)` list `fieldValidates` evaluates. Tested via the
    `Constraint.isMut` constructor predicate (no `DecidableEq Constraint` available). -/
def isMutField (f : AccountField) : Prop :=
  (f.ty.impliedConstraints ++ f.constraints).any Constraint.isMut = true

instance (f : AccountField) : Decidable (isMutField f) := by
  unfold isMutField; infer_instance

/-- A `(fi, fj)` pair is exempt from the distinct-key requirement iff either field explicitly
    lists the other in its `allowDuplicate` opt-out. -/
def exemptPair (fi fj : AccountField) : Prop :=
  fj.name ∈ fi.allowDuplicate ∨ fi.name ∈ fj.allowDuplicate

instance (fi fj : AccountField) : Decidable (exemptPair fi fj) := by
  unfold exemptPair; infer_instance

/-- THE STRUCT-LEVEL SAFETY CHECK (M8.4): every ordered pair of distinct field indices `i < j`
    that are BOTH `mut` and NOT mutually opted-out must resolve to accounts with distinct keys.
    None-safe via `satisfiesSome`: a missing account (`atField = none`) leaves the obligation
    unsatisfied, exactly like every per-field constraint. Iterating `zipIdx × zipIdx` with the
    `i < j` guard quantifies over a finite list, so the whole thing stays decidable. -/
def distinctMutKeys (s : AccountsStruct) (c : Ctx) : Prop :=
  ∀ p ∈ s.fields.zipIdx, ∀ q ∈ s.fields.zipIdx,
    p.2 < q.2 → isMutField p.1 → isMutField q.1 → ¬ exemptPair p.1 q.1 →
      (Ctx.atField s c p.2).satisfiesSome (fun a =>
        (Ctx.atField s c q.2).satisfiesSome (fun b => a.key ≠ b.key))

instance (s : AccountsStruct) (c : Ctx) : Decidable (distinctMutKeys s c) := by
  unfold distinctMutKeys; infer_instance

/-- THE CONTRACT: a context validates a struct iff it is well-formed, all of its mutable
    accounts have pairwise-distinct keys (unless explicitly opted out), and every field
    satisfies all of its constraints. -/
def validates (s : AccountsStruct) (c : Ctx) : Prop :=
  WellFormed s c ∧
    distinctMutKeys s c ∧
    ∀ p ∈ s.fields.zipIdx, fieldValidates s c p.2 p.1

instance (s : AccountsStruct) (c : Ctx) : Decidable (validates s c) := by
  unfold validates; infer_instance

end VerifiedAnchor
