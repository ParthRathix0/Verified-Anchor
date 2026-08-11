import VerifiedAnchor.Constraints.Ast
import VerifiedAnchor.Solana.Account

namespace VerifiedAnchor

/-- The runtime context: accounts positionally aligned with `AccountsStruct.fields`,
    plus the raw instruction data (used by `seeds = [arg(..)]`). -/
structure Ctx where
  accounts  : List AccountInfo
  instrData : ByteArray := ByteArray.empty
  deriving Inhabited
-- DecidableEq not derived: ByteArray lacks it; equality on Ctx is not needed yet.

/-- Build a Ctx from just accounts (instrData empty). Keeps existing examples terse. -/
def Ctx.ofAccounts (l : List AccountInfo) : Ctx := { accounts := l }

/-- Number of runtime accounts. -/
def Ctx.length (c : Ctx) : Nat := c.accounts.length

/-- Resolve a declared field name to its account, by matching field position. -/
def Ctx.lookup (s : AccountsStruct) (c : Ctx) (name : String) : Option AccountInfo := do
  let idx ← List.findIdx? (·.name == name) s.fields
  c.accounts[idx]?

/-- Resolve the account paired with a specific field (by index in the struct). -/
def Ctx.atField (_s : AccountsStruct) (c : Ctx) (idx : Nat) : Option AccountInfo :=
  c.accounts[idx]?

/-- The raw bytes of a named `#[instruction(...)]` argument, as a seed would use them.
    For `string`/`vec` the LENGTH PREFIX IS STRIPPED — `name.as_bytes()` in Anchor yields the
    payload, not the Borsh framing (real Anchor never hands seed code a length prefix).
    Fixed-size types return their whole encoding. Lives here (not in `Ast.lean`) because it
    needs `Ctx`, and `Ctx` is defined in this file, which `Ast.lean` cannot import without a
    cycle. -/
def AccountsStruct.argBytes (s : AccountsStruct) (c : Ctx) (name : String) : Option ByteArray := do
  let (off, t) ← locate (Ty.struct s.instrArgs) [name] c.instrData 0
  match t with
  | .string | .vec _ => do
      let n ← readUIntLE c.instrData off 4
      if off + 4 + n ≤ c.instrData.size then
        pure (c.instrData.extract (off + 4) (off + 4 + n))
      else none
  | other => do
      let w ← encodedWidth other c.instrData off
      if off + w ≤ c.instrData.size then
        pure (c.instrData.extract off (off + w))
      else none

/-- Structural well-formedness: one account per declared field. -/
def WellFormed (s : AccountsStruct) (c : Ctx) : Prop :=
  c.length = s.fields.length

instance (s : AccountsStruct) (c : Ctx) : Decidable (WellFormed s c) :=
  inferInstanceAs (Decidable (c.length = s.fields.length))

end VerifiedAnchor
