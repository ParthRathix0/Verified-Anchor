import VerifiedAnchor.Codegen.Soundness

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
def goodCtx : Ctx := [vaultAcct, authAcct]

/-- Tampered: the authority is not a signer. -/
def tamperedCtx : Ctx := [vaultAcct, { authAcct with isSigner := false }]

#guard genValidate transfer goodCtx = true
#guard genValidate transfer tamperedCtx = false

/-- `transfer` is in the M3 subset (only unchecked types, only mut/signer). -/
theorem transfer_M3 : M3Subset transfer := by decide

/-- THE CLOSED LOOP: the generated validator accepting the good context PROVES the M1
    contract holds — via the generic soundness theorem. Rust struct → emitted Lean spec →
    machine-checked contract obligation. -/
theorem transfer_good_validates : validates transfer goodCtx :=
  (genValidate_sound transfer goodCtx transfer_M3).mp (by decide)

end VerifiedAnchor.Codegen.Examples
