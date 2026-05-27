import VerifiedAnchor.Solana.Pubkey
import VerifiedAnchor.Solana.Layout
import VerifiedAnchor.Solana.Discriminator

namespace VerifiedAnchor

/-- A single seed in a PDA derivation. -/
inductive SeedSpec where
  | literal (bytes : ByteArray)        -- e.g. b"vault"
  | fieldKey (field : String)          -- another account's key bytes
  deriving Inhabited

inductive BumpSpec where
  | declared (b : UInt8)
  | canonical
  deriving Inhabited, DecidableEq

/-- The Anchor constraint subset in scope for v1. -/
inductive Constraint where
  | signer
  | mut
  | owner          (expected : Pubkey)
  | hasOne         (field : String)
  | seeds          (seeds : List SeedSpec) (bump : BumpSpec)
  | init           (payer : String) (space : Nat) (owner : Pubkey)
  | close          (dest : String)
  | discriminator  (expected : ByteArray)   -- 8 bytes
  deriving Inhabited

/-- Account wrapper types; each implies certain base constraints. -/
inductive AccountType where
  | account          (typeName : String) (layout : FieldLayout) (programId : Pubkey)
  | signer
  | program          (id : Pubkey)
  | systemAccount
  | uncheckedAccount
  deriving Inhabited

/-- Base constraints implied by the wrapper type, before explicit annotations. -/
def AccountType.impliedConstraints : AccountType → List Constraint
  | .account tn _ pid => [Constraint.owner pid, Constraint.discriminator (accountDiscriminator tn)]
  | .signer           => [Constraint.signer]
  | .program _        => []
  | .systemAccount    => []
  | .uncheckedAccount => []

/-- Look up the layout offset of a `Pubkey` field within an account type. -/
def AccountType.layoutOffsetOf : AccountType → String → Option Nat
  | .account _ layout _, name => layout.offsetOf name
  | _, _ => none

structure AccountField where
  name        : String
  ty          : AccountType
  constraints : List Constraint
  deriving Inhabited

structure AccountsStruct where
  programId : Pubkey
  fields    : List AccountField
  deriving Inhabited

/-- Find a declared field by name. -/
def AccountsStruct.fieldNamed (s : AccountsStruct) (name : String) : Option AccountField :=
  s.fields.find? (·.name == name)

end VerifiedAnchor
