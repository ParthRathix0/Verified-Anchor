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

/-! ## M10: the `constraint = <expr>` sublanguage. -/

private def exprVaultTy : Ty := .struct [("bump", .u8), ("amount", .u64)]

/-- 8 disc + bump 3 + amount 1000 (u64 LE). -/
private def exprVaultData : ByteArray :=
  ⟨(Array.replicate 8 (0 : UInt8)) ++ #[(3 : UInt8)] ++ #[232, 3, 0, 0, 0, 0, 0, 0]⟩

private def exprStruct : AccountsStruct :=
  { programId := Pubkey.zero
  , fields :=
    [ { name := "vault"
      , ty := AccountType.account "Vault" exprVaultTy Pubkey.zero
      , constraints := [] }
    , { name := "user", ty := AccountType.uncheckedAccount, constraints := [] } ] }

private def exprVaultAcct : AccountInfo :=
  { key := Pubkey.zero, lamports := 500, data := exprVaultData, owner := Pubkey.zero
  , rentEpoch := 0, isSigner := false, isWritable := false, executable := false }

private def exprUserAcct : AccountInfo :=
  { key := Pubkey.zero, lamports := 10, data := ByteArray.empty, owner := Pubkey.zero
  , rentEpoch := 0, isSigner := true, isWritable := false, executable := false }

private def exprCtx : Ctx := Ctx.ofAccounts [exprVaultAcct, exprUserAcct]

-- amount (1000) >= 1000
#guard evalExpr exprStruct exprCtx
        (.cmp .ge (.field 0 ["amount"]) (.lit (.nat 1000))) == some true
-- amount (1000) > 1000 is false
#guard evalExpr exprStruct exprCtx
        (.cmp .gt (.field 0 ["amount"]) (.lit (.nat 1000))) == some false
-- a missing field fails CLOSED (none, not false)
#guard evalExpr exprStruct exprCtx
        (.cmp .ge (.field 0 ["nope"]) (.lit (.nat 0))) == none
-- comparing a key against a number fails closed
#guard evalExpr exprStruct exprCtx
        (.cmp .lt (.key 0) (.lit (.nat 1))) == none
-- account metadata operands
#guard evalExpr exprStruct exprCtx (.truthy (.isSigner 1)) == some true
#guard evalExpr exprStruct exprCtx (.truthy (.isSigner 0)) == some false
#guard evalExpr exprStruct exprCtx
        (.cmp .eq (.key 0) (.key 1)) == some true
-- boolean structure
#guard evalExpr exprStruct exprCtx
        (.and (.truthy (.isSigner 1))
              (.cmp .ge (.field 0 ["amount"]) (.lit (.nat 1)))) == some true
#guard evalExpr exprStruct exprCtx
        (.not (.truthy (.isSigner 1))) == some false

-- and the constraint arm agrees with the contract
#guard genConstraint exprStruct exprCtx 0 exprStruct.fields[0]!
        (Constraint.expr (.cmp .ge (.field 0 ["amount"]) (.lit (.nat 1000)))) == true
#guard genConstraint exprStruct exprCtx 0 exprStruct.fields[0]!
        (Constraint.expr (.cmp .gt (.field 0 ["amount"]) (.lit (.nat 1000)))) == false
#guard genConstraint exprStruct exprCtx 0 exprStruct.fields[0]!
        (Constraint.expr (.cmp .ge (.field 0 ["nope"]) (.lit (.nat 0)))) == false

/- The `#guard`s above are elaborator-level only. `evalExpr` is what Task 11's soundness lemma
   and Task 12's emitted specs both reduce through, so it gets the same KERNEL-level regression
   treatment as `locate`/`readVal`/`argBytes`: every example below is `by decide`, which fails
   the build if anyone reintroduces well-founded or `partial` recursion anywhere on the
   `evalOperand` → `locateField'` → `locate` → `encodedWidth` → `readVal` path. -/

/-- TRUE case: the located `u64` at offset 9 decodes to 1000 and clears the bound. -/
example : evalExpr exprStruct exprCtx
    (.cmp .ge (.field 0 ["amount"]) (.lit (.nat 1000))) = some true := by decide

/-- FALSE case: evaluable, and the answer is genuinely `some false` — distinct from `none`. -/
example : evalExpr exprStruct exprCtx
    (.cmp .gt (.field 0 ["amount"]) (.lit (.nat 1000))) = some false := by decide

/-- NONE case: an unknown field name is unevaluable, so the whole expression is `none`. -/
example : evalExpr exprStruct exprCtx
    (.cmp .ge (.field 0 ["nope"]) (.lit (.nat 0))) = none := by decide

