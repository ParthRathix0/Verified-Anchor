import VerifiedAnchor.Codegen.Soundness
import VerifiedAnchor.Codegen.Lifecycle
import VerifiedAnchor.Decision.Check

namespace VerifiedAnchor.Codegen.Examples
open VerifiedAnchor

/-- Opaque placeholder for an `owner = EXPR` whose pubkey is unknown at macro time.
    (Unused by this example, which has no `owner` constraint; declared so emitted specs
    that DO use `owner` still elaborate.) -/
private opaque ownerPlaceholder : Pubkey

-- verbatim output of `Transfer::lean_spec()` (Rust, Task 3)
def transfer : AccountsStruct :=
{ programId := Pubkey.zero
, fields :=
  [ { name := "vault", ty := AccountType.uncheckedAccount, constraints := [Constraint.mut] }
  , { name := "authority", ty := AccountType.uncheckedAccount, constraints := [Constraint.signer] } ] }
-- end generated block

/-- A writable vault account. -/
def vaultAcct : AccountInfo :=
  { key := Pubkey.zero, lamports := 0, data := ByteArray.empty, owner := Pubkey.zero,
    rentEpoch := 0, isSigner := false, isWritable := true, executable := false }

/-- A signing authority account. -/
def authAcct : AccountInfo :=
  { key := Pubkey.zero, lamports := 0, data := ByteArray.empty, owner := Pubkey.zero,
    rentEpoch := 0, isSigner := true, isWritable := false, executable := false }

/-- Good: vault writable, authority signs. -/
def goodCtx : Ctx := Ctx.ofAccounts [vaultAcct, authAcct]

/-- Tampered: the authority is not a signer. -/
def tamperedCtx : Ctx := Ctx.ofAccounts [vaultAcct, { authAcct with isSigner := false }]

#guard genValidate transfer goodCtx = true
#guard genValidate transfer tamperedCtx = false

/-- `transfer` is in the M3 subset (only unchecked types, only mut/signer). -/
theorem transfer_M3 : M3Subset transfer := by decide

/-- THE CLOSED LOOP: the generated validator accepting the good context PROVES the M1
    contract holds — via the generic soundness theorem. Rust struct → emitted Lean spec →
    machine-checked contract obligation. -/
theorem transfer_good_validates : validates transfer goodCtx :=
  (genValidate_sound transfer goodCtx transfer_M3).mp (by decide)

/-! ## has_one closed-loop (relational, M3)

A typed `Account<Vault>` stores `authority : Pubkey` at offset 8. `has_one` is crypto-free,
so the per-constraint `checkConstraint` reduces on concrete data — demonstrating the
relational check biting on a matching vs forged authority. (The full `genValidate` for a
typed account would also evaluate the implied `discriminator`, which is opaque under
`sha256`; the soundness proof covers it symbolically, à la M1's Withdraw.) -/
def vaultLayoutE : FieldLayout := [("authority", 8)]
def authKeyE : Pubkey := Pubkey.ofBytes (List.replicate 32 5)
def vaultFieldE : AccountField :=
  { name := "vault", ty := AccountType.account "Vault" vaultLayoutE Pubkey.zero,
    constraints := [Constraint.hasOne "authority"] }
def withHasOne : AccountsStruct :=
  { programId := Pubkey.zero
  , fields := [ vaultFieldE
              , { name := "authority", ty := AccountType.uncheckedAccount, constraints := [] } ] }
/-- vault data = 8-byte discriminator ++ stored authority key. -/
def vaultDataE (stored : Pubkey) : ByteArray :=
  ByteArray.mk (Array.replicate 8 0) ++ ByteArray.mk stored.toArray
def hoVault (stored : Pubkey) : AccountInfo :=
  { key := Pubkey.zero, lamports := 0, data := vaultDataE stored, owner := Pubkey.zero,
    rentEpoch := 0, isSigner := false, isWritable := false, executable := false }
def hoAuthority : AccountInfo :=
  { key := authKeyE, lamports := 0, data := ByteArray.empty, owner := Pubkey.zero,
    rentEpoch := 0, isSigner := false, isWritable := false, executable := false }
def hoGood : Ctx := Ctx.ofAccounts [hoVault authKeyE, hoAuthority]
def hoBad : Ctx := Ctx.ofAccounts [hoVault (Pubkey.ofBytes (List.replicate 32 6)), hoAuthority]

#guard checkConstraint withHasOne hoGood 0 vaultFieldE (Constraint.hasOne "authority") = true
#guard checkConstraint withHasOne hoBad 0 vaultFieldE (Constraint.hasOne "authority") = false

/-! ## Lifecycle (Hoare framework, M3)

`applyInit` on a funded-signer-payer + empty-target context succeeds, and the M1 `init`
post-condition (owner set, ≥ `space+8` bytes) follows from `init_establishes_post`. -/
def lcDisc : ByteArray := ByteArray.mk (Array.replicate 8 0)
def lcPre : Ctx := Ctx.ofAccounts
  [ { key := Pubkey.zero, lamports := 0, data := ByteArray.empty, owner := Pubkey.zero,
      rentEpoch := 0, isSigner := false, isWritable := false, executable := false }
  , { key := Pubkey.zero, lamports := 1000, data := ByteArray.empty, owner := Pubkey.zero,
      rentEpoch := 0, isSigner := true, isWritable := true, executable := false } ]

#guard (applyInit 0 1 0 Pubkey.zero lcDisc 500 lcPre).isSome

/-- Concrete instantiation of the Hoare theorem: whatever `applyInit` produces here, the
    target ends up program-owned with at least `space+8` bytes. -/
theorem lc_init_establishes :
    ∀ c', applyInit 0 1 0 Pubkey.zero lcDisc 500 lcPre = some c' →
      ∃ a, c'.accounts[0]? = some a ∧ a.owner = Pubkey.zero ∧ 0 + 8 ≤ a.data.size :=
  fun c' h => init_establishes_post 0 1 0 Pubkey.zero lcDisc 500 lcPre c' (by decide) (by decide) h

end VerifiedAnchor.Codegen.Examples
