# Verified Anchor — Milestone 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Lean 4 formal validation contract for Anchor account validation — a declarative `validates : AccountsStruct → Ctx → Prop`, an executable `validatesBool`, a proof they agree, and worked examples — with a green `lake build` and zero `sorry`.

**Architecture:** Layered Lean library. `Solana/` models the account world concretely (crypto primitives axiomatized); `Constraints/` is the constraint AST (the Rust↔Lean seam for later milestones); `Contract/` is the declarative spec; `Decision/` is the executable checker + agreement theorem; `Examples/` demonstrates the contract biting.

**Tech Stack:** Lean 4.30.0, Lake 5.0.0, `batteries` dependency only. No mathlib.

---

## Conventions for every task

- **Toolchain:** every `lake` command must run with elan on PATH. Prefix with:
  `export PATH="$HOME/.elan/bin:$PATH"` (or `source "$HOME/.elan/env"`).
- **Working dir:** all Lean work happens in `/home/parth/Desktop/PARTH/Verification/lean`.
- **"Test" in Lean:** we do not have pytest. A task's check is one or both of:
  - `#guard <bool expr>` lines that fail compilation if false (compile-time assertions),
  - `example : <prop> := by <proof>` / `theorem` that fail compilation if unproved.
  The build command `lake build` is the test runner. A failing test = a build error.
- **Zero `sorry`:** no task may leave a `sorry`. If a proof is not closing, that is a real
  failure to debug (use superpowers:systematic-debugging), not something to stub.
- **Commit** after each task with the message shown.

---

## File structure

| File | Responsibility |
|------|----------------|
| `lean/lakefile.toml` | Package def, `batteries` require, lib target |
| `lean/lean-toolchain` | Pin `leanprover/lean4:v4.30.0` |
| `lean/VerifiedAnchor.lean` | Root: imports every module |
| `lean/VerifiedAnchor/Solana/Pubkey.lean` | `Pubkey`, `Lamports`, constants |
| `lean/VerifiedAnchor/Solana/Account.lean` | `AccountInfo` structure |
| `lean/VerifiedAnchor/Solana/Crypto.lean` | opaque `sha256`/`isOnCurve`, `createProgramAddress`, `findProgramAddress` |
| `lean/VerifiedAnchor/Solana/Discriminator.lean` | Anchor 8-byte discriminator |
| `lean/VerifiedAnchor/Solana/Layout.lean` | `FieldLayout`, `readPubkey` |
| `lean/VerifiedAnchor/Constraints/Ast.lean` | `Constraint`, `AccountType`, `AccountField`, `AccountsStruct`, `impliedConstraints` |
| `lean/VerifiedAnchor/Constraints/Context.lean` | `Ctx`, `lookup`, `WellFormed` |
| `lean/VerifiedAnchor/Contract/Satisfies.lean` | `satisfies` per-constraint Prop semantics |
| `lean/VerifiedAnchor/Contract/Validates.lean` | `fieldValidates`, `validates` |
| `lean/VerifiedAnchor/Decision/Check.lean` | `checkConstraint`, `validatesBool` |
| `lean/VerifiedAnchor/Decision/Agreement.lean` | `validates_iff_validatesBool` |
| `lean/VerifiedAnchor/Examples/Withdraw.lean` | Worked example + `#guard` + lemmas |

---

## Task 0: Lake project scaffold

**Files:**
- Create: `lean/lakefile.toml`, `lean/lean-toolchain`, `lean/VerifiedAnchor.lean`, `lean/VerifiedAnchor/Solana/Placeholder.lean`

- [ ] **Step 1: Pin the toolchain**

Create `lean/lean-toolchain`:
```
leanprover/lean4:v4.30.0
```

- [ ] **Step 2: Write the lakefile**

Create `lean/lakefile.toml`:
```toml
name = "verified-anchor"
defaultTargets = ["VerifiedAnchor"]

[[require]]
name = "batteries"
git = "https://github.com/leanprover-community/batteries"
rev = "v4.30.0"

[[lean_lib]]
name = "VerifiedAnchor"
```

- [ ] **Step 3: Minimal root + placeholder module (so the lib has something to build)**

Create `lean/VerifiedAnchor/Solana/Placeholder.lean`:
```lean
namespace VerifiedAnchor
/-- Temporary anchor so the library target builds before real modules land. -/
def placeholder : Nat := 0
end VerifiedAnchor
```
Create `lean/VerifiedAnchor.lean`:
```lean
import VerifiedAnchor.Solana.Placeholder
```