/-- THE TYPE-CONFUSION GUARANTEE: ordering a `key` against a `nat` is `none`, NOT `some false`.
    Both reject, but only `none` says "this comparison is meaningless" — the distinction Task 12
    relies on when it decides whether a Rust expression is compilable at all. -/
example : evalExpr exprStruct exprCtx
    (.cmp .lt (.key 0) (.lit (.nat 1))) = none := by decide

/-- `eq`/`ne`, by contrast, are TOTAL over `Value`: mismatched constructors compare `false`
    rather than failing, because "is this the same value" is always a meaningful question. -/
example : evalExpr exprStruct exprCtx
    (.cmp .eq (.key 0) (.lit (.nat 1))) = some false := by decide

/-! ### Signed ordering, and the `nat`/`int` refusal.

    `evalCmp` has TWO ordering families — `nat`/`nat` and `int`/`int` — and the signed one needs
    its own coverage: a `.int` comparison routes through different arms and through `readVal`'s
    sign-reconstruction (`n - 2^64`), so the unsigned examples above say nothing about it. -/

private def signedTy : Ty := .struct [("delta", .i64)]

/-- 8 disc + `delta = -1` (i64 LE: eight `0xff` bytes). Chosen negative on purpose — a value
    whose raw bytes read as a huge `nat` if the sign is ever dropped. -/
private def signedData : ByteArray :=
  ⟨(Array.replicate 8 (0 : UInt8)) ++ (Array.replicate 8 (0xff : UInt8))⟩

private def signedStruct : AccountsStruct :=
  { programId := Pubkey.zero
  , fields :=
    [ { name := "vault"
      , ty := AccountType.account "Signed" signedTy Pubkey.zero
      , constraints := [] } ] }

private def signedCtx : Ctx := Ctx.ofAccounts [{ exprVaultAcct with data := signedData }]

/-- The decode really is signed: `-1`, not `2^64 - 1`. If `readVal`'s `.i64` arm ever lost its
    sign reconstruction, this is the example that catches it. -/
example : evalOperand signedStruct signedCtx (.field 0 ["delta"]) = some (.int (-1)) := by decide

/-- `.int`/`.int` ordering, TRUE and FALSE. Both directions of each operator are exercised so a
    transposed arm in `evalCmp` cannot hide. -/
example : evalExpr signedStruct signedCtx
    (.cmp .lt (.field 0 ["delta"]) (.lit (.int 0))) = some true := by decide
example : evalExpr signedStruct signedCtx
    (.cmp .ge (.field 0 ["delta"]) (.lit (.int 0))) = some false := by decide
example : evalExpr signedStruct signedCtx
    (.cmp .le (.field 0 ["delta"]) (.lit (.int (-1)))) = some true := by decide
example : evalExpr signedStruct signedCtx
    (.cmp .gt (.field 0 ["delta"]) (.lit (.int (-1)))) = some false := by decide
example : evalExpr signedStruct signedCtx
    (.cmp .gt (.lit (.int 1)) (.lit (.int (-1)))) = some true := by decide

