import VerifiedAnchor.Constraints.Context
import VerifiedAnchor.Solana.Crypto

/-- `o` holds some value satisfying `P`. The single combinator all constraint cases are
    built from, so decidability is compositional.

    Defined in the root `Option` namespace (not under `VerifiedAnchor`) so that the
    `o.satisfiesSome P` dot-notation resolves against the real `Option` type. -/
def Option.satisfiesSome {α : Type _} (o : Option α) (P : α → Prop) : Prop :=
  ∃ a, o = some a ∧ P a

instance {α : Type _} (o : Option α) (P : α → Prop) [∀ a, Decidable (P a)] :
    Decidable (o.satisfiesSome P) :=
  match o with
  | none => isFalse (by simp [Option.satisfiesSome])
  | some a => decidable_of_iff (P a) (by simp [Option.satisfiesSome])

namespace VerifiedAnchor

/-- Whether an actual bump matches a declared bump spec (canonical accepts anything). -/
def bumpMatches : BumpSpec → UInt8 → Prop
  | .declared db, actual => actual = db
  | .canonical, _ => True

instance (b : BumpSpec) (actual : UInt8) : Decidable (bumpMatches b actual) :=
  match b with
  | .declared db => inferInstanceAs (Decidable (actual = db))
  | .canonical => isTrue trivial

/-- Resolve a list of seed specs against the context into concrete seed bytes. -/
def resolveSeeds (s : AccountsStruct) (c : Ctx) : List SeedSpec → List ByteArray
  | [] => []
  | .literal bytes :: rest => bytes :: resolveSeeds s c rest
  | .fieldKey name :: rest =>
      (match Ctx.lookup s c name with
       | some a => ByteArray.mk a.key.toArray
       | none => ByteArray.empty) :: resolveSeeds s c rest

/-- What it means for the account at field index `idx` (declared field `f`) to satisfy one
    constraint, given the whole struct `s` and runtime accounts `c`. -/
def satisfies (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) :
    Constraint → Prop
  | .signer => (Ctx.atField s c idx).satisfiesSome (fun a => a.isSigner = true)
  | .mut    => (Ctx.atField s c idx).satisfiesSome (fun a => a.isWritable = true)
  | .owner expected => (Ctx.atField s c idx).satisfiesSome (fun a => a.owner = expected)
  | .discriminator d => (Ctx.atField s c idx).satisfiesSome (fun a => hasDiscriminator a d)
  | .hasOne field =>
      (Ctx.atField s c idx).satisfiesSome (fun a =>
        (f.ty.layoutOffsetOf field).satisfiesSome (fun off =>
          (readPubkey a.data off).satisfiesSome (fun val =>
            (Ctx.lookup s c field).satisfiesSome (fun target => val = target.key))))
  | .seeds ss b =>
      (Ctx.atField s c idx).satisfiesSome (fun a =>
        (findProgramAddress (resolveSeeds s c ss) s.programId).satisfiesSome (fun pr =>
          pr.1 = a.key ∧ bumpMatches b pr.2))
  | .init payer _space owner =>
      (Ctx.atField s c idx).satisfiesSome (fun a =>
        (Ctx.lookup s c payer).satisfiesSome (fun p =>
          a.owner = owner ∧ p.isSigner = true ∧ p.isWritable = true ∧ _space + 8 ≤ a.data.size))
  | .close dest =>
      (Ctx.atField s c idx).satisfiesSome (fun a =>
        (Ctx.lookup s c dest).satisfiesSome (fun _ => True) ∧ a.lamports = 0)

/-- The contract is decidable, constraint by constraint. Load-bearing for `validatesBool`. -/
instance (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) (k : Constraint) :
    Decidable (satisfies s c idx f k) := by
  cases k <;> simp only [satisfies] <;> infer_instance

end VerifiedAnchor