- [ ] **Step 4: Fetch deps and build**

Run (from `lean/`):
```bash
export PATH="$HOME/.elan/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/lean && lake update && lake build
```
Expected: `lake update` resolves `batteries` (matching `v4.30.0`); `lake build` succeeds with no errors.
If `rev = "v4.30.0"` does not exist for batteries, run `lake update` after changing `rev` to `main` and pin to the resolved commit shown in `lake-manifest.json`.

- [ ] **Step 5: Commit**

```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/ && git commit -m "feat(lean): scaffold lake project with batteries dep"
```

---

## Task 1: Solana primitives — Pubkey and AccountInfo

**Files:**
- Create: `lean/VerifiedAnchor/Solana/Pubkey.lean`, `lean/VerifiedAnchor/Solana/Account.lean`

- [ ] **Step 1: Write Pubkey with a compile-time check**

Create `lean/VerifiedAnchor/Solana/Pubkey.lean`:
```lean
namespace VerifiedAnchor

/-- A Solana public key: 32 bytes. -/
abbrev Pubkey := Vector UInt8 32

/-- Account balance in lamports. -/
abbrev Lamports := UInt64

namespace Pubkey

/-- The all-zero pubkey (also the System Program id placeholder for examples). -/
def zero : Pubkey := Vector.replicate 32 0

/-- Build a pubkey from a list of bytes, padding with 0 / truncating to exactly 32.
    Total by construction — the length proof is discharged by `List.length_map`/`simp`. -/
def ofBytes (bs : List UInt8) : Pubkey :=
  ⟨((List.range 32).map (fun i => bs.getD i 0)).toArray, by simp⟩

end Pubkey

/-- DecidableEq is needed everywhere downstream. -/
example : DecidableEq Pubkey := inferInstance

#guard Pubkey.zero == Pubkey.zero
end VerifiedAnchor
```
The hard requirements: `Pubkey` has `DecidableEq` (the `example` must compile) and
`zero`/`ofBytes` exist for examples. If the `by simp` length proof does not close, try
`by simp [List.length_map, List.length_range]`.

- [ ] **Step 2: Build to verify the DecidableEq instance + guard**

Run: `cd /home/parth/Desktop/PARTH/Verification/lean && export PATH="$HOME/.elan/bin:$PATH" && lake build VerifiedAnchor.Solana.Pubkey`
Expected: success. If `Vector` is unrecognized, add `import Batteries` at the top (Lean
4.30 has `Vector` in core; the import is a fallback).

- [ ] **Step 3: Write AccountInfo**

Create `lean/VerifiedAnchor/Solana/Account.lean`:
```lean
import VerifiedAnchor.Solana.Pubkey

namespace VerifiedAnchor

/-- Faithful model of Solana's `AccountInfo` fields. -/
structure AccountInfo where
  key        : Pubkey
  lamports   : UInt64
  data       : ByteArray
  owner      : Pubkey
  rentEpoch  : UInt64
  isSigner   : Bool
  isWritable : Bool
  executable : Bool
  deriving Inhabited

/-- First `n` bytes of an account's data, or all of them if shorter. -/
def AccountInfo.dataPrefix (a : AccountInfo) (n : Nat) : ByteArray :=
  a.data.extract 0 (min n a.data.size)

end VerifiedAnchor
```
NOTE: `ByteArray` has no `DecidableEq` by default in a usable form for `deriving` on the
struct; we only derive `Inhabited`. Byte comparisons are done explicitly via
`ByteArray` element access downstream, so struct-level `DecidableEq` is not required.

- [ ] **Step 4: Build**

Run: `lake build VerifiedAnchor.Solana.Account`
Expected: success.

- [ ] **Step 5: Commit**

```bash
git add lean/VerifiedAnchor/Solana/Pubkey.lean lean/VerifiedAnchor/Solana/Account.lean
git commit -m "feat(solana): Pubkey and AccountInfo concrete models"
```

---

## Task 2: Crypto — axiomatized primitives, concrete PDA algorithm

**Files:**
- Create: `lean/VerifiedAnchor/Solana/Crypto.lean`

- [ ] **Step 1: Write the crypto module**

