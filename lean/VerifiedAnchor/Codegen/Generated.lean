import VerifiedAnchor.Contract.Validates

namespace VerifiedAnchor

/-- Per-constraint Bool checks, exactly transcribing the generated Rust `if`s. -/
def genSigner (a : AccountInfo) : Bool := a.isSigner
def genMut    (a : AccountInfo) : Bool := a.isWritable
def genOwner  (expected : Pubkey) (a : AccountInfo) : Bool := decide (a.owner = expected)

/-- Operational check of one M2 constraint against the resolved account. Constraints
    outside the M2 subset are not generated, so they return `false` here. -/
def genConstraint (a : AccountInfo) : Constraint → Bool
  | .signer  => genSigner a
  | .mut     => genMut a
  | .owner e => genOwner e a
  | _        => false

/-- The generated per-field check: resolve the account, then every (implied ++ explicit)
    constraint must pass. A missing account fails. -/
def genFieldValidate (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) : Bool :=
  match Ctx.atField s c idx with
  | none   => false
  | some a => (f.ty.impliedConstraints ++ f.constraints).all (genConstraint a)

/-- The generated validator: well-formed account count, then every field validates.
    Mirrors the emitted Rust `validate` (positional, short-circuiting). -/
def genValidate (s : AccountsStruct) (c : Ctx) : Bool :=
  decide (c.length = s.fields.length) &&
    s.fields.zipIdx.all (fun p => genFieldValidate s c p.2 p.1)

end VerifiedAnchor
