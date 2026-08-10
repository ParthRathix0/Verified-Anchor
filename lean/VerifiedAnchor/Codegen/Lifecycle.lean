import VerifiedAnchor.Contract.Satisfies

namespace VerifiedAnchor

/-- Update the account at index `i` (no-op if out of range), preserving `instrData`. -/
def Ctx.update (c : Ctx) (i : Nat) (g : AccountInfo → AccountInfo) : Ctx :=
  match c.accounts[i]? with
  | some a => { c with accounts := c.accounts.set i (g a) }
  | none => c

/-- Model of Anchor `init`: system create_account funded by `payer`, then discriminator
    write. Fails (none) unless idx≠payerIdx, both in range, payer signer+writable with
    ≥rent lamports, target empty. Effect: target gets owner, data=disc++zeros to size
    (space+8), +rent lamports; payer loses rent. -/
def applyInit (idx payerIdx : Nat) (space : Nat) (owner : Pubkey) (disc : ByteArray)
    (rent : UInt64) (c : Ctx) : Option Ctx :=
  if idx = payerIdx then none else
  match c.accounts[idx]?, c.accounts[payerIdx]? with
  | some a, some p =>
    if p.isSigner = true ∧ p.isWritable = true ∧ rent ≤ p.lamports ∧ a.data.size = 0 then
      let newData := disc ++ ByteArray.mk (Array.replicate (space + 8 - disc.size) 0)
      let c1 := c.update idx (fun a => { a with owner := owner, data := newData, lamports := a.lamports + rent })
      some (c1.update payerIdx (fun p => { p with lamports := p.lamports - rent }))
    else none
  | _, _ => none

/-- Model of Anchor `close`: move all target lamports to `dest`, write the closed marker. -/
def applyClose (idx destIdx : Nat) (c : Ctx) : Option Ctx :=
  if idx = destIdx then none else
  match c.accounts[idx]?, c.accounts[destIdx]? with
  | some a, some _ =>
    let c1 := c.update destIdx (fun d => { d with lamports := d.lamports + a.lamports })
    some (c1.update idx (fun a => { a with lamports := 0, data := closedAccountDiscriminator }))
  | _, _ => none

/-- Resize `d` to total length `L`: keep the first `min d.size L` bytes, zero-fill the rest.
    `size = L` by construction; the grown tail is `replicate _ 0`. -/
def resize (d : ByteArray) (L : Nat) : ByteArray :=
  d.extract 0 (min d.size L) ++ ByteArray.mk (Array.replicate (L - min d.size L) 0)

/-- Model of Anchor `realloc`: resize the target's data to `newLen`, TOP UP its lamports from
    `payer` to the rent-exempt minimum, and zero-fill the grown region. Funding is
    **top-up-only and surplus-preserving**: the modelled effect is
    `lamports' = max lamports (rentExemptMinimum newLen)`, so the account is NEVER debited and
    any surplus is kept. A realloc that needs no top-up (already rent-exempt for `newLen` —
    every shrink and every over-funded account) SUCCEEDS and preserves lamports (`delta = 0`).
    Fails (none) unless idx≠payerIdx, both in range, payer is a writable signer, and the payer
    can cover the (possibly zero) top-up. Formulated around `max`/subtraction of the smaller
    from the larger, so no `UInt64` op ever underflows. -/
def applyRealloc (idx payerIdx newLen : Nat) (zero : Bool) (c : Ctx) : Option Ctx :=
  if idx = payerIdx then none else
  match c.accounts[idx]?, c.accounts[payerIdx]? with
  | some a, some p =>
    let target := max a.lamports (rentExemptMinimum newLen)  -- surplus-preserving; never below min
    let delta  := target - a.lamports                        -- 0 when already exempt; never underflows
    if p.isSigner = true ∧ p.isWritable = true ∧ delta ≤ p.lamports then
      let c1 := c.update idx (fun a => { a with data := resize a.data newLen, lamports := target })
      some (c1.update payerIdx (fun p => { p with lamports := p.lamports - delta }))
    else none
  | _, _ => none

/-- Model of Anchor `init_if_needed`: if the account is uninitialized (all-zero discriminator),
    run `applyInit`; otherwise accept it ONLY if it is already a valid, sufficiently-sized,
    program-owned account (else `none` — the reinit guard). Both success branches establish the
    same `init` post-condition (owned by `owner`, data ≥ space+8). -/