Create `lean/VerifiedAnchor/Solana/Crypto.lean`:
```lean
import VerifiedAnchor.Solana.Pubkey

namespace VerifiedAnchor

/-- SHA-256. Uninterpreted: we model its interface, not its bit-level behavior. -/
opaque sha256 : ByteArray → ByteArray

/-- SHA-256 always returns 32 bytes. The only crypto fact M1 relies on. -/
axiom sha256_size (b : ByteArray) : (sha256 b).size = 32

/-- Read the SHA-256 output as a `Pubkey`. Uses the total `ofBytes` constructor so no
    length proof against `sha256_size` is needed (it pads/truncates to 32). -/
def sha256Pubkey (b : ByteArray) : Pubkey :=
  Pubkey.ofBytes (sha256 b).toList

/-- ed25519 on-curve test. Uninterpreted. -/
opaque isOnCurve : Pubkey → Bool

/-- Solana's create_program_address: hash seeds ++ programId ++ marker, fail if on-curve. -/
def createProgramAddress (seeds : List ByteArray) (programId : Pubkey) : Option Pubkey :=
  let marker := "ProgramDerivedAddress".toUTF8
  let input := (seeds.foldl (· ++ ·) ByteArray.empty)
                 ++ ⟨programId.toArray⟩ ++ marker
  let candidate := sha256Pubkey input
  if isOnCurve candidate then none else some candidate

/-- Solana's find_program_address: iterate bump 255→0, first off-curve hit wins. -/
def findProgramAddress (seeds : List ByteArray) (programId : Pubkey) :
    Option (Pubkey × UInt8) :=
  let rec go (bump : Nat) : Option (Pubkey × UInt8) :=
    match bump with
    | 0 => match createProgramAddress (seeds ++ [⟨#[0]⟩]) programId with
           | some pk => some (pk, 0)
           | none => none
    | n+1 =>
      match createProgramAddress (seeds ++ [⟨#[UInt8.ofNat (n+1)]⟩]) programId with
      | some pk => some (pk, UInt8.ofNat (n+1))
      | none => go n
  go 255

end VerifiedAnchor
```

- [ ] **Step 2: Build, confirming no `sorry` remains**

Run: `lake build VerifiedAnchor.Solana.Crypto`
Expected: success. Then verify zero `sorry`:
`grep -rn "sorry" VerifiedAnchor/Solana/Crypto.lean` → no matches.

- [ ] **Step 3: Commit**

```bash
git add lean/VerifiedAnchor/Solana/Crypto.lean
git commit -m "feat(solana): axiomatized sha256/isOnCurve with concrete PDA derivation"
```

---

## Task 3: Discriminator

**Files:**
- Create: `lean/VerifiedAnchor/Solana/Discriminator.lean`

- [ ] **Step 1: Write the module**

Create `lean/VerifiedAnchor/Solana/Discriminator.lean`:
```lean
import VerifiedAnchor.Solana.Crypto
import VerifiedAnchor.Solana.Account

namespace VerifiedAnchor

/-- Anchor's 8-byte account discriminator: first 8 bytes of sha256("account:<Name>"). -/
def accountDiscriminator (name : String) : ByteArray :=
  (sha256 ("account:" ++ name).toUTF8).extract 0 8

/-- Two byte arrays agree on their first `n` bytes. -/
def bytesAgreePrefix (x y : ByteArray) (n : Nat) : Prop :=
  ∀ i, i < n → (x.get? i) = (y.get? i)

instance (x y : ByteArray) (n : Nat) : Decidable (bytesAgreePrefix x y n) := by
  unfold bytesAgreePrefix
  exact Nat.decidableBallLT n (fun i _ => x.get? i = y.get? i)

/-- An account's data begins with the given 8-byte discriminator. -/
def hasDiscriminator (a : AccountInfo) (d : ByteArray) : Prop :=
  bytesAgreePrefix a.data d 8

instance (a : AccountInfo) (d : ByteArray) : Decidable (hasDiscriminator a d) :=
  inferInstanceAs (Decidable (bytesAgreePrefix _ _ _))

end VerifiedAnchor
```
NOTE: `ByteArray.get?` exists in core. If `Nat.decidableBallLT` has a different name in
4.30, use `inferInstance` after `unfold` (the body is a bounded `∀` over `Nat`, which is
decidable) or `decidable_of_iff` against `(List.range n).all ...`.

- [ ] **Step 2: Build**

Run: `lake build VerifiedAnchor.Solana.Discriminator`
Expected: success, no `sorry`.

- [ ] **Step 3: Commit**

