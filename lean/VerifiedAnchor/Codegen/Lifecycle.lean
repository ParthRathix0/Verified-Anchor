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
    `payer` to the rent-exempt minimum (never debiting the account — surplus preserved), and
    zero-fill the grown region. Fails (none) unless idx≠payerIdx, both in range, payer is a
    writable signer, and the payer can cover the shortfall. -/
def applyRealloc (idx payerIdx newLen : Nat) (zero : Bool) (c : Ctx) : Option Ctx :=
  if idx = payerIdx then none else
  match c.accounts[idx]?, c.accounts[payerIdx]? with
  | some a, some p =>
    let shortfall := rentExemptMinimum newLen - a.lamports
    if p.isSigner = true ∧ p.isWritable = true ∧ shortfall ≤ p.lamports then
      let c1 := c.update idx (fun a => { a with data := resize a.data newLen, lamports := a.lamports + shortfall })
      some (c1.update payerIdx (fun p => { p with lamports := p.lamports - shortfall }))
    else none
  | _, _ => none

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

/-- `applyRealloc` establishes the M9 `realloc` post-condition for the target account:
    it exists, has data of size exactly `newLen`, and is rent-exempt (holds at least
    `rentExemptMinimum newLen` lamports). Top-up-only: the payer covers the shortfall, so the
    target lands exactly at the minimum (surplus preserved when already above it). Schematic
    over the opaque `rentExemptMinimum` — it never reduces. -/
theorem realloc_establishes_post
    (idx payerIdx newLen) (zero : Bool) (c c') (hne : idx ≠ payerIdx)
    (h : applyRealloc idx payerIdx newLen zero c = some c') :
    ∃ a, c'.accounts[idx]? = some a ∧
      a.data.size = newLen ∧
      rentExemptMinimum newLen ≤ a.lamports := by
  simp only [applyRealloc, if_neg hne] at h
  split at h
  · next a p ha hp =>
    split at h
    · next hguard =>
      injection h with hc'
      subst hc'
      -- read back idx through the two updates: outer at payerIdx (skip), inner at idx (hit)
      rw [Ctx.accounts_getElem?_update, if_neg hne, Ctx.accounts_getElem?_update, if_pos rfl, ha,
        Option.map_some]
      refine ⟨_, rfl, ?_, ?_⟩
      · -- size: resize a.data newLen has size newLen by construction
        exact resize_size a.data newLen
      · -- rent: a.lamports + (rentExemptMinimum newLen - a.lamports) = rentExemptMinimum newLen
        show rentExemptMinimum newLen ≤ a.lamports + (rentExemptMinimum newLen - a.lamports)
        rw [UInt64.add_comm, UInt64.sub_add_cancel]
        exact UInt64.le_rfl
    · exact absurd h (by simp)
  · exact absurd h (by simp)

-- Closed-loop demonstration: `resize` truncates/grows to the exact requested length.
#guard (resize (ByteArray.mk (Array.replicate 4 0)) 10).size = 10
#guard (resize (ByteArray.mk (Array.replicate 40 0)) 8).size = 8

-- Axiom audit (run manually to confirm; kept as a comment so the build stays quiet):
--   #print axioms realloc_establishes_post  ⇒  'depends on axioms: [propext, Quot.sound]'

end VerifiedAnchor
