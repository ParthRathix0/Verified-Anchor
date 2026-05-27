# Verified Anchor — Milestone 1 Design

*Formal validation contract for Anchor account validation, in Lean 4.*

Status: **approved design** (2026-05-27). Target: Milestone 1 of `verified_anchor_proposal.md`.

---

## 1. Goal and context

Milestone 1 of the Verified Anchor proposal: a Lean 4 library defining the Solana
account model, a constraint language mirroring Anchor's `#[derive(Accounts)]`
annotations, and a **validation contract**

```
validates : AccountsStruct → Ctx → Prop
```

that holds iff every constraint declared in an accounts struct is satisfied by a
concrete set of accounts. This is the standalone, reviewable specification of what
Anchor's account validation *ought* to enforce. It depends on no implementation code.

It also ships an **executable checker** `validatesBool` with a machine-checked proof
that the checker agrees with the declarative contract — giving a runnable reference
oracle for testing the spec and for later milestones.

This is **not** a verification of any macro expansion (Milestones 2–4), not a Rust
crate (M2+), and not cargo integration (M5). It is the foundation those build on.

### Decisions locked during brainstorming
- **Scope**: Milestone 1 only (Lean contract + executable checker + examples).
- **Fidelity**: tech-heavy / Solana-faithful. Model the account world concretely;
  axiomatize only genuinely uninterpretable cryptography (SHA-256, ed25519 on-curve).
- **Spec form**: declarative `Prop` **plus** executable `Bool` checker with an
  agreement theorem.
- **Architecture**: layered Lean library (Approach A).
- **Dependencies**: `batteries` (std4) only. No mathlib — we need `DecidableEq`,
  `Vector`/`ByteArray`, and basic tactics, none of which require mathlib's
  mathematics. Keeps builds fast and the spec self-contained.
- **Toolchain**: Lean `4.30.0`, Lake `5.0.0` (installed via elan).

---

## 2. Repository layout

The repo is shaped for the whole project; Milestone 1 fills `lean/`.

```
verified-anchor/                       (repo root = /home/parth/Desktop/PARTH/Verification)
├── lean/                              ← M1 deliverable (M2–M4 proofs land here later)
│   ├── lakefile.toml                  require batteries
│   ├── lean-toolchain                 leanprover/lean4:v4.30.0
│   ├── VerifiedAnchor.lean            root import aggregating all modules
│   └── VerifiedAnchor/
│       ├── Solana/                    concrete Solana account world
│       │   ├── Pubkey.lean
│       │   ├── Account.lean
│       │   ├── Crypto.lean
│       │   ├── Discriminator.lean
│       │   └── Layout.lean
│       ├── Constraints/               the constraint AST (Rust↔Lean seam for M2+)
│       │   ├── Ast.lean
│       │   └── Context.lean
│       ├── Contract/                  the declarative contract
│       │   ├── Satisfies.lean
│       │   └── Validates.lean
│       ├── Decision/                  executable checker + agreement proof
│       │   ├── Check.lean
│       │   └── Agreement.lean
│       └── Examples/                  worked structs, #eval + lemmas
│           └── Withdraw.lean
├── rust/                              ← M2+ proc-macro crate + M5 integration (empty in M1)
└── docs/superpowers/specs/2026-05-27-verified-anchor-m1-design.md
```

---

## 3. Layer 1 — `VerifiedAnchor.Solana` (concrete account world)

### `Pubkey.lean`
- `abbrev Pubkey := Vector UInt8 32` with `DecidableEq` (derivable).
- `abbrev Lamports := UInt64`.
- Helper constructors / a `systemProgram`, `zero` pubkey for examples.

### `Account.lean`
Faithful to Solana's `AccountInfo` fields:
```lean
structure AccountInfo where
  key        : Pubkey
  lamports    : UInt64
  data        : ByteArray
  owner       : Pubkey
  rentEpoch   : UInt64
  isSigner    : Bool
  isWritable  : Bool
  executable  : Bool
```