```bash
git add lean/VerifiedAnchor/Solana/Discriminator.lean
git commit -m "feat(solana): Anchor discriminator with decidable prefix check"
```

---

## Task 4: Layout — Borsh-lite Pubkey reader

**Files:**
- Create: `lean/VerifiedAnchor/Solana/Layout.lean`

- [ ] **Step 1: Write the module**

Create `lean/VerifiedAnchor/Solana/Layout.lean`:
```lean
import VerifiedAnchor.Solana.Pubkey

namespace VerifiedAnchor

/-- Maps a named `Pubkey` field of an account's deserialized struct to its byte offset
    (offset measured from the start of `data`, i.e. including the 8-byte discriminator). -/
abbrev FieldLayout := List (String × Nat)

def FieldLayout.offsetOf (l : FieldLayout) (name : String) : Option Nat :=
  (l.find? (·.1 == name)).map (·.2)

/-- Read a 32-byte Pubkey at `offset` in `data`, or `none` if out of bounds.
    Uses the total `ofBytes` constructor — no length proof needed. -/
def readPubkey (data : ByteArray) (offset : Nat) : Option Pubkey :=
  if offset + 32 ≤ data.size then
    some (Pubkey.ofBytes ((data.extract offset (offset + 32)).toList))
  else none

end VerifiedAnchor
```

- [ ] **Step 2: Build**

Run: `lake build VerifiedAnchor.Solana.Layout`
Expected: success, no `sorry`.

- [ ] **Step 3: Commit**

```bash
git add lean/VerifiedAnchor/Solana/Layout.lean
git commit -m "feat(solana): FieldLayout and readPubkey (Borsh-lite)"
```

---

## Task 5: Constraint AST

**Files:**
- Create: `lean/VerifiedAnchor/Constraints/Ast.lean`

- [ ] **Step 1: Write the AST**

Create `lean/VerifiedAnchor/Constraints/Ast.lean`:
```lean
import VerifiedAnchor.Solana.Pubkey
import VerifiedAnchor.Solana.Layout
import VerifiedAnchor.Solana.Discriminator

namespace VerifiedAnchor

/-- A single seed in a PDA derivation. -/
inductive SeedSpec where
  | literal (bytes : ByteArray)        -- e.g. b"vault"
  | fieldKey (field : String)          -- another account's key bytes
  deriving Inhabited

inductive BumpSpec where
  | declared (b : UInt8)
  | canonical
  deriving Inhabited, DecidableEq

/-- The Anchor constraint subset in scope for v1. -/
inductive Constraint where
  | signer
  | mut
  | owner          (expected : Pubkey)
  | hasOne         (field : String)
  | seeds          (seeds : List SeedSpec) (bump : BumpSpec)
  | init           (payer : String) (space : Nat) (owner : Pubkey)
  | close          (dest : String)
  | discriminator  (expected : ByteArray)   -- 8 bytes
  deriving Inhabited

/-- Account wrapper types; each implies certain base constraints. -/
inductive AccountType where
  | account          (typeName : String) (layout : FieldLayout) (programId : Pubkey)
  | signer
  | program          (id : Pubkey)
  | systemAccount
  | uncheckedAccount
  deriving Inhabited

/-- Base constraints implied by the wrapper type, before explicit annotations. -/
def AccountType.impliedConstraints : AccountType → List Constraint
  | .account tn _ pid => [Constraint.owner pid, Constraint.discriminator (accountDiscriminator tn)]
  | .signer           => [Constraint.signer]
  | .program _        => []      -- executable check modeled separately if needed
  | .systemAccount    => []
  | .uncheckedAccount => []

/-- Look up the layout offset of a `Pubkey` field within an account type. -/
def AccountType.layoutOffsetOf : AccountType → String → Option Nat
  | .account _ layout _, name => layout.offsetOf name
  | _, _ => none

structure AccountField where
  name        : String
  ty          : AccountType
  constraints : List Constraint
  deriving Inhabited

structure AccountsStruct where
  programId : Pubkey
  fields    : List AccountField
  deriving Inhabited

/-- Find a declared field by name. -/
def AccountsStruct.fieldNamed (s : AccountsStruct) (name : String) : Option AccountField :=
  s.fields.find? (·.name == name)

end VerifiedAnchor
```

- [ ] **Step 2: Build**

Run: `lake build VerifiedAnchor.Constraints.Ast`
Expected: success.

- [ ] **Step 3: Commit**