/-- THE REFUSAL, pinned as a regression test rather than left to inspection: ordering a `nat`
    against an `int` is `none` in BOTH argument orders. This is the pairing a future maintainer
    is most likely to "fix" by inserting a coercion — don't. `-1 : i64` and `18446744073709551615
    : u64` have identical bytes, so any coercion silently picks a sign convention on the
    developer's behalf, and picking wrong turns a rejecting constraint into an accepting one.
    Refusing to compare is the only answer that cannot be wrong. -/
example : evalExpr signedStruct signedCtx
    (.cmp .lt (.field 0 ["delta"]) (.lit (.nat 0))) = none := by decide
example : evalExpr signedStruct signedCtx
    (.cmp .gt (.lit (.nat 1)) (.field 0 ["delta"])) = none := by decide

/-- `eq`/`ne` stay TOTAL across the same cross-pair: `nat 1` and `int 1` are different `Value`s,
    so they compare unequal rather than failing. Contrast with the orderings directly above —
    this is the deliberate seam between "meaningless to order" and "answerable to compare". -/
example : evalExpr exprStruct exprCtx
    (.cmp .eq (.lit (.nat 1)) (.lit (.int 1))) = some false := by decide
example : evalExpr exprStruct exprCtx
    (.cmp .ne (.lit (.nat 1)) (.lit (.int 1))) = some true := by decide

/-- Metadata operands: `lamports` crosses `UInt64.toNat`, `dataLen` reads `ByteArray.size`. -/
example : evalExpr exprStruct exprCtx
    (.cmp .ge (.lamports 0) (.lit (.nat 500))) = some true := by decide
example : evalExpr exprStruct exprCtx
    (.cmp .eq (.dataLen 0) (.lit (.nat 17))) = some true := by decide
example : evalExpr exprStruct exprCtx (.truthy (.isSigner 1)) = some true := by decide
example : evalExpr exprStruct exprCtx (.truthy (.isSigner 0)) = some false := by decide

/-- `truthy` of a non-`bool` operand is unevaluable, not false. -/
example : evalExpr exprStruct exprCtx (.truthy (.lamports 0)) = none := by decide

/-- An out-of-range account index fails closed at `Ctx.atField`. -/
example : evalExpr exprStruct exprCtx (.truthy (.isSigner 7)) = none := by decide

/-- Boolean structure reduces, including `or`/`not`. -/
example : evalExpr exprStruct exprCtx
    (.and (.truthy (.isSigner 1))
          (.cmp .ge (.field 0 ["amount"]) (.lit (.nat 1)))) = some true := by decide
example : evalExpr exprStruct exprCtx
    (.or (.truthy (.isSigner 0)) (.truthy (.isSigner 1))) = some true := by decide
example : evalExpr exprStruct exprCtx (.not (.truthy (.isSigner 1))) = some false := by decide

/-- STRICTNESS, pinned deliberately: neither `and` nor `or` short-circuits. Both operands are
    evaluated, so an unevaluable side makes the whole expression `none` no matter what the other
    side says. The two connectives are NOT symmetric in what that buys, and the asymmetry is the
    point:

    * `and` — `false && <unevaluable>` is `none` here and would be `some false` under
      short-circuit evaluation. Both REJECT, so for `and` this is genuinely not a safety
      difference; strictness only buys the proof convenience below.

    * `or` — `true || <unevaluable>` is `none` here (REJECT) but would be `some true` under
      short-circuit evaluation (ACCEPT). That is the difference between rejecting and accepting
      an account set, i.e. exactly the milestone's headline guarantee. **The strictness of `or`
      is load-bearing safety, not a stylistic choice, and must not be "optimized" into a
      short-circuit.** An unevaluable operand means the expression's meaning is unknown; an
      unknown must never be resolved in the caller's favour just because a sibling happened to
      be true.

    Strictness also keeps Task 11's proof free of any reasoning about evaluation order, and
    surfaces a bug on the right-hand side instead of masking it. Task 12's Rust codegen is
    written against these semantics: it must not lower `constraint = a || b` to Rust's
    short-circuiting `||` when `b` can fail to evaluate. -/
example : evalExpr exprStruct exprCtx
    (.and (.truthy (.isSigner 0)) (.cmp .ge (.field 0 ["nope"]) (.lit (.nat 0)))) = none := by
  decide
/-- THE SAFETY-CRITICAL ONE: a `true` left operand does NOT rescue an unevaluable right one.
    `none` (reject), never `some true` (accept). -/
example : evalExpr exprStruct exprCtx
    (.or (.truthy (.isSigner 1)) (.cmp .ge (.field 0 ["nope"]) (.lit (.nat 0)))) = none := by
  decide
/-- And the corresponding `satisfies`/`genConstraint` consequence, stated where it bites: an
    `or` whose right side is unevaluable is UNSATISFIED even though its left side is true. -/
example : genConstraint exprStruct exprCtx 0 exprStruct.fields[0]!
    (Constraint.expr (.or (.truthy (.isSigner 1))
                          (.cmp .ge (.field 0 ["nope"]) (.lit (.nat 0))))) = false := by decide
example : ¬ satisfies exprStruct exprCtx 0 exprStruct.fields[0]!
    (Constraint.expr (.or (.truthy (.isSigner 1))
                          (.cmp .ge (.field 0 ["nope"]) (.lit (.nat 0))))) := by decide

/-- The `.pubkey` decode — the arm that once jammed the kernel via `ByteArray.toList` — reduces
    through `evalOperand` too: the `authority` field at offset 9 equals account 1's key. -/
example : evalExpr hasOneStruct hasOneCtx
    (.cmp .eq (.field 0 ["authority"]) (.key 1)) = some true := by decide
example : evalExpr hasOneStruct hasOneWrongCtx
    (.cmp .eq (.field 0 ["authority"]) (.key 1)) = some false := by decide

/-! ### `locateField'` really walks a PATH, not just a name. -/

private def nestedTy : Ty :=
  .struct [("bump", .u8), ("inner", .struct [("a", .u32), ("b", .u64)])]

