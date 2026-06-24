import VerifiedAnchor.Constraints.Context
import VerifiedAnchor.Solana.Crypto
import VerifiedAnchor.Solana.Rent

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

/-- Whether an actual bump matches a declared bump spec (canonical accepts anything).
    The `.stored` opt-in does NOT go through `bumpMatches` (its PDA-derivation IS the check),
    so this case is never reached from `satisfies`; it accepts (`True`) only to stay total. -/
def bumpMatches : BumpSpec → UInt8 → Prop
  | .declared db, actual => actual = db
  | .canonical, _ => True
  | .stored _, _ => True

instance (b : BumpSpec) (actual : UInt8) : Decidable (bumpMatches b actual) :=
  match b with
  | .declared db => inferInstanceAs (Decidable (actual = db))
  | .canonical => isTrue trivial
  | .stored _ => isTrue trivial

/-- Resolve a list of seed specs against the context into concrete seed bytes. -/
def resolveSeeds (s : AccountsStruct) (c : Ctx) : List SeedSpec → List ByteArray
  | [] => []
  | .literal bytes :: rest => bytes :: resolveSeeds s c rest
  | .fieldKey name :: rest =>
      (match Ctx.lookup s c name with
       | some a => ByteArray.mk a.key.toArray
       | none => ByteArray.empty) :: resolveSeeds s c rest
  | .instrArg off len :: rest =>
      c.instrData.extract off (off + len) :: resolveSeeds s c rest

/-- Anchor's `CLOSED_ACCOUNT_DISCRIMINATOR`: 8 bytes of `0xff` written to a closed
    account's data so it can never be re-deserialized as a live account. -/
def closedAccountDiscriminator : ByteArray :=
  (⟨#[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]⟩ : ByteArray)

/-- What it means for the account at field index `idx` (declared field `f`) to satisfy one
    constraint, given the whole struct `s` and runtime accounts `c`. -/
def satisfies (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) :
    Constraint → Prop
  | .signer => (Ctx.atField s c idx).satisfiesSome (fun a => a.isSigner = true)
  | .mut    => (Ctx.atField s c idx).satisfiesSome (fun a => a.isWritable = true)
  | .owner expected => (Ctx.atField s c idx).satisfiesSome (fun a => a.owner = expected)
  | .executable => (Ctx.atField s c idx).satisfiesSome (fun a => a.executable = true)
  | .address expected => (Ctx.atField s c idx).satisfiesSome (fun a => a.key = expected)
  | .rentExempt =>
      -- The account's balance covers the (opaque) rent-exempt minimum for its data size.
      -- `≤` on `UInt64` is decidable, so the `Decidable (satisfies)` instance still derives.
      (Ctx.atField s c idx).satisfiesSome (fun a => rentExemptMinimum a.data.size ≤ a.lamports)
  | .discriminator d => (Ctx.atField s c idx).satisfiesSome (fun a => hasDiscriminator a d)
  | .hasOne field =>
      (Ctx.atField s c idx).satisfiesSome (fun a =>
        (f.ty.layoutOffsetOf field).satisfiesSome (fun off =>
          (readPubkey a.data off).satisfiesSome (fun val =>
            (Ctx.lookup s c field).satisfiesSome (fun target => val = target.key))))
  | .seeds ss b program =>
      -- `seeds::program` override: derive against the FOREIGN id `program` if given, else the
      -- struct's own `s.programId`. `getD` is definitional, so the soundness proof is unchanged.
      let pid := program.getD s.programId
      (Ctx.atField s c idx).satisfiesSome (fun a =>
        match b with
        | .stored off =>
            -- Opt-in non-canonical bump: read the bump byte from instr data at `off`, derive
            -- the PDA with THAT specific bump, require it equals the account key. No canonical
            -- (`findProgramAddress`) requirement — the deliberate, less-safe opt-in. None-safe:
            -- an out-of-range `off` (or on-curve derivation) leaves the spec unsatisfied.
            (c.instrData.data[off]?).satisfiesSome (fun bb =>
              (createProgramAddress (resolveSeeds s c ss ++ [(⟨#[bb]⟩ : ByteArray)]) pid).satisfiesSome
                (fun pk => pk = a.key))
        | .declared _ | .canonical =>
            (findProgramAddress (resolveSeeds s c ss) pid).satisfiesSome (fun pr =>
              pr.1 = a.key ∧ bumpMatches b pr.2))
  | .init payer space owner =>
      (Ctx.atField s c idx).satisfiesSome (fun a =>
        (Ctx.lookup s c payer).satisfiesSome (fun p =>
          a.owner = owner ∧ p.isSigner = true ∧ p.isWritable = true ∧ space + 8 ≤ a.data.size))
  | .close dest =>
      (Ctx.atField s c idx).satisfiesSome (fun a =>
        (Ctx.lookup s c dest).satisfiesSome (fun _ => True) ∧
          a.lamports = 0 ∧ hasDiscriminator a closedAccountDiscriminator)

/-- The contract is decidable, constraint by constraint. Load-bearing for `validatesBool`.
    The `.seeds` case must also split on the `BumpSpec` so the inner `match b with` reduces to
    a concrete (decidable) predicate before `infer_instance`. -/
instance (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) (k : Constraint) :
    Decidable (satisfies s c idx f k) := by
  cases k with
  | seeds ss b program => cases b <;> simp only [satisfies] <;> infer_instance
  | _ => simp only [satisfies] <;> infer_instance

end VerifiedAnchor