```bash
git add lean/VerifiedAnchor/Constraints/Ast.lean
git commit -m "feat(constraints): constraint AST (Rust<->Lean seam)"
```

---

## Task 6: Context

**Files:**
- Create: `lean/VerifiedAnchor/Constraints/Context.lean`

- [ ] **Step 1: Write the module**

Create `lean/VerifiedAnchor/Constraints/Context.lean`:
```lean
import VerifiedAnchor.Constraints.Ast
import VerifiedAnchor.Solana.Account

namespace VerifiedAnchor

/-- The runtime accounts, positionally aligned with `AccountsStruct.fields`. -/
abbrev Ctx := List AccountInfo

/-- Resolve a declared field name to its account, by matching field position. -/
def Ctx.lookup (s : AccountsStruct) (c : Ctx) (name : String) : Option AccountInfo := do
  let idx ← s.fields.findIdx? (·.name == name)
  c[idx]?

/-- Resolve the account paired with a specific field (by index in the struct). -/
def Ctx.atField (s : AccountsStruct) (c : Ctx) (idx : Nat) : Option AccountInfo :=
  c[idx]?

/-- Structural well-formedness: one account per declared field. -/
def WellFormed (s : AccountsStruct) (c : Ctx) : Prop :=
  c.length = s.fields.length

instance (s : AccountsStruct) (c : Ctx) : Decidable (WellFormed s c) :=
  inferInstanceAs (Decidable (c.length = s.fields.length))

end VerifiedAnchor
```

- [ ] **Step 2: Build**

Run: `lake build VerifiedAnchor.Constraints.Context`
Expected: success.

- [ ] **Step 3: Commit**

```bash
git add lean/VerifiedAnchor/Constraints/Context.lean
git commit -m "feat(constraints): Ctx, lookup, WellFormed"
```

---

## Task 7: Contract — per-constraint semantics (`satisfies`)

**Files:**
- Create: `lean/VerifiedAnchor/Contract/Satisfies.lean`

- [ ] **Step 1: Write `satisfies` with a Decidable instance**

Create `lean/VerifiedAnchor/Contract/Satisfies.lean`:
```lean
import VerifiedAnchor.Constraints.Context
import VerifiedAnchor.Solana.Crypto

namespace VerifiedAnchor

/-- What it means for the account at `idx` (with field `f`) to satisfy one constraint. -/
def satisfies (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) :
    Constraint → Prop
  | .signer => (Ctx.atField s c idx).any (·.isSigner)  -- see Bool/Prop note
  | .mut    => (Ctx.atField s c idx).any (·.isWritable)
  | .owner expected =>
      ∃ a, Ctx.atField s c idx = some a ∧ a.owner = expected
  | .hasOne field =>
      ∃ a target off val,
        Ctx.atField s c idx = some a ∧
        f.ty.layoutOffsetOf field = some off ∧
        readPubkey a.data off = some val ∧
        Ctx.lookup s c field = some target ∧
        val = target.key
  | .discriminator d =>
      ∃ a, Ctx.atField s c idx = some a ∧ hasDiscriminator a d
  | .seeds ss b =>
      ∃ a, Ctx.atField s c idx = some a ∧
        ∃ bump, findProgramAddress (resolveSeeds s c ss) s.programId = some (a.key, bump) ∧
          (match b with | .declared db => bump = db | .canonical => True)
  | .init payer space owner =>
      ∃ a p,
        Ctx.atField s c idx = some a ∧
        Ctx.lookup s c payer = some p ∧
        a.owner = owner ∧ p.isSigner ∧ p.isWritable ∧ a.data.size ≥ space + 8
  | .close dest =>
      ∃ a, Ctx.atField s c idx = some a ∧
        (Ctx.lookup s c dest).isSome ∧ a.lamports = 0
where
  /-- Resolve a seed list against the context (literal bytes, or another field's key). -/
  resolveSeeds (s : AccountsStruct) (c : Ctx) : List SeedSpec → List ByteArray
    | [] => []
    | .literal bytes :: rest => bytes :: resolveSeeds s c rest
    | .fieldKey name :: rest =>
        (match Ctx.lookup s c name with
         | some a => ⟨a.key.toArray⟩
         | none => ByteArray.empty) :: resolveSeeds s c rest

end VerifiedAnchor
```
NOTE on `.any`: `Option.any` returns `Bool`; in a `Prop`-valued function wrap it as
`(Ctx.atField s c idx).any (·.isSigner) = true`, OR (cleaner) write these two cases in the
same existential style as `owner`:
```lean
  | .signer => ∃ a, Ctx.atField s c idx = some a ∧ a.isSigner = true
  | .mut    => ∃ a, Ctx.atField s c idx = some a ∧ a.isWritable = true
```
Use the existential form — it is uniform and keeps the Decidable instance regular.

