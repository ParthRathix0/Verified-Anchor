import VerifiedAnchor.Constraints.Ast
import VerifiedAnchor.Solana.Account

namespace VerifiedAnchor

/-- The runtime accounts, positionally aligned with `AccountsStruct.fields`. -/
abbrev Ctx := List AccountInfo

/-- Resolve a declared field name to its account, by matching field position. -/
def Ctx.lookup (s : AccountsStruct) (c : Ctx) (name : String) : Option AccountInfo := do
  let idx ← List.findIdx? (·.name == name) s.fields
  c[idx]?

/-- Resolve the account paired with a specific field (by index in the struct). -/
def Ctx.atField (_s : AccountsStruct) (c : Ctx) (idx : Nat) : Option AccountInfo :=
  c[idx]?

/-- Structural well-formedness: one account per declared field. -/
def WellFormed (s : AccountsStruct) (c : Ctx) : Prop :=
  c.length = s.fields.length

instance (s : AccountsStruct) (c : Ctx) : Decidable (WellFormed s c) :=
  inferInstanceAs (Decidable (c.length = s.fields.length))

end VerifiedAnchor
