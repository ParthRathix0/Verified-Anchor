import VerifiedAnchor.Codegen.Lifecycle

namespace VerifiedAnchor

/-- The fixed 8-byte discriminator the codegen writes on init. -/
def initDisc : ByteArray := ByteArray.mk (Array.replicate 8 0)

/-- Post-condition obligation for one constraint at field index `idx` of struct `s`. -/
def lifecyclePost (s : AccountsStruct) (idx : Nat) : Constraint → Prop
  | .init payerName space owner =>
      ∀ payerIdx, List.findIdx? (·.name == payerName) s.fields = some payerIdx →
        ∀ rent c c', applyInit idx payerIdx space owner initDisc rent c = some c' →
          ∃ a, c'.accounts[idx]? = some a ∧ a.owner = owner ∧ space + 8 ≤ a.data.size
  | .close destName =>
      ∀ destIdx, List.findIdx? (·.name == destName) s.fields = some destIdx →
        ∀ c c', applyClose idx destIdx c = some c' →
          ∃ a, c'.accounts[idx]? = some a ∧ a.lamports = 0 ∧ hasDiscriminator a closedAccountDiscriminator
  | .realloc payerName newLen z =>
      ∀ payerIdx, List.findIdx? (·.name == payerName) s.fields = some payerIdx →
        ∀ c c' aPre, c.accounts[idx]? = some aPre →
          applyRealloc idx payerIdx newLen z c = some c' →
            ∃ a, c'.accounts[idx]? = some a ∧ a.data.size = newLen ∧
              rentExemptMinimum newLen ≤ a.lamports ∧ aPre.lamports ≤ a.lamports
  | .initIfNeeded payerName space owner =>
      ∀ payerIdx, List.findIdx? (·.name == payerName) s.fields = some payerIdx →
        ∀ rent c c', applyInitIfNeeded idx payerIdx space owner initDisc rent c = some c' →
          ∃ a, c'.accounts[idx]? = some a ∧ a.owner = owner ∧ space + 8 ≤ a.data.size
  | _ => True

/-- Decidable well-formedness: each init/close clause resolves payer/dest to a DIFFERENT
    index than the field itself (so the applyInit/applyClose idx≠… guard holds). -/
def lifecycleClauseWF (s : AccountsStruct) (idx : Nat) : Constraint → Bool
  | .init payerName _ _ =>
      match List.findIdx? (·.name == payerName) s.fields with
      | some pi => decide (idx ≠ pi)
      | none => true
  | .close destName =>
      match List.findIdx? (·.name == destName) s.fields with
      | some di => decide (idx ≠ di)
      | none => true
  | .realloc payerName _ _ =>
      match List.findIdx? (·.name == payerName) s.fields with
      | some pi => decide (idx ≠ pi)
      | none => true
  | .initIfNeeded payerName _ _ =>
      match List.findIdx? (·.name == payerName) s.fields with
      | some pi => decide (idx ≠ pi)
      | none => true
  | _ => true

def StructLifecycleWF (s : AccountsStruct) : Prop :=
  ∀ p ∈ s.fields.zipIdx, ∀ k ∈ p.1.constraints, lifecycleClauseWF s p.2 k = true

instance (s : AccountsStruct) : Decidable (StructLifecycleWF s) := by
  unfold StructLifecycleWF; infer_instance

/-- THE GENERIC LIFECYCLE THEOREM: one decidable well-formedness predicate implies the M1
    init/close post-conditions for every lifecycle field. Per-struct checking is then just
    `decide (StructLifecycleWF spec)`. -/