### `Crypto.lean` (algorithm concrete, primitives axiomatized)
- `opaque sha256 : ByteArray → ByteArray` with `axiom sha256_size : (sha256 b).size = 32`.
- `opaque isOnCurve : Pubkey → Bool` (ed25519 point test).
- `createProgramAddress : List ByteArray → Pubkey → Option Pubkey` — defined via
  `sha256` of `seeds ++ [programId, "ProgramDerivedAddress"]`, returning `none` when
  on-curve. Concrete algorithm over the opaque hash/curve.
- `findProgramAddress : List ByteArray → Pubkey → Option (Pubkey × UInt8)` — the real
  bump iteration (255 → 0) calling `createProgramAddress`, first off-curve hit wins.
- Stated assumptions captured as axioms only where unavoidable (e.g. `sha256_size`).
  Collision-resistance is **not** asserted in M1 — no proof in M1 needs it; it enters
  with PDA soundness in M4.

### `Discriminator.lean`
- `accountDiscriminator (name : String) : Vector UInt8 8` =
  first 8 bytes of `sha256 ("account:" ++ name).toUTF8`.
- `hasDiscriminator (a : AccountInfo) (d : Vector UInt8 8) : Prop` = first 8 bytes of
  `a.data` equal `d`.

### `Layout.lean` (Borsh-lite, only what `has_one` needs)
- `readPubkey (data : ByteArray) (offset : Nat) : Option Pubkey` — reads 32 bytes at an
  offset (after the 8-byte discriminator) iff in-bounds.
- A `FieldLayout := List (String × Nat)` mapping a named `Pubkey` field to its byte
  offset; `AccountType` for program accounts carries one. This models exactly the
  field access Anchor's `has_one` codegen performs (`account.authority`), without
  implementing general Borsh deserialization.

---

## 4. Layer 2 — `VerifiedAnchor.Constraints` (the load-bearing interface)

This AST is the **seam between the Rust macro (M2) and the Lean proofs**. It is
designed deliberately in M1 so later milestones do not rewrite it.

### `Ast.lean`
```lean
structure SeedSpec where ...        -- literal bytes | reference to another field's key | field data slice
inductive BumpSpec | declared (b : UInt8) | canonical

inductive Constraint
  | signer
  | mut
  | owner          (expected : Pubkey)
  | hasOne         (field : String)                       -- relational
  | seeds          (seeds : List SeedSpec) (bump : BumpSpec)
  | init           (payer : String) (space : Nat) (owner : Pubkey)
  | close          (dest : String)
  | discriminator  (expected : Vector UInt8 8)

inductive AccountType
  | account        (name : String) (layout : FieldLayout) (programId : Pubkey)  -- Account<'info, T>
  | signer                                                                       -- Signer<'info>
  | program        (id : Pubkey)
  | systemAccount
  | uncheckedAccount
  -- type-implied constraints captured by `impliedConstraints : AccountType → List Constraint`

structure AccountField where
  name        : String
  ty          : AccountType
  constraints : List Constraint

structure AccountsStruct where
  programId : Pubkey
  fields    : List AccountField
```
- `impliedConstraints : AccountType → List Constraint` — e.g. `Account<T>` implies
  `owner programId` + `discriminator (accountDiscriminator T)`; `Signer` implies
  `signer`.

### `Context.lean`
- `abbrev Ctx := List AccountInfo` resolved positionally against `fields`.
- `Ctx.lookup (s : AccountsStruct) (c : Ctx) (name : String) : Option AccountInfo`
  resolves a field name (for relational constraints) to its account.
- `WellFormed s c : Prop` — `c.length = s.fields.length` (and any structural
  invariants needed by `validates`).

---

## 5. Layer 3 — `VerifiedAnchor.Contract` (the headline artifact)

### `Satisfies.lean`
`satisfies : AccountsStruct → Ctx → AccountField → Constraint → Prop`, by case:
- `signer` → the field's account `isSigner`.
- `mut` → `isWritable`.
- `owner e` → account `owner = e`.
- `hasOne f` → `readPubkey (account.data) (layoutOffset field f) = some (lookup f).key`.
- `seeds ss b` → `findProgramAddress (resolve ss) programId = some (account.key, bump)`.
- `init payer space owner` → post-init conditions: owner set, rent-exempt lamports,
  discriminator written, payer mutable+signer. (Stated as the predicate Anchor
  guarantees post-`init`.)