- [ ] **Step 2: Add a Decidable instance for `satisfies`**

Append to the same file:
```lean
instance (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) (k : Constraint) :
    Decidable (satisfies s c idx f k) := by
  cases k <;> simp only [satisfies] <;> infer_instance
```
NOTE: each case reduces to decidable equalities/conjunctions over `Option`/`Pubkey`/`Bool`
plus the `hasDiscriminator` instance from Task 3. If `infer_instance` fails on the
existential-over-`Option` cases, rewrite each `∃ a, opt = some a ∧ P a` using
`Option.rec`/pattern match into a `match` returning a `Prop` whose `Decidable` is direct,
e.g. define a helper `withAccount (o : Option AccountInfo) (P : AccountInfo → Prop) : Prop`
with `instance [∀ a, Decidable (P a)] : Decidable (withAccount o P)`. Then the case proofs
are `infer_instance`.

- [ ] **Step 3: Build**

Run: `lake build VerifiedAnchor.Contract.Satisfies`
Expected: success, no `sorry`.

- [ ] **Step 4: Commit**

```bash
git add lean/VerifiedAnchor/Contract/Satisfies.lean
git commit -m "feat(contract): per-constraint satisfies semantics + Decidable"
```

---

## Task 8: Contract — `validates`

**Files:**
- Create: `lean/VerifiedAnchor/Contract/Validates.lean`

- [ ] **Step 1: Write the module**

Create `lean/VerifiedAnchor/Contract/Validates.lean`:
```lean
import VerifiedAnchor.Contract.Satisfies

namespace VerifiedAnchor

/-- A field satisfies all its constraints (type-implied ones first, then explicit). -/
def fieldValidates (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) : Prop :=
  ∀ k ∈ (f.ty.impliedConstraints ++ f.constraints), satisfies s c idx f k

instance (s c idx f) : Decidable (fieldValidates s c idx f) :=
  inferInstanceAs (Decidable (∀ k ∈ _, satisfies s c idx f k))

/-- THE CONTRACT: a context validates a struct iff it is well-formed and every field
    satisfies all of its constraints. -/
def validates (s : AccountsStruct) (c : Ctx) : Prop :=
  WellFormed s c ∧
    ∀ p ∈ s.fields.zipIdx, fieldValidates s c p.2 p.1

instance (s c) : Decidable (validates s c) := by
  unfold validates; infer_instance
```
NOTE: `List.zipIdx` pairs each field with its index (`(field, idx)`). Confirm the field
order: `zipIdx` yields `(a, 0), (b, 1), …`; `p.1` is the field, `p.2` the index. If the
membership-quantifier Decidable instance is missing, use `List.decidableBAll`.

- [ ] **Step 2: Build**

Run: `lake build VerifiedAnchor.Contract.Validates`
Expected: success.

- [ ] **Step 3: Commit**

```bash
git add lean/VerifiedAnchor/Contract/Validates.lean
git commit -m "feat(contract): validates : AccountsStruct -> Ctx -> Prop"
```

---

## Task 9: Decision — executable checker

**Files:**
- Create: `lean/VerifiedAnchor/Decision/Check.lean`

- [ ] **Step 1: Write `validatesBool` via `decide`**

Because `validates` already has a `Decidable` instance (Task 8), the executable checker is
its decision procedure. This guarantees agreement by construction.

Create `lean/VerifiedAnchor/Decision/Check.lean`:
```lean
import VerifiedAnchor.Contract.Validates

namespace VerifiedAnchor

/-- Executable account-validation checker. Agrees with `validates` by construction
    (it is the decision procedure of the `Decidable (validates …)` instance). -/
def validatesBool (s : AccountsStruct) (c : Ctx) : Bool :=
  decide (validates s c)

/-- Per-constraint executable check, exposed for examples/diagnostics. -/
def checkConstraint (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField)
    (k : Constraint) : Bool :=
  decide (satisfies s c idx f k)

end VerifiedAnchor
```

- [ ] **Step 2: Build**

Run: `lake build VerifiedAnchor.Decision.Check`
Expected: success.

- [ ] **Step 3: Commit**

