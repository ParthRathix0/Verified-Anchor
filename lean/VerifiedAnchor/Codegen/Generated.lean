import VerifiedAnchor.Contract.Validates

/-- `o` holds a value satisfying the Bool predicate `p` (false if `o` is none).

    Defined in the root `Option` namespace (mirroring `Option.satisfiesSome`) so that the
    `o.allB p` dot-notation resolves against the real `Option` type. -/
def Option.allB {α} (o : Option α) (p : α → Bool) : Bool :=
  match o with | none => false | some a => p a

namespace VerifiedAnchor

/-- Relational has_one check: the Pubkey at the field's layout offset in this account's data
    equals the looked-up field account's key. None-safe. -/
def genHasOne (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) (field : String) : Bool :=
  (Ctx.atField s c idx).allB (fun a =>
    (f.ty.layoutOffsetOf field).allB (fun off =>
      (readPubkey a.data off).allB (fun val =>
        (Ctx.lookup s c field).allB (fun target => decide (val = target.key)))))

/-- Bool mirror of `bumpMatches`: declared bumps must match exactly; canonical accepts any. -/
def bumpMatchesB : BumpSpec → UInt8 → Bool
  | .declared db, actual => actual == db
  | .canonical,  _       => true

/-- Operational PDA check: derive the canonical PDA from the resolved seeds and the
    program id, require it equals the account key, and the bump matches. None-safe. -/
def genSeeds (s : AccountsStruct) (c : Ctx) (idx : Nat)
    (ss : List SeedSpec) (b : BumpSpec) : Bool :=
  (Ctx.atField s c idx).allB (fun a =>
    (findProgramAddress (resolveSeeds s c ss) s.programId).allB (fun pr =>
      decide (pr.1 = a.key) && bumpMatchesB b pr.2))

/-- Operational check of one constraint, resolving accounts from the full context.
    (init/close are not validation constraints → false.) -/
def genConstraint (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) :
    Constraint → Bool
  | .signer          => (Ctx.atField s c idx).allB (fun a => a.isSigner)
  | .mut             => (Ctx.atField s c idx).allB (fun a => a.isWritable)
  | .owner e         => (Ctx.atField s c idx).allB (fun a => decide (a.owner = e))
  | .executable      => (Ctx.atField s c idx).allB (fun a => a.executable)
  | .address e       => (Ctx.atField s c idx).allB (fun a => decide (a.key = e))
  | .discriminator d => (Ctx.atField s c idx).allB (fun a => decide (hasDiscriminator a d))
  | .hasOne field    => genHasOne s c idx f field
  | .seeds ss b      => genSeeds s c idx ss b
  | _                => false

def genFieldValidate (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) : Bool :=
  (f.ty.impliedConstraints ++ f.constraints).all (genConstraint s c idx f)

def genValidate (s : AccountsStruct) (c : Ctx) : Bool :=
  decide (c.length = s.fields.length) &&
    s.fields.zipIdx.all (fun p => genFieldValidate s c p.2 p.1)

end VerifiedAnchor