theorem lifecycle_sound (s : AccountsStruct) (h : StructLifecycleWF s) :
    ∀ p ∈ s.fields.zipIdx, ∀ k ∈ p.1.constraints, lifecyclePost s p.2 k := by
  intro p hp k hk
  have hwf := h p hp k hk
  cases k with
  | init payerName space owner =>
    intro payerIdx hpayer rent c c' heff
    simp only [lifecycleClauseWF, hpayer] at hwf
    have hne : p.2 ≠ payerIdx := of_decide_eq_true hwf
    exact init_establishes_post p.2 payerIdx space owner initDisc rent c c' hne (by decide) heff
  | close destName =>
    intro destIdx hdest c c' heff
    simp only [lifecycleClauseWF, hdest] at hwf
    have hne : p.2 ≠ destIdx := of_decide_eq_true hwf
    exact close_establishes_post p.2 destIdx c c' hne heff
  | signer => trivial
  | «mut» => trivial
  | owner e => trivial
  | hasOne f => trivial
  | discriminator d => trivial
  | seeds ss b program => trivial
  | executable => trivial
  | address e => trivial
  | rentExempt => trivial
  | realloc payerName newLen z =>
    intro payerIdx hpayer c c' aPre hpre heff
    simp only [lifecycleClauseWF, hpayer] at hwf
    have hne : p.2 ≠ payerIdx := of_decide_eq_true hwf
    exact realloc_establishes_post p.2 payerIdx newLen z c c' aPre hne hpre heff
  | zero => trivial
  | initIfNeeded payerName space owner =>
    intro payerIdx hpayer rent c c' heff
    simp only [lifecycleClauseWF, hpayer] at hwf
    have hne : p.2 ≠ payerIdx := of_decide_eq_true hwf
    exact initIfNeeded_establishes_post p.2 payerIdx space owner initDisc rent c c' hne (by decide) heff

/-- Sanity: a struct whose `init` payer resolves to a different field is well-formed; one
    whose payer resolves to itself is not. (Crypto-free, so `decide` reduces.) -/
private def egInitGood : AccountsStruct :=
  { programId := Pubkey.zero
  , fields := [ { name := "new", ty := AccountType.uncheckedAccount,
                  constraints := [Constraint.init "payer" 0 Pubkey.zero] }
              , { name := "payer", ty := AccountType.uncheckedAccount, constraints := [] } ] }
private def egInitBad : AccountsStruct :=
  { programId := Pubkey.zero
  , fields := [ { name := "new", ty := AccountType.uncheckedAccount,
                  constraints := [Constraint.init "new" 0 Pubkey.zero] } ] }  -- payer = self
#guard decide (StructLifecycleWF egInitGood) = true
#guard decide (StructLifecycleWF egInitBad) = false

/-- Sanity: a struct whose `realloc` payer resolves to a different field is well-formed; one
    whose payer resolves to itself is not. (Crypto-free, so `decide` reduces.) -/
private def egReallocGood : AccountsStruct :=
  { programId := Pubkey.zero
  , fields := [ { name := "data", ty := AccountType.uncheckedAccount,
                  constraints := [Constraint.realloc "payer" 64 true] }
              , { name := "payer", ty := AccountType.uncheckedAccount, constraints := [] } ] }
private def egReallocBad : AccountsStruct :=
  { programId := Pubkey.zero
  , fields := [ { name := "data", ty := AccountType.uncheckedAccount,
                  constraints := [Constraint.realloc "data" 64 true] } ] }
#guard decide (StructLifecycleWF egReallocGood) = true
#guard decide (StructLifecycleWF egReallocBad) = false

/-- Sanity: a struct whose `init_if_needed` payer resolves to a different field is well-formed; one
    whose payer resolves to itself is not. (Crypto-free, so `decide` reduces.) -/
private def egIinGood : AccountsStruct :=
  { programId := Pubkey.zero
  , fields := [ { name := "acct", ty := AccountType.uncheckedAccount,
                  constraints := [Constraint.initIfNeeded "payer" 0 Pubkey.zero] }
              , { name := "payer", ty := AccountType.uncheckedAccount, constraints := [] } ] }
private def egIinBad : AccountsStruct :=
  { programId := Pubkey.zero
  , fields := [ { name := "acct", ty := AccountType.uncheckedAccount,
                  constraints := [Constraint.initIfNeeded "acct" 0 Pubkey.zero] } ] }  -- payer = self
#guard decide (StructLifecycleWF egIinGood) = true
#guard decide (StructLifecycleWF egIinBad) = false

end VerifiedAnchor
