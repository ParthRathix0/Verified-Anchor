import VerifiedAnchor.Contract.Validates

namespace VerifiedAnchor

/-- Executable account-validation checker. Agrees with `validates` by construction
    (it is the decision procedure of the `Decidable (validates …)` instance). -/
def validatesBool (s : AccountsStruct) (c : Ctx) : Bool :=
  decide (validates s c)

/-- Per-constraint executable check, exposed for examples/diagnostics. -/
def checkConstraint (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField)
    (k : Constraint) : Bool :=
  decide (satisfies s c idx f k)

end VerifiedAnchor
