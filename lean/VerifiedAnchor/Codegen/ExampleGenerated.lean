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
def vaultLayoutE : Ty := .struct [("authority", .pubkey)]
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
                                       SeedSpec.instrArg 0 4] BumpSpec.canonical none] }
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

/-! ## stored (non-canonical) bump closed-loop (M4)

The opt-in `bump = arg(off)` reads the bump byte from `instrData` at `off` and derives the
PDA with THAT specific bump via `createProgramAddress` — NO canonical `findProgramAddress`
requirement (the deliberate, less-safe opt-in). Like canonical seeds the derivation hashes
through the opaque `sha256`, so `genSeeds` does not reduce under `decide`; we demonstrate the
same two honest halves (crypto-free seed resolution + the symbolic soundness arrow) plus the
M4 membership of the new `BumpSpec.stored` constructor. The empirical accept/reject against
the real `create_program_address` lives in the Rust tests. -/
def storedField : AccountField :=
  { name := "pda", ty := AccountType.uncheckedAccount,
    constraints := [Constraint.seeds [SeedSpec.literal "vault".toUTF8]
                                     (BumpSpec.stored 0) none] }
def withStoredBump : AccountsStruct :=
  { programId := pdaProg, fields := [storedField] }

/-- A context whose instruction data carries the stored bump byte at offset 0. -/
def storedCtx : Ctx :=
  { accounts := [ { key := Pubkey.zero, lamports := 0, data := ByteArray.empty,
                    owner := Pubkey.zero, rentEpoch := 0, isSigner := false,
                    isWritable := false, executable := false } ],
    instrData := (⟨#[255]⟩ : ByteArray) }
#guard (resolveSeeds withStoredBump storedCtx [SeedSpec.literal "vault".toUTF8]).length = 1

/-- `withStoredBump` is in the M4 subset (`.seeds _ _ _` qualifies regardless of bump). -/
theorem withStoredBump_M4 : M4Subset withStoredBump := by decide

/-- THE STORED-BUMP CLOSED LOOP (symbolic): for any context, the generated stored-bump PDA
    validator agrees with the M1 contract. -/
theorem withStoredBump_sound (c : Ctx) :
    genValidate withStoredBump c = true ↔ validates withStoredBump c :=
  genValidate_sound withStoredBump c withStoredBump_M4

/-! ## seeds::program — foreign program-id PDA closed-loop (M4)

The `seeds::program = <expr>` override derives the PDA against a program id OTHER than the
struct's own `s.programId`. Modelled as the third `Constraint.seeds` field: `some someProgId`
(here a distinct placeholder) ⇒ derive against THAT id. Like every PDA case the derivation
hashes through the opaque `sha256`, so `genSeeds` does not reduce under `decide`; we show the
crypto-free seed resolution half plus the symbolic soundness arrow, and the M4 membership of
the program-override `.seeds`. The empirical accept/reject against the foreign program id lives
in the Rust tests. -/
def someProgId : Pubkey := Pubkey.ofBytes (List.replicate 32 9)
def seedsProgField : AccountField :=
  { name := "pda", ty := AccountType.uncheckedAccount,
    constraints := [Constraint.seeds [SeedSpec.literal "vault".toUTF8]
                                     BumpSpec.canonical (some someProgId)] }
def withSeedsProgram : AccountsStruct :=
  { programId := pdaProg, fields := [seedsProgField] }

#guard (resolveSeeds withSeedsProgram seedCtx [SeedSpec.literal "vault".toUTF8]).length = 1

/-- `withSeedsProgram` is in the M4 subset (`.seeds _ _ _` qualifies regardless of program). -/
theorem withSeedsProgram_M4 : M4Subset withSeedsProgram := by decide

/-- THE seeds::program CLOSED LOOP (symbolic): for any context, the generated foreign-program
    PDA validator agrees with the M1 contract. -/
theorem withSeedsProgram_sound (c : Ctx) :
    genValidate withSeedsProgram c = true ↔ validates withSeedsProgram c :=
  genValidate_sound withSeedsProgram c withSeedsProgram_M4

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

/-! ## Distinct mutable keys (M8.4)

The SAFE-BY-DEFAULT struct-level check: two `mut` accounts may not be the same account
(the "duplicate mutable accounts" vuln class). `dupStruct` has two `mut` fields; the same
ctx is accepted when their keys differ (`ctxDistinct`) and rejected when they collide
(`ctxSameKey`). `dupOk` opts the pair out via `allowDuplicate`, so the collision is allowed. -/

/-- Two writable accounts (no per-field constraint forces them apart). -/
def dupStruct : AccountsStruct :=
  { programId := Pubkey.zero
  , fields :=
    [ { name := "a", ty := AccountType.uncheckedAccount, constraints := [Constraint.mut] }
    , { name := "b", ty := AccountType.uncheckedAccount, constraints := [Constraint.mut] } ] }

/-- Opt-out twin: field `a` explicitly permits aliasing `b`. -/
def dupOk : AccountsStruct :=
  { programId := Pubkey.zero
  , fields :=
    [ { name := "a", ty := AccountType.uncheckedAccount, constraints := [Constraint.mut],
        allowDuplicate := ["b"] }
    , { name := "b", ty := AccountType.uncheckedAccount, constraints := [Constraint.mut] } ] }

def mutAcct (k : Pubkey) : AccountInfo :=
  { key := k, lamports := 0, data := ByteArray.empty, owner := Pubkey.zero,
    rentEpoch := 0, isSigner := false, isWritable := true, executable := false }

def keyA : Pubkey := Pubkey.ofBytes (List.replicate 32 1)
def keyB : Pubkey := Pubkey.ofBytes (List.replicate 32 2)

/-- Both writable, DISTINCT keys ⇒ accepted. -/
def ctxDistinct : Ctx := Ctx.ofAccounts [mutAcct keyA, mutAcct keyB]
/-- Both writable, SAME key (the duplicate-mutable attack) ⇒ rejected. -/
def ctxSameKey : Ctx := Ctx.ofAccounts [mutAcct keyA, mutAcct keyA]

#guard genValidate dupStruct ctxDistinct = true
#guard genValidate dupStruct ctxSameKey = false
-- opt-out: the SAME-key ctx is allowed because `a` permits aliasing `b`.
#guard genValidate dupOk ctxSameKey = true

theorem dupStruct_M4 : M4Subset dupStruct := by decide
/-- Closed loop: the distinct-mut-key check agrees with the contract for any ctx. -/
theorem dupStruct_sound (c : Ctx) : genValidate dupStruct c = true ↔ validates dupStruct c :=
  genValidate_sound dupStruct c dupStruct_M4

/-! ## rent_exempt closed-loop (M8.5)

`rent_exempt = enforce` is modelled as `Constraint.rentExempt` in the Lean AST. The runtime
check compares `accounts[i].lamports` against the opaque `rentExemptMinimum accounts[i].data.size`
— an uninterpreted wall, exactly like `sha256`. We therefore demonstrate the two honest halves:

* `M4Subset rentExemptStruct` reduces under `decide` (it only inspects `isM4Constraint`, which
  is a concrete Bool match on the constructor — fully decidable, no opaque call).
* The symbolic soundness arrow `genValidate_sound` instantiated at `rentExemptStruct` — valid
  for ALL contexts, schematic over `rentExemptMinimum`.

We intentionally DO NOT write `#guard genValidate rentExemptStruct ctx = true/false` over any
concrete lamport value because `rentExemptMinimum` is OPAQUE and will not reduce under `decide`.
The empirical accept/reject lives in the Rust litesvm tests (an under-funded account is rejected
on-chain; a properly-funded account is accepted). -/

/-- A single account with `rent_exempt = enforce`. The macro emits `Constraint.rentExempt`. -/
def rentExemptStruct : AccountsStruct :=
  { programId := Pubkey.zero
  , fields := [ { name := "vault", ty := AccountType.uncheckedAccount,
                  constraints := [Constraint.rentExempt] } ] }

/-- `rentExemptStruct` is in the M4 subset (`isM4Constraint .rentExempt = true` is decidable). -/
theorem rentExemptStruct_M4 : M4Subset rentExemptStruct := by decide

/-- THE rent_exempt CLOSED LOOP (symbolic): for any context, the generated rent-exemption
    validator agrees with the M1 contract — the soundness theorem instantiated at the
    rent-exempt struct. Schematic over the opaque `rentExemptMinimum`. -/
theorem rentExemptStruct_sound (c : Ctx) :
    genValidate rentExemptStruct c = true ↔ validates rentExemptStruct c :=
  genValidate_sound rentExemptStruct c rentExemptStruct_M4

/-! ## zero closed-loop (M4, crypto-free)

`Constraint.zero` checks that the discriminator slot is all-zero (uninitialized account).
Unlike `discriminator`, `isZeroDisc` compares against literal zeros — no `sha256` wall — so
`genConstraint` DOES reduce under `decide`.

`zeroStruct` has one field with an explicit `.zero` constraint.
`zeroAcct` has 8 zero bytes (the discriminator slot); `nonZeroAcct` has a non-zero first byte. -/

def zeroStruct : AccountsStruct :=
  { programId := Pubkey.zero
  , fields := [ { name := "acct", ty := AccountType.uncheckedAccount,
                  constraints := [Constraint.zero] } ] }

/-- Eight zero bytes in the discriminator slot — accepted by `zero`. -/
def zeroAcct : AccountInfo :=
  { key := Pubkey.zero, lamports := 0, data := ByteArray.mk (Array.replicate 8 0),
    owner := Pubkey.zero, rentEpoch := 0, isSigner := false, isWritable := false,
    executable := false }

/-- Non-zero first byte — rejected by `zero`. -/
def nonZeroAcct : AccountInfo := { zeroAcct with data := ByteArray.mk (Array.replicate 8 1) }

#guard genConstraint zeroStruct (Ctx.ofAccounts [zeroAcct]) 0
         { name := "acct", ty := AccountType.uncheckedAccount, constraints := [Constraint.zero] }
         Constraint.zero = true
#guard genConstraint zeroStruct (Ctx.ofAccounts [nonZeroAcct]) 0
         { name := "acct", ty := AccountType.uncheckedAccount, constraints := [Constraint.zero] }
         Constraint.zero = false
#guard genValidate zeroStruct (Ctx.ofAccounts [zeroAcct]) = true
#guard genValidate zeroStruct (Ctx.ofAccounts [nonZeroAcct]) = false

theorem zeroStruct_M4 : M4Subset zeroStruct := by decide

/-- THE zero CLOSED LOOP: decide confirms the constraint reduces (crypto-free), and
    M4Subset + genValidate_sound close the proof obligation for the zero-data context. -/
theorem zeroStruct_sound (c : Ctx) :
    genValidate zeroStruct c = true ↔ validates zeroStruct c :=
  genValidate_sound zeroStruct c zeroStruct_M4

theorem zeroStruct_good_validates : validates zeroStruct (Ctx.ofAccounts [zeroAcct]) :=
  (genValidate_sound zeroStruct (Ctx.ofAccounts [zeroAcct]) zeroStruct_M4).mp (by decide)

-- `decide (M4Subset zeroStruct)` reduces to `true` — the subset check is crypto-free.
#guard decide (M4Subset zeroStruct) = true

/-! ## M10: `has_one` reads the named field, not a hardcoded offset.

    `hasOneVaultTy` places `authority` AFTER a `u8`, so the target sits at offset 9, not 8.
    Before M10 the model read offset 8 and this `#guard` would fail. -/

private def hasOneVaultTy : Ty := .struct [("bump", .u8), ("authority", .pubkey)]

private def authKey : Pubkey := Pubkey.ofBytes (List.replicate 32 (5 : UInt8))

/-- 8 disc bytes, then bump = 7, then 32 bytes of `authKey`. -/
private def hasOneVaultData : ByteArray :=
  ⟨(Array.replicate 8 (0 : UInt8)) ++ #[(7 : UInt8)] ++ (Array.replicate 32 (5 : UInt8))⟩

private def hasOneStruct : AccountsStruct :=
  { programId := Pubkey.zero
  , fields :=
    [ { name := "vault"
      , ty := AccountType.account "Vault" hasOneVaultTy Pubkey.zero
      , constraints := [Constraint.hasOne "authority"] }
    , { name := "authority", ty := AccountType.uncheckedAccount, constraints := [] } ] }

private def hasOneVaultAcct : AccountInfo :=
  { key := Pubkey.zero, lamports := 1, data := hasOneVaultData, owner := Pubkey.zero
  , rentEpoch := 0, isSigner := false, isWritable := false, executable := false }

private def hasOneAuthAcct : AccountInfo :=
  { key := authKey, lamports := 1, data := ByteArray.empty, owner := Pubkey.zero
  , rentEpoch := 0, isSigner := false, isWritable := false, executable := false }

private def hasOneCtx : Ctx :=
  Ctx.ofAccounts [hasOneVaultAcct, hasOneAuthAcct]

-- the offset-9 field is found and matches
#guard genConstraint hasOneStruct hasOneCtx 0 hasOneStruct.fields[0]!
        (Constraint.hasOne "authority") == true

-- a mismatched authority is rejected
private def hasOneWrongCtx : Ctx :=
  Ctx.ofAccounts [hasOneVaultAcct, { hasOneAuthAcct with key := Pubkey.zero }]

#guard genConstraint hasOneStruct hasOneWrongCtx 0 hasOneStruct.fields[0]!
        (Constraint.hasOne "authority") == false

/- The `#guard`s above run in the elaborator, which evaluates via the compiler and is happy to
   step through definitions the *kernel* refuses to unfold. The `by decide`s below are the
   stronger claim: the whole `has_one` path — `locateField`, the `encodedWidth` walk over the
   leading `u8`, and `readVal`'s `.pubkey` decode — reduces in the kernel. That is not free.
   `readVal` originally gathered its 32 bytes with `ByteArray.toList`, which core defines by
   well-founded recursion; it passed every `#guard` while being kernel-opaque, and these
   examples are what surface that class of regression. -/

/-- The located offset is 9, not 8: `bump : u8` sits in front of `authority`. This single
    number is the entire defect M10 fixes. -/
example :
    (hasOneStruct.fields[0]!.ty.locateField "authority" hasOneVaultData).map (·.1) = some 9 := by
  decide

example :
    genConstraint hasOneStruct hasOneCtx 0 hasOneStruct.fields[0]!
      (Constraint.hasOne "authority") = true := by
  decide

example :
    genConstraint hasOneStruct hasOneWrongCtx 0 hasOneStruct.fields[0]!
      (Constraint.hasOne "authority") = false := by
  decide

/-- The model side reduces too, so the `iff` below is not bridging a stuck term to a stuck term. -/
example : satisfies hasOneStruct hasOneCtx 0 hasOneStruct.fields[0]!
    (Constraint.hasOne "authority") :=
  (genConstraint_hasOne_iff hasOneStruct hasOneCtx 0 hasOneStruct.fields[0]! "authority").mp
    (by decide)

/-! ## M10: named instruction arguments resolve through the Borsh machinery. -/

private def argStruct : AccountsStruct :=
  { programId := Pubkey.zero
  , instrArgs := [("amount", Ty.u64), ("label", Ty.string)]
  , fields := [] }

/-- amount = 1 (u64 LE), then label = "hi" (u32 len + utf8). -/
private def argCtx : Ctx :=
  { accounts := []
  , instrData := ⟨#[1,0,0,0,0,0,0,0, 2,0,0,0, 104, 105]⟩ }

#guard (argStruct.argBytes argCtx "amount").map (·.toList)
         == some [1,0,0,0,0,0,0,0]
#guard (argStruct.argBytes argCtx "label").map (·.toList) == some [104, 105]
#guard (argStruct.argBytes argCtx "missing") == none

/- As with `Locate.lean`, the `#guard`s above only prove the elaborator can step through
   `argBytes`; they say nothing about the kernel. `argBytes` is Task 9's contract — the Rust
   macro must mirror it byte-for-byte, especially the length-prefix stripping on `string`/`vec`
   — so it gets the same `by decide` regression treatment as `locate`/`readVal`/`has_one`. -/

/-- `u64` is fixed-size: `argBytes` returns the whole 8-byte encoding, no framing to strip. -/
example : argStruct.argBytes argCtx "amount" = some (⟨#[1,0,0,0,0,0,0,0]⟩ : ByteArray) := by
  decide

/-- THE POINT: `string`'s 4-byte length prefix is stripped. `label`'s raw Borsh encoding is
    6 bytes (`2,0,0,0,104,105`); `argBytes` returns only the 2 payload bytes `[104,105]` —
    exactly what Anchor's `label.as_bytes()` would hand seed code. -/
example : argStruct.argBytes argCtx "label" = some (⟨#[104, 105]⟩ : ByteArray) := by
  decide

example : argStruct.argBytes argCtx "missing" = none := by decide

end VerifiedAnchor.Codegen.Examples