```bash
git add lean/VerifiedAnchor/Decision/Check.lean
git commit -m "feat(decision): validatesBool executable checker"
```

---

## Task 10: Decision — agreement theorem

**Files:**
- Create: `lean/VerifiedAnchor/Decision/Agreement.lean`

- [ ] **Step 1: Write and prove the agreement theorem**

Create `lean/VerifiedAnchor/Decision/Agreement.lean`:
```lean
import VerifiedAnchor.Decision.Check

namespace VerifiedAnchor

/-- The executable checker agrees with the declarative contract. -/
theorem validates_iff_validatesBool (s : AccountsStruct) (c : Ctx) :
    validates s c ↔ validatesBool s c = true := by
  unfold validatesBool
  exact (decide_eq_true_iff).symm

/-- Corollary: a true checker result is a proof of the contract. -/
theorem validatesBool_sound (s : AccountsStruct) (c : Ctx)
    (h : validatesBool s c = true) : validates s c :=
  (validates_iff_validatesBool s c).mpr h

/-- Corollary: the checker never rejects a validating context. -/
theorem validatesBool_complete (s : AccountsStruct) (c : Ctx)
    (h : validates s c) : validatesBool s c = true :=
  (validates_iff_validatesBool s c).mp h

end VerifiedAnchor
```
NOTE: `decide_eq_true_iff` has signature `decide p = true ↔ p` (it may require `[Decidable p]`,
which is in scope). If the name differs in 4.30, use `decide_eq_true_eq` or
`Decidable.decide_eq_true_iff`; `by simp` also closes it.

- [ ] **Step 2: Build**

Run: `lake build VerifiedAnchor.Decision.Agreement`
Expected: success, no `sorry`.

- [ ] **Step 3: Commit**

```bash
git add lean/VerifiedAnchor/Decision/Agreement.lean
git commit -m "feat(decision): prove validates_iff_validatesBool agreement"
```

---

## Task 11: Examples — Withdraw

**Files:**
- Create: `lean/VerifiedAnchor/Examples/Withdraw.lean`

- [ ] **Step 1: Encode the proposal's Withdraw struct and contexts**

Create `lean/VerifiedAnchor/Examples/Withdraw.lean`:
```lean
import VerifiedAnchor.Decision.Agreement

namespace VerifiedAnchor.Examples
open VerifiedAnchor

/-- Program id for the example. -/
def progId : Pubkey := Pubkey.ofBytes (List.replicate 32 7)

/-- Vault layout: `authority : Pubkey` stored right after the 8-byte discriminator. -/
def vaultLayout : FieldLayout := [("authority", 8)]

/-- struct Withdraw { #[account(mut, has_one = authority)] vault; authority: Signer } -/
def withdraw : AccountsStruct where
  programId := progId
  fields :=
    [ { name := "vault"
      , ty := AccountType.account "Vault" vaultLayout progId
      , constraints := [Constraint.mut, Constraint.hasOne "authority"] }
    , { name := "authority"
      , ty := AccountType.signer
      , constraints := [] } ]

/-- An authority key used in the good context. -/
def authKey : Pubkey := Pubkey.ofBytes (List.replicate 32 9)

/-- Vault data: 8 discriminator bytes (the real Vault discriminator) ++ authKey bytes. -/
def vaultData (storedAuth : Pubkey) : ByteArray :=
  (accountDiscriminator "Vault") ++ ⟨storedAuth.toArray⟩

/-- GOOD context: vault owned by program, discriminator correct, stored authority == signer. -/
def goodCtx : Ctx :=
  [ { key := Pubkey.ofBytes (List.replicate 32 1), lamports := 100,
      data := vaultData authKey, owner := progId, rentEpoch := 0,
      isSigner := false, isWritable := true, executable := false }
  , { key := authKey, lamports := 1, data := ByteArray.empty, owner := Pubkey.zero,
      rentEpoch := 0, isSigner := true, isWritable := false, executable := false } ]

/-- TAMPERED context: stored authority does NOT match the signer. -/
def tamperedCtx : Ctx :=
  [ { (goodCtx.get! 0) with data := vaultData (Pubkey.ofBytes (List.replicate 32 2)) }
  , goodCtx.get! 1 ]

-- The checker bites: accepts good, rejects tampered (non-crypto constraints run concretely;
-- the discriminator constraint matches because vaultData uses the real discriminator).
#guard validatesBool withdraw goodCtx = true
#guard validatesBool withdraw tamperedCtx = false

/-- A direct proof against the declarative contract (via the agreement theorem). -/
theorem good_validates : validates withdraw goodCtx := by
  apply validatesBool_sound
  native_decide

end VerifiedAnchor.Examples
```
NOTE: if the `#guard`/`native_decide` lines hang or fail to reduce because `sha256` is
`opaque` (the discriminator constraint cannot compute), do ONE of the following and note
it in the commit:
  (a) Drop the type-implied `discriminator` from this example by using
      `AccountType.uncheckedAccount` with explicit `[Constraint.mut, Constraint.hasOne …]`
      so every remaining constraint is crypto-free and the guards reduce; OR
  (b) keep `Account` but replace `#guard`/`native_decide` with `example : … := by decide`
      restricted to the crypto-free fields.