def applyInitIfNeeded (idx payerIdx space : Nat) (owner : Pubkey) (disc : ByteArray)
    (rent : UInt64) (c : Ctx) : Option Ctx :=
  match c.accounts[idx]? with
  | some a =>
    if isZeroDisc a then applyInit idx payerIdx space owner disc rent c
    else if a.owner = owner ∧ space + 8 ≤ a.data.size then some c
    else none
  | none => none

/-- Read-back lemma for `Ctx.update`: an index reads through `g` exactly when it is the
    updated index (and stays in range), otherwise it is untouched. -/
theorem Ctx.accounts_getElem?_update (c : Ctx) (i j : Nat) (g : AccountInfo → AccountInfo) :
    (c.update j g).accounts[i]? = if i = j then (c.accounts[i]?).map g else c.accounts[i]? := by
  unfold Ctx.update
  cases hj : c.accounts[j]? with
  | none =>
    have : ¬ j < c.length := by
      intro hlt; rw [List.getElem?_eq_getElem hlt] at hj; exact (Option.some_ne_none _) hj
    by_cases hij : i = j
    · subst hij; simp [hj]
    · simp [hij]
  | some a =>
    by_cases hij : i = j
    · subst hij
      have hlt : i < c.length := by
        rw [List.getElem?_eq_some_iff] at hj; exact hj.1
      simp [List.getElem?_set_self hlt, hj]
    · simp [List.getElem?_set_ne (Ne.symm hij), hij]

/-- `applyInit` establishes the M1 `init` post-condition for the target account:
    it exists, is owned by `owner`, and has data of size at least `space + 8`. -/