- `close dest` → discriminator zeroed/closed marker, lamports moved to `dest`.
- `discriminator d` → `hasDiscriminator account d`.

### `Validates.lean`
```lean
def fieldValidates (s) (c) (f) : Prop :=
  (f.ty.impliedConstraints ++ f.constraints).Forall (satisfies s c f)
def validates (s : AccountsStruct) (c : Ctx) : Prop :=
  WellFormed s c ∧ s.fields.Forall (fun f => fieldValidates s c f)
```

---

## 6. Layer 4 — `VerifiedAnchor.Decision` (executable checker + agreement)

### `Check.lean`
- `checkConstraint : … → Bool` mirroring `satisfies` case-by-case.
- `validatesBool : AccountsStruct → Ctx → Bool`.
- Runs concretely for `signer`, `mut`, `owner`, `hasOne`. Crypto-dependent checks
  (`discriminator`, `seeds`) are expressed through the opaque primitives: they remain
  well-defined and decidable but stay symbolic under `#eval` (do not reduce).

### `Agreement.lean`
```lean
theorem validates_iff_validatesBool (s) (c) : validates s c ↔ validatesBool s c = true
```
Proved by structural decomposition + per-constraint agreement lemmas. Each `satisfies`
case is decidable given `DecidableEq Pubkey`/byte equality, so the lemma is mechanical.

### Optional follow-on (not required for M1 completion)
A concrete reference `sha256` (real SHA-256 in Lean) behind a parameter so
`discriminator`/`seeds` examples also `#eval`. Useful for M6 case studies. Tracked but
out of the M1 done-bar.

---

## 7. Layer 5 — `VerifiedAnchor.Examples`

`Withdraw.lean`: the proposal's example encoded —
```rust
#[derive(Accounts)] struct Withdraw { #[account(mut, has_one = authority)] vault; authority: Signer }
```
- A `goodCtx` where `validatesBool = true` (proved, where crypto-free).
- A `tamperedCtx` (e.g. wrong `vault.authority`) where `validatesBool = false`.
- `#eval` demonstrating the checker bites on the runnable constraints, plus 1–2 proved
  lemmas exercising `validates` directly.

---

## 8. How this carries Milestones 2–7

- **M2–M4** (proof-producing macros): the Rust crate in `rust/` emits a value of the
  `Constraints` AST; per-constraint soundness theorems ("emitted Rust code ⊨
  `validates`") import `Contract` + `Decision`. **The AST is the seam** — fixed in M1.
- **M5** (cargo integration): `lean/` is already a lake project; a cargo build script
  drives `lake build` in the same workflow.
- **M6** (empirical validation): `validatesBool` is the oracle; case studies land in
  `Examples/CaseStudies/`.
- **M7** (release + QEDGen): `VerifiedAnchor.Solana` namespace mirrors `QEDGen.Solana`
  so the account types interoperate.

---

## 9. Scope and non-goals (M1)

**In scope.** Spec all in-scope constraints (`init, mut, has_one, seeds, bump, signer,
owner, close`); the executable checker + agreement theorem; the `Withdraw` examples; a
green `lake build` with zero `sorry`.

**Out of scope (M1).** Any proof about macro expansions (M2+); the Rust proc-macro
crate (M2+); cargo integration (M5); full Borsh deserialization (only Pubkey-field
reads); `#[account(constraint = <expr>)]` arbitrary expressions; Token-2022 extension
constraints; CPI helpers.

---

## 10. Done-bar for Milestone 1

1. `lake build` is green with **no `sorry`** across all modules.
2. `validates` is defined for every in-scope constraint.
3. `validates_iff_validatesBool` is proved.
4. `Examples/Withdraw.lean` shows `validatesBool` true on a good context and false on a
   tampered one, plus at least one direct `validates` lemma.
5. Design doc and code committed to git.
