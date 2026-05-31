import VerifiedAnchor.Codegen.Soundness
import VerifiedAnchor.Codegen.Lifecycle
import VerifiedAnchor.Decision.Check

namespace VerifiedAnchor.Codegen.Examples
open VerifiedAnchor

/-- Opaque placeholder for an `owner = EXPR` whose pubkey is unknown at macro time.
    (Unused by this example, which has no `owner` constraint; declared so emitted specs
    that DO use `owner` still elaborate.) -/
private opaque ownerPlaceholder : Pubkey

-- pre-M7a snapshot of `Transfer::lean_spec()` output (Rust, Task 3).
-- NOTE: after M7a the macro also emits typed AccountType entries (account/signer/systemAccount)
-- for Account<T>, Signer, SystemAccount, and Program wrappers.  The Transfer struct here
-- uses UncheckedAccount only, so its lean_spec() output is unchanged; the proofs below still hold.
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

/-- `transfer` is in the M4 subset (only unchecked types, only mut/signer). -/
theorem transfer_M4 : M4Subset transfer := by decide

/-- THE CLOSED LOOP: the generated validator accepting the good context PROVES the M1
    contract holds — via the generic soundness theorem. Rust struct → emitted Lean spec →
    machine-checked contract obligation. -/
theorem transfer_good_validates : validates transfer goodCtx :=
  (genValidate_sound transfer goodCtx transfer_M4).mp (by decide)

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

/-! ## seeds / PDA closed-loop (M4)

PDA derivation hashes through the opaque `sha256`, so `genSeeds` does NOT reduce under
`decide` (same wall as `discriminator`). We therefore demonstrate two honest halves:
* `resolveSeeds` is crypto-free, so the instruction-arg slice + literal resolution reduces
  concretely (the new M4 seed plumbing, computed);
* the soundness arrow is the symbolic `genValidate_sound` instantiation on a concrete
  seeds-bearing struct. The empirical PDA accept/reject lives in the Rust tests against the
  real `find_program_address`. -/
def pdaProg : Pubkey := Pubkey.ofBytes (List.replicate 32 7)
def pdaField : AccountField :=
  { name := "pda", ty := AccountType.uncheckedAccount,
    constraints := [Constraint.seeds [SeedSpec.literal "vault".toUTF8,
                                       SeedSpec.instrArg 0 4] BumpSpec.canonical] }
def withSeeds : AccountsStruct :=
  { programId := pdaProg, fields := [pdaField] }

/-- The instruction-arg seed slices the first 4 bytes of `instrData`; the literal resolves
    verbatim. (Crypto-free — this reduces.) -/
def seedCtx : Ctx :=
  { accounts := [ { key := Pubkey.zero, lamports := 0, data := ByteArray.empty,
                    owner := Pubkey.zero, rentEpoch := 0, isSigner := false,
                    isWritable := false, executable := false } ],
    instrData := (⟨#[10, 20, 30, 40, 50, 60]⟩ : ByteArray) }
#guard (resolveSeeds withSeeds seedCtx
          [SeedSpec.literal "vault".toUTF8, SeedSpec.instrArg 0 4]).length = 2
#guard (resolveSeeds withSeeds seedCtx [SeedSpec.instrArg 0 4])[0]? =
          some (⟨#[10, 20, 30, 40]⟩ : ByteArray)

/-- `withSeeds` is in the M4 subset. -/
theorem withSeeds_M4 : M4Subset withSeeds := by decide

/-- THE SEEDS CLOSED LOOP (symbolic): for any context, the generated PDA validator agrees
    with the M1 contract — the soundness theorem instantiated at the seeds-bearing struct. -/
theorem withSeeds_sound (c : Ctx) : genValidate withSeeds c = true ↔ validates withSeeds c :=
  genValidate_sound withSeeds c withSeeds_M4

/-! ## Wrapper base checks: `SystemAccount` and `Program<P>` (M4)

These mirror the macro's `wrapper_implied`: a `SystemAccount<'info>` field implies an
owner check, and a `Program<'info, P>` field implies `executable` + `key = P::ID`. The
modelled pubkeys are placeholders (`Pubkey.zero`); the runtime checks `system_program::ID`
and `P::ID`, and `genValidate_sound` is schematic over the pubkey. Crypto-free, so the
checks reduce under `decide`. -/

def sysAcctStruct : AccountsStruct :=
  { programId := Pubkey.zero
  , fields := [ { name := "sys", ty := AccountType.systemAccount, constraints := [] } ] }
def sysOwned : AccountInfo :=
  { key := Pubkey.zero, lamports := 1, data := ByteArray.empty, owner := Pubkey.zero,
    rentEpoch := 0, isSigner := false, isWritable := false, executable := false }
def sysWrongOwner : AccountInfo := { sysOwned with owner := Pubkey.ofBytes (List.replicate 32 3) }
#guard genValidate sysAcctStruct (Ctx.ofAccounts [sysOwned]) = true
#guard genValidate sysAcctStruct (Ctx.ofAccounts [sysWrongOwner]) = false
theorem sysAcct_M4 : M4Subset sysAcctStruct := by decide
/-- Closed loop: the modelled SystemAccount owner check agrees with the contract. -/
theorem sysAcct_sound (c : Ctx) : genValidate sysAcctStruct c = true ↔ validates sysAcctStruct c :=
  genValidate_sound sysAcctStruct c sysAcct_M4

def progStruct : AccountsStruct :=
  { programId := Pubkey.zero
  , fields := [ { name := "prog", ty := AccountType.program Pubkey.zero, constraints := [] } ] }
def progGood : AccountInfo :=
  { key := Pubkey.zero, lamports := 1, data := ByteArray.empty, owner := Pubkey.zero,
    rentEpoch := 0, isSigner := false, isWritable := false, executable := true }
def progNotExec : AccountInfo := { progGood with executable := false }
def progWrongKey : AccountInfo := { progGood with key := Pubkey.ofBytes (List.replicate 32 4) }
#guard genValidate progStruct (Ctx.ofAccounts [progGood]) = true
#guard genValidate progStruct (Ctx.ofAccounts [progNotExec]) = false       -- not executable
#guard genValidate progStruct (Ctx.ofAccounts [progWrongKey]) = false      -- wrong program id
theorem prog_M4 : M4Subset progStruct := by decide
/-- Closed loop: the modelled Program executable + address checks agree with the contract. -/
theorem prog_sound (c : Ctx) : genValidate progStruct c = true ↔ validates progStruct c :=
  genValidate_sound progStruct c prog_M4

end VerifiedAnchor.Codegen.Examples