theorem init_establishes_post
    (idx payerIdx space owner disc rent c c') (hne : idx ≠ payerIdx) (hdisc : disc.size = 8)
    (h : applyInit idx payerIdx space owner disc rent c = some c') :
    ∃ a, c'.accounts[idx]? = some a ∧ a.owner = owner ∧ space + 8 ≤ a.data.size := by
  simp only [applyInit, if_neg hne] at h
  split at h
  · next a p ha hp =>
    split at h
    · next hguard =>
      -- h : some (...) = some c'
      injection h with hc'
      subst hc'
      -- read back idx through the two updates: outer at payerIdx (skip), inner at idx (hit)
      rw [Ctx.accounts_getElem?_update, if_neg hne, Ctx.accounts_getElem?_update, if_pos rfl, ha,
        Option.map_some]
      -- witness is now pinned by `rfl`; owner is `rfl`, data size remains
      refine ⟨_, rfl, rfl, ?_⟩
      -- data size: disc ++ replicate (space+8-disc.size) 0
      show space + 8 ≤ (disc ++ ByteArray.mk (Array.replicate (space + 8 - disc.size) 0)).size
      rw [ByteArray.size_append, hdisc]
      show space + 8 ≤ 8 + (Array.replicate (space + 8 - 8) 0).size
      rw [Array.size_replicate]
      omega
    · exact absurd h (by simp)
  · exact absurd h (by simp)

/-- `applyClose` establishes the M1 `close` post-condition for the target account:
    it exists, has zero lamports, and carries the closed-account discriminator. -/
theorem close_establishes_post
    (idx destIdx c c') (hne : idx ≠ destIdx)
    (h : applyClose idx destIdx c = some c') :
    ∃ a, c'.accounts[idx]? = some a ∧ a.lamports = 0 ∧ hasDiscriminator a closedAccountDiscriminator := by
  simp only [applyClose, if_neg hne] at h
  split at h
  · next a d ha hd =>
    injection h with hc'
    subst hc'
    -- read back idx: outer update is at idx (hit), inner at destIdx (skip)
    rw [Ctx.accounts_getElem?_update, if_pos rfl, Ctx.accounts_getElem?_update, if_neg hne, ha,
      Option.map_some]
    -- witness pinned by `rfl`; lamports is `rfl`, discriminator remains
    refine ⟨_, rfl, rfl, ?_⟩
    -- data = closedAccountDiscriminator, so prefix agrees with itself
    unfold hasDiscriminator bytesAgreePrefix
    intro i _
    rfl
  · exact absurd h (by simp)

/-- `resize d L` always has size exactly `L` (the extract keeps `min d.size L` bytes, the
    zero-fill supplies the remaining `L - min d.size L`). -/
theorem resize_size (d : ByteArray) (L : Nat) : (resize d L).size = L := by
  unfold resize
  rw [ByteArray.size_append, ByteArray.size_extract]
  show min (min d.size L) d.size - 0 + (Array.replicate (L - min d.size L) 0).size = L
  rw [Array.size_replicate]
  omega

-- `UInt64.max` order facts. The `Max UInt64` instance is `maxOfLe`, so
-- `max a b = if a ≤ b then b else a` definitionally; these are the `le_max_left`/`le_max_right`/
-- `max_eq_left` facts, reproven from `UInt64` order primitives (no Mathlib in scope).

/-- The right argument is `≤` the `UInt64` max. -/
theorem UInt64.le_max_right (a b : UInt64) : b ≤ max a b := by
  show b ≤ if a ≤ b then b else a
  split
  · exact UInt64.le_rfl
  · rename_i h; exact (UInt64.le_total a b).resolve_left h

/-- The left argument is `≤` the `UInt64` max. -/
theorem UInt64.le_max_left (a b : UInt64) : a ≤ max a b := by
  show a ≤ if a ≤ b then b else a
  split
  · assumption
  · exact UInt64.le_rfl

/-- When the left argument dominates, the `UInt64` max is the left argument (surplus preserved). -/
theorem UInt64.max_eq_left {a b : UInt64} (h : b ≤ a) : max a b = a := by
  show (if a ≤ b then b else a) = a
  split
  · rename_i hab; exact UInt64.le_antisymm h hab
  · rfl

/-- `applyRealloc` establishes the M9 `realloc` post-condition for the target account:
    it exists, has data of size exactly `newLen`, is rent-exempt (holds at least
    `rentExemptMinimum newLen` lamports), and — the safety property — is **never debited**:
    its post-lamports are `≥` its pre-lamports (`aPre` is the pre-state account read back from
    the hypotheses). Top-up-only and surplus-preserving: post-lamports are
    `max aPre.lamports (rentExemptMinimum newLen)`, so both the rent bound (`le_max_right`) and
    the never-debited bound (`le_max_left`) hold with no subtraction/underflow. Schematic over
    the opaque `rentExemptMinimum` — it never reduces. -/
theorem realloc_establishes_post
    (idx payerIdx newLen) (zero : Bool) (c c' aPre) (hne : idx ≠ payerIdx)
    (hpre : c.accounts[idx]? = some aPre)
    (h : applyRealloc idx payerIdx newLen zero c = some c') :
    ∃ a, c'.accounts[idx]? = some a ∧
      a.data.size = newLen ∧
      rentExemptMinimum newLen ≤ a.lamports ∧
      aPre.lamports ≤ a.lamports := by
  simp only [applyRealloc, if_neg hne] at h
  split at h
  · next a p ha hp =>
    -- pin the pre-state account: `ha` and `hpre` agree, so `a = aPre`
    rw [hpre] at ha; injection ha with ha; subst ha
    split at h
    · next hguard =>
      injection h with hc'
      subst hc'
      -- read back idx through the two updates: outer at payerIdx (skip), inner at idx (hit)
      rw [Ctx.accounts_getElem?_update, if_neg hne, Ctx.accounts_getElem?_update, if_pos rfl, hpre,
        Option.map_some]
      refine ⟨_, rfl, ?_, ?_, ?_⟩
      · -- size: resize aPre.data newLen has size newLen by construction
        exact resize_size aPre.data newLen
      · -- rent: rentExemptMinimum newLen ≤ max aPre.lamports (rentExemptMinimum newLen)
        exact UInt64.le_max_right aPre.lamports (rentExemptMinimum newLen)
      · -- never debited: aPre.lamports ≤ max aPre.lamports (rentExemptMinimum newLen)
        exact UInt64.le_max_left aPre.lamports (rentExemptMinimum newLen)
    · exact absurd h (by simp)
  · exact absurd h (by simp)

/-- **No-top-up ⇒ SUCCEED + PRESERVE (regression witness for finding #1).** When the target is
    already rent-exempt for `newLen` (`rentExemptMinimum newLen ≤ a.lamports` — every shrink and
    every over-funded account), `applyRealloc` returns `some` (given a writable-signer payer, no
    balance requirement since the top-up is 0) and the target's lamports are UNCHANGED, i.e. the
    account is not debited and its surplus is preserved. This is exactly the case the old
    underflowing model wrongly rejected. Schematic over the opaque `rentExemptMinimum`. -/
theorem applyRealloc_noTopUp_succeeds
    (idx payerIdx newLen) (zero : Bool) (c) (a p) (hne : idx ≠ payerIdx)
    (ha : c.accounts[idx]? = some a) (hp : c.accounts[payerIdx]? = some p)
    (hsign : p.isSigner = true) (hwrite : p.isWritable = true)
    (hexempt : rentExemptMinimum newLen ≤ a.lamports) :
    ∃ c' a', applyRealloc idx payerIdx newLen zero c = some c' ∧
      c'.accounts[idx]? = some a' ∧
      a'.lamports = a.lamports ∧             -- SUCCEEDS with lamports PRESERVED (not debited)
      a'.data.size = newLen := by
  have hmax : max a.lamports (rentExemptMinimum newLen) = a.lamports := UInt64.max_eq_left hexempt
  simp only [applyRealloc, if_neg hne, ha, hp, hmax, hsign, hwrite, UInt64.sub_self,
    UInt64.zero_le, and_self, if_pos]
  -- name the resulting ctx; read idx back through the two updates to expose the witness `a'`
  refine ⟨_, { a with data := resize a.data newLen }, rfl, ?_, rfl, ?_⟩
  · -- read back idx: outer at payerIdx (skip), inner at idx (hit)
    rw [Ctx.accounts_getElem?_update, if_neg hne, Ctx.accounts_getElem?_update, if_pos rfl, ha,
      Option.map_some]
  · -- data resized to newLen
    exact resize_size a.data newLen

-- Closed-loop demonstration: `resize` truncates/grows to the exact requested length.
#guard (resize (ByteArray.mk (Array.replicate 4 0)) 10).size = 10   -- grow
#guard (resize (ByteArray.mk (Array.replicate 40 0)) 8).size = 8    -- shrink

/-- Concrete surplus/shrink witness for finding #1: an over-funded, writable-signer payer and a
    target holding more than any `rentExemptMinimum` (its lamports `⊔ m = its lamports` for the
    minimum `m` it already dominates). `applyRealloc` returns `some` and the target keeps its
    lamports. `rentExemptMinimum` is opaque so this cannot reduce under `#eval`; it is discharged
    as an `example` from `applyRealloc_noTopUp_succeeds`. -/
example (c : Ctx) (a p : AccountInfo)
    (ha : c.accounts[0]? = some a) (hp : c.accounts[1]? = some p)
    (hsign : p.isSigner = true) (hwrite : p.isWritable = true)
    (hexempt : rentExemptMinimum 8 ≤ a.lamports) :
    ∃ c' a', applyRealloc 0 1 8 true c = some c' ∧
      c'.accounts[0]? = some a' ∧ a'.lamports = a.lamports ∧ a'.data.size = 8 :=
  applyRealloc_noTopUp_succeeds 0 1 8 true c a p (by decide) ha hp hsign hwrite hexempt

/-- `applyInitIfNeeded` establishes the SAME `init` post-condition for the target account in BOTH
    success branches: it exists, is owned by `owner`, and has data of size at least `space + 8`.
    The uninitialized branch delegates to `init_establishes_post`; the existing-account branch is
    the reinit guard — it only succeeds when the account is already program-owned and large enough,
    so the post holds with no external hypothesis (a wrong-owner account returns `none`). -/
theorem initIfNeeded_establishes_post
    (idx payerIdx space owner disc rent c c') (hne : idx ≠ payerIdx) (hdisc : disc.size = 8)
    (h : applyInitIfNeeded idx payerIdx space owner disc rent c = some c') :
    ∃ a, c'.accounts[idx]? = some a ∧ a.owner = owner ∧ space + 8 ≤ a.data.size := by
  unfold applyInitIfNeeded at h
  split at h
  · next a hpre =>
    split at h
    · -- uninitialized: applyInit … = some c'; reuse init_establishes_post wholesale
      exact init_establishes_post idx payerIdx space owner disc rent c c' hne hdisc h
    · -- existing account: split on the owner+size guard
      split at h
      · next hguard =>
        -- guard passed: some c = some c', so c = c'; witness is the existing account `a`
        injection h with hc'
        subst hc'
        exact ⟨a, hpre, hguard.1, hguard.2⟩
      · exact absurd h (by simp)
  · exact absurd h (by simp)

-- Axiom audit (run manually to confirm; kept as a comment so the build stays quiet):
--   #print axioms realloc_establishes_post  ⇒  'depends on axioms: [propext, Quot.sound]'
--   #print axioms initIfNeeded_establishes_post  ⇒  'depends on axioms: [propext, Quot.sound]'

end VerifiedAnchor