The intent that MUST hold: an example where `validatesBool` is `true` on a good context and
`false` on a tampered one, plus one proved `validates` lemma. Adjust the encoding to make
that true without `sorry`.

- [ ] **Step 2: Build**

Run: `lake build VerifiedAnchor.Examples.Withdraw`
Expected: success; the two `#guard`s pass; `good_validates` compiles.

- [ ] **Step 3: Commit**

```bash
git add lean/VerifiedAnchor/Examples/Withdraw.lean
git commit -m "feat(examples): Withdraw contract example with checker + proof"
```

---

## Task 12: Root wiring, README, full build

**Files:**
- Modify: `lean/VerifiedAnchor.lean`
- Delete: `lean/VerifiedAnchor/Solana/Placeholder.lean`
- Create: `lean/README.md`

- [ ] **Step 1: Replace the root import to aggregate every module**

Overwrite `lean/VerifiedAnchor.lean`:
```lean
import VerifiedAnchor.Solana.Pubkey
import VerifiedAnchor.Solana.Account
import VerifiedAnchor.Solana.Crypto
import VerifiedAnchor.Solana.Discriminator
import VerifiedAnchor.Solana.Layout
import VerifiedAnchor.Constraints.Ast
import VerifiedAnchor.Constraints.Context
import VerifiedAnchor.Contract.Satisfies
import VerifiedAnchor.Contract.Validates
import VerifiedAnchor.Decision.Check
import VerifiedAnchor.Decision.Agreement
import VerifiedAnchor.Examples.Withdraw
```

- [ ] **Step 2: Remove the placeholder module**

```bash
rm lean/VerifiedAnchor/Solana/Placeholder.lean
```

- [ ] **Step 3: Write the README**

Create `lean/README.md`:
```markdown
# Verified Anchor — Lean library (Milestone 1)

Formal validation contract for Anchor's `#[derive(Accounts)]` account validation.

- `VerifiedAnchor/Solana/` — concrete Solana account model (sha256/ed25519 axiomatized).
- `VerifiedAnchor/Constraints/` — the constraint AST (Rust↔Lean seam for later milestones).
- `VerifiedAnchor/Contract/` — `validates : AccountsStruct → Ctx → Prop`.
- `VerifiedAnchor/Decision/` — `validatesBool` + `validates_iff_validatesBool`.
- `VerifiedAnchor/Examples/` — worked examples.

## Build
```bash
export PATH="$HOME/.elan/bin:$PATH"
lake build
```
```

- [ ] **Step 4: Full clean build with zero sorry**

Run:
```bash
cd /home/parth/Desktop/PARTH/Verification/lean && export PATH="$HOME/.elan/bin:$PATH"
lake build
grep -rn "sorry" VerifiedAnchor/ ; echo "exit: $?"
```
Expected: `lake build` fully green. `grep` prints nothing and `echo` shows `exit: 1`
(no matches). If any `sorry` is found, it is a task failure — go back and close it.

- [ ] **Step 5: Commit**

```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/ && git commit -m "feat(lean): wire root imports, README, remove placeholder; M1 build green"
```

---

## Done-bar verification (run after Task 12)

1. `lake build` green, zero `sorry` (Task 12 Step 4). ✅
2. `validates` defined for every in-scope constraint — `signer, mut, owner, hasOne, seeds,
   init, close, discriminator` all present in `satisfies` (Task 7). ✅
3. `validates_iff_validatesBool` proved (Task 10). ✅
4. `Examples/Withdraw.lean` shows checker true/false on good/tampered + a `validates` lemma
   (Task 11). ✅
5. Everything committed. ✅
```
