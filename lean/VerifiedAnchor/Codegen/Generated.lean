import VerifiedAnchor.Contract.Validates
import VerifiedAnchor.Solana.Rent

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

/-- Bool mirror of `bumpMatches`: declared bumps must match exactly; canonical accepts any.
    `.stored` does not go through `bumpMatchesB` (its derivation IS the check); the `true` here
    only keeps the function total, mirroring `bumpMatches`. -/
def bumpMatchesB : BumpSpec → UInt8 → Bool
  | .declared db, actual => actual == db
  | .canonical,  _       => true
  | .stored _,   _       => true

/-- Operational PDA check (Bool mirror of `satisfies (.seeds ss b program)`). The `program`
    override selects the derivation program id: `none` ⇒ `s.programId`, `some p` ⇒ the foreign
    `p`. For canonical/declared
    bumps: derive the canonical PDA from the resolved seeds and the program id, require it
    equals the account key, and the bump matches. For the opt-in `.stored off` bump: read the
    bump byte from instr data at `off`, derive the PDA with THAT specific bump via
    `createProgramAddress`, require it equals the account key — NO canonical requirement.
    None-safe throughout. -/
def genSeeds (s : AccountsStruct) (c : Ctx) (idx : Nat)
    (ss : List SeedSpec) (b : BumpSpec) (program : Option Pubkey) : Bool :=
  -- `seeds::program` override: derive against the FOREIGN id if given, else `s.programId`.
  let pid := program.getD s.programId
  (Ctx.atField s c idx).allB (fun a =>
    match b with
    | .stored off =>
        (c.instrData.data[off]?).allB (fun bb =>
          (createProgramAddress (resolveSeeds s c ss ++ [(⟨#[bb]⟩ : ByteArray)]) pid).allB
            (fun pk => decide (pk = a.key)))
    | .declared _ | .canonical =>
        (findProgramAddress (resolveSeeds s c ss) pid).allB (fun pr =>
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
  | .rentExempt      => (Ctx.atField s c idx).allB (fun a => decide (rentExemptMinimum a.data.size ≤ a.lamports))
  | .discriminator d => (Ctx.atField s c idx).allB (fun a => decide (hasDiscriminator a d))
  | .hasOne field    => genHasOne s c idx f field
  | .seeds ss b program => genSeeds s c idx ss b program
  | _                => false

def genFieldValidate (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) : Bool :=
  (f.ty.impliedConstraints ++ f.constraints).all (genConstraint s c idx f)

/-- Bool mirror of `isMutField`: the field carries `mut` (type-implied or explicit). -/
def isMutFieldB (f : AccountField) : Bool :=
  (f.ty.impliedConstraints ++ f.constraints).any Constraint.isMut

/-- Bool mirror of `exemptPair`: either field lists the other in its `allowDuplicate` opt-out. -/
def exemptPairB (fi fj : AccountField) : Bool :=
  fi.allowDuplicate.contains fj.name || fj.allowDuplicate.contains fi.name

/-- Bool mirror of `distinctMutKeys`. For each ordered pair `p, q` with `p.2 < q.2` that are
    both `mut` and not exempt, require distinct keys; every other pair passes vacuously. The
    `decide (p.2 < q.2) → … ` guards are encoded as `!cond || rest`, mirroring the implication
    chain in `distinctMutKeys`. -/
def distinctMutKeysB (s : AccountsStruct) (c : Ctx) : Bool :=
  s.fields.zipIdx.all (fun p =>
    s.fields.zipIdx.all (fun q =>
      !(decide (p.2 < q.2) && isMutFieldB p.1 && isMutFieldB q.1 && !exemptPairB p.1 q.1) ||
        (Ctx.atField s c p.2).allB (fun a =>
          (Ctx.atField s c q.2).allB (fun b => decide (a.key ≠ b.key)))))

def genValidate (s : AccountsStruct) (c : Ctx) : Bool :=
  decide (c.length = s.fields.length) &&
    distinctMutKeysB s c &&
    s.fields.zipIdx.all (fun p => genFieldValidate s c p.2 p.1)

end VerifiedAnchor
