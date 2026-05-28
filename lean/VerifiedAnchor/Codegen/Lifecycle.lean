import VerifiedAnchor.Contract.Satisfies

namespace VerifiedAnchor

/-- Update the account at index `i` (no-op if out of range). `List.set`-based so
    `List.getElem?_set_self`/`_ne` give clean read-back. -/
def Ctx.update (c : Ctx) (i : Nat) (g : AccountInfo → AccountInfo) : Ctx :=
  match c[i]? with
  | some a => c.set i (g a)
  | none => c

/-- Model of Anchor `init`: system create_account funded by `payer`, then discriminator
    write. Fails (none) unless idx≠payerIdx, both in range, payer signer+writable with
    ≥rent lamports, target empty. Effect: target gets owner, data=disc++zeros to size
    (space+8), +rent lamports; payer loses rent. -/
def applyInit (idx payerIdx : Nat) (space : Nat) (owner : Pubkey) (disc : ByteArray)
    (rent : UInt64) (c : Ctx) : Option Ctx :=
  if idx = payerIdx then none else
  match c[idx]?, c[payerIdx]? with
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
  match c[idx]?, c[destIdx]? with
  | some a, some _ =>
    let c1 := c.update destIdx (fun d => { d with lamports := d.lamports + a.lamports })
    some (c1.update idx (fun a => { a with lamports := 0, data := closedAccountDiscriminator }))
  | _, _ => none

/-- Read-back lemma for `Ctx.update`: an index reads through `g` exactly when it is the
    updated index (and stays in range), otherwise it is untouched. -/
theorem Ctx.getElem?_update (c : Ctx) (i j : Nat) (g : AccountInfo → AccountInfo) :
    (c.update j g)[i]? = if i = j then (c[i]?).map g else c[i]? := by
  unfold Ctx.update
  split
  · next a hj =>
    by_cases h : i = j
    · subst h
      rw [List.getElem?_set_self (List.getElem?_eq_some_iff.mp hj).1, hj]
      simp
    · rw [List.getElem?_set_ne (fun heq => h heq.symm)]
      simp [h]
  · next hj =>
    by_cases h : i = j
    · subst h; simp [hj]
    · simp [h]

/-- `applyInit` establishes the M1 `init` post-condition for the target account:
    it exists, is owned by `owner`, and has data of size at least `space + 8`. -/
theorem init_establishes_post
    (idx payerIdx space owner disc rent c c') (hne : idx ≠ payerIdx) (hdisc : disc.size = 8)
    (h : applyInit idx payerIdx space owner disc rent c = some c') :
    ∃ a, c'[idx]? = some a ∧ a.owner = owner ∧ space + 8 ≤ a.data.size := by
  simp only [applyInit, if_neg hne] at h
  split at h
  · next a p ha hp =>
    split at h
    · next hguard =>
      -- h : some (...) = some c'
      injection h with hc'
      subst hc'
      -- read back idx through the two updates: outer at payerIdx (skip), inner at idx (hit)
      rw [Ctx.getElem?_update, if_neg hne, Ctx.getElem?_update, if_pos rfl, ha,
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
    ∃ a, c'[idx]? = some a ∧ a.lamports = 0 ∧ hasDiscriminator a closedAccountDiscriminator := by
  simp only [applyClose, if_neg hne] at h
  split at h
  · next a d ha hd =>
    injection h with hc'
    subst hc'
    -- read back idx: outer update is at idx (hit), inner at destIdx (skip)
    rw [Ctx.getElem?_update, if_pos rfl, Ctx.getElem?_update, if_neg hne, ha,
      Option.map_some]
    -- witness pinned by `rfl`; lamports is `rfl`, discriminator remains
    refine ⟨_, rfl, rfl, ?_⟩
    -- data = closedAccountDiscriminator, so prefix agrees with itself
    unfold hasDiscriminator bytesAgreePrefix
    intro i _
    rfl
  · exact absurd h (by simp)

end VerifiedAnchor
