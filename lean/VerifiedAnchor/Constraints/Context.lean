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

/-- Structural well-formedness: at least one account per declared field.

    A PREFIX condition, not an exact count, and deliberately so. Anchor passes surplus accounts
    through to `ctx.remaining_accounts`, so a framework that rejected them would not be a
    drop-in replacement; the generated Rust accordingly guards only `accounts.len() < n`. This
    used to read `c.length = s.fields.length`, which made the contract claim something the
    generated code does not enforce and forced the soundness statement to carry a caveat.

    Nothing is weakened by the relaxation, because nothing ever looked at a surplus account:
    every per-field check and the distinct-mut-key check range over `s.fields.zipIdx`, i.e. the
    DECLARED prefix only. Accounts at index ≥ `s.fields.length` were unconstrained under the
    equality too. The change is to what the contract CLAIMS, not to what it CHECKS — and it is
    what makes the soundness guarantee unconditional rather than qualified. -/
def WellFormed (s : AccountsStruct) (c : Ctx) : Prop :=
  s.fields.length ≤ c.length

instance (s : AccountsStruct) (c : Ctx) : Decidable (WellFormed s c) :=
  inferInstanceAs (Decidable (s.fields.length ≤ c.length))

end VerifiedAnchor