/-- 8 disc + bump 3 + inner.a = 7 (u32 LE) + inner.b = 42 (u64 LE). `inner.b` sits at 13. -/
private def nestedData : ByteArray :=
  ⟨(Array.replicate 8 (0 : UInt8)) ++ #[(3 : UInt8)]
    ++ #[(7 : UInt8), 0, 0, 0] ++ #[(42 : UInt8), 0, 0, 0, 0, 0, 0, 0]⟩

private def nestedStruct : AccountsStruct :=
  { programId := Pubkey.zero
  , fields :=
    [ { name := "vault"
      , ty := AccountType.account "Nested" nestedTy Pubkey.zero
      , constraints := [] } ] }

private def nestedCtx : Ctx :=
  Ctx.ofAccounts [{ exprVaultAcct with data := nestedData }]

example :
    (nestedStruct.fields[0]!.ty.locateField' ["inner", "b"] nestedData) = some (13, Ty.u64) := by
  decide

example : evalExpr nestedStruct nestedCtx
    (.cmp .eq (.field 0 ["inner", "b"]) (.lit (.nat 42))) = some true := by decide

/-- A path through a scalar is not a path: `bump` has no `.x`, so it fails closed. -/
example : evalExpr nestedStruct nestedCtx
    (.cmp .eq (.field 0 ["bump", "x"]) (.lit (.nat 3))) = none := by decide

/-- The single-name `locateField` is definitionally the one-element path. -/
example : nestedStruct.fields[0]!.ty.locateField "bump" nestedData
    = nestedStruct.fields[0]!.ty.locateField' ["bump"] nestedData := rfl

/-! ### `instrArg` operands resolve through the same Borsh walk as seeds. -/

example : evalExpr argStruct argCtx
    (.cmp .eq (.instrArg "amount") (.lit (.nat 1))) = some true := by decide

/-- An aggregate-typed argument is not a scalar `Value`: `readVal .string` is `none`, so a
    `constraint = label == ...` fails closed rather than comparing framing bytes. -/
example : evalExpr argStruct argCtx
    (.cmp .eq (.instrArg "label") (.lit (.nat 2))) = none := by decide

example : evalExpr argStruct argCtx
    (.cmp .eq (.instrArg "missing") (.lit (.nat 0))) = none := by decide

/-! ### The constraint arm: `genConstraint` and `satisfies` agree, and both fail closed. -/

example : genConstraint exprStruct exprCtx 0 exprStruct.fields[0]!
    (Constraint.expr (.cmp .ge (.field 0 ["amount"]) (.lit (.nat 1000)))) = true := by decide
example : genConstraint exprStruct exprCtx 0 exprStruct.fields[0]!
    (Constraint.expr (.cmp .gt (.field 0 ["amount"]) (.lit (.nat 1000)))) = false := by decide

/-- BOTH kinds of rejection collapse to `false` operationally: `some false` and `none` alike. -/
example : genConstraint exprStruct exprCtx 0 exprStruct.fields[0]!
    (Constraint.expr (.cmp .ge (.field 0 ["nope"]) (.lit (.nat 0)))) = false := by decide
example : genConstraint exprStruct exprCtx 0 exprStruct.fields[0]!
    (Constraint.expr (.cmp .lt (.key 0) (.lit (.nat 1)))) = false := by decide

/-- The MODEL side reduces too, so Task 11 will not be bridging a stuck term to a stuck term. -/
example : satisfies exprStruct exprCtx 0 exprStruct.fields[0]!
    (Constraint.expr (.cmp .ge (.field 0 ["amount"]) (.lit (.nat 1000)))) := by decide
example : ¬ satisfies exprStruct exprCtx 0 exprStruct.fields[0]!
    (Constraint.expr (.cmp .gt (.field 0 ["amount"]) (.lit (.nat 1000)))) := by decide
/-- Fail-closed on the model side: an unevaluable expression is UNSATISFIED, not satisfied. -/
example : ¬ satisfies exprStruct exprCtx 0 exprStruct.fields[0]!
    (Constraint.expr (.cmp .ge (.field 0 ["nope"]) (.lit (.nat 0)))) := by decide

/-- End to end: an `expr` constraint attached to a field flows through `genValidate`. -/
private def exprStructWithConstraint : AccountsStruct :=
  { exprStruct with
    fields :=
      [ { name := "vault"
        , ty := AccountType.uncheckedAccount
        , constraints := [Constraint.expr (.cmp .ge (.lamports 0) (.lit (.nat 500)))] }
      , { name := "user", ty := AccountType.uncheckedAccount, constraints := [] } ] }

example : genValidate exprStructWithConstraint exprCtx = true := by decide
example : genValidate exprStructWithConstraint
    (Ctx.ofAccounts [{ exprVaultAcct with lamports := 499 }, exprUserAcct]) = false := by decide

end VerifiedAnchor.Codegen.Examples
