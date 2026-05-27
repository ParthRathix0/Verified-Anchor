# Verified Anchor — Milestone 2 Design

*Proof-producing macro expansions for the foundational constraints (`mut`, `signer`, `owner`).*

Status: **approved design** (2026-05-27). Target: Milestone 2 of `verified_anchor_proposal.md`. Builds on Milestone 1 (the Lean validation contract, already on `master`).

---

## 1. Goal and context

Milestone 2 delivers the first *proof-producing macro expansion*: a real Rust procedural
macro that generates Solana account-validation code for the three simplest in-scope
constraints — `mut`, `signer`, `owner` — paired with a machine-checked Lean proof that the
generated validator's logic implements the Milestone 1 contract `validates`.

### The core challenge (stated honestly)

There is no embedding of Rust's runtime (or sBPF) operational semantics in Lean. We
therefore cannot literally prove "the emitted Rust, when executed, implements `validates`".
The proposal anticipates exactly this and calls for a *conservative* bridge: the macro
emits a structured representation of its expansion, and a Lean checker proves that
representation implements the contract. M2 realizes that bridge.

What M2 proves, precisely: an **operational Lean model** of the generated validator
(`genValidate`, built from per-constraint Bool checks `genSigner`/`genMut`/`genOwner`)
**agrees with the declarative M1 contract** for every struct in the M2 constraint subset.
The Rust `validate` code is a faithful, clause-by-clause transcription of that Lean model;
the transcription gap is documented and bolstered by differential tests, not proven.

### Decisions locked during brainstorming
- **Approach**: modeled codegen + generic soundness proof (not full Rust-semantics
  embedding, not test-only).
- **Proof target**: BOTH a generic codegen-soundness theorem AND per-struct `.lean`
  emission as the tangible bridge artifact.
- **Rust target**: real Solana types — `solana_program::account_info::AccountInfo`
  (fields `is_signer`/`is_writable`/`owner` are exactly the three constraints). Verified to
  build with rustc 1.93.1 (`solana-program v2.3.0`, ~11s). Full `anchor-lang` API compat is
  deferred to M5.
- **Toolchain**: Rust 1.93.1 / cargo (crates.io reachable; `syn`/`quote`/`proc-macro2` and
  `solana-program 2` all fetch & build). Lean 4.30.0 / Lake 5.0.0 (M1 library).

---

## 2. Repository layout (M2 additions)

```
rust/                                    NEW cargo workspace
├── Cargo.toml                           [workspace] members = the two crates below
├── verified-anchor-macros/              proc-macro crate
│   ├── Cargo.toml                       proc-macro = true; deps syn(full), quote, proc-macro2
│   └── src/lib.rs                       #[derive(VerifiedAccounts)] + parsing + codegen + lean_spec
└── verified-anchor/                     runtime + tests crate
    ├── Cargo.toml                       deps: solana-program "2", verified-anchor-macros (path)
    ├── src/lib.rs                       `Validate` trait, `VAError`, re-export of the derive
    └── tests/
        ├── behavior.rs                  validate() accepts good / rejects each violation
        └── lean_spec.rs                 lean_spec() emits the expected AccountsStruct literal

lean/VerifiedAnchor/Codegen/             NEW Lean layer in the existing library
├── Generated.lean                       genSigner/genMut/genOwner; genValidate (operational)
├── Soundness.lean                       per-constraint lemmas + generic soundness theorem
└── ExampleGenerated.lean                Rust-emitted spec for one struct + checked obligation

docs/
├── superpowers/specs/2026-05-27-verified-anchor-m2-design.md   (this file)
└── verified-anchor-bridge.md            Rust↔Lean correspondence + trust boundary
```

The Lean root `lean/VerifiedAnchor.lean` gains imports for the new `Codegen/` modules.

---

## 3. Rust side

### 3.1 `verified-anchor-macros` (proc-macro crate)

`#[derive(VerifiedAccounts)]` applies to a struct of named fields. Each field may carry a
`#[account(...)]` attribute listing M2 constraints:
- `#[account(signer)]` — require the account is a signer.
- `#[account(mut)]` — require the account is writable.
- `#[account(owner = EXPR)]` — require `account.owner == EXPR` (EXPR is a Rust expression
  evaluating to a `&Pubkey`/`Pubkey`).
Multiple may combine, e.g. `#[account(mut, owner = crate::ID)]`. A field with a `Signer`
type (modeled by the field being declared with our marker) implies `signer`.

The derive generates two items in an `impl` block:

1. **`fn validate(&self, accounts: &[AccountInfo]) -> Result<(), VAError>`** — checks each
   field's constraints in declaration order, short-circuiting on the first failure. Each
   check is a direct transcription of the Lean `gen*` function:
   - signer → `if !accounts[i].is_signer { return Err(VAError::MissingSigner { field }); }`
   - mut → `if !accounts[i].is_writable { return Err(VAError::NotWritable { field }); }`
   - owner → `if accounts[i].owner != &expected { return Err(VAError::WrongOwner { field }); }`
   Returns `Ok(())` if all pass. The field↔index mapping is positional (field declaration
   order = account slice order), matching the Lean `Ctx`.

2. **`fn lean_spec() -> String`** — returns the Milestone-1 `AccountsStruct` literal for this
   struct, as Lean source text (e.g. `{ programId := …, fields := [ … ] }`). This is the
   per-struct bridge artifact: pasted/written into the Lean project, it lets the generic
   soundness theorem and `genValidate` be instantiated and `#eval`/`decide`-checked for the
   user's actual struct. `owner = EXPR` literals that are compile-time-unknown pubkeys are
   emitted as a documented placeholder symbol the Lean side treats opaquely.

The proc-macro crate depends ONLY on `syn` (full), `quote`, `proc-macro2` — it does not link
Solana, so it stays light and fast.

### 3.2 `verified-anchor` (runtime + tests crate)

- `trait Validate { fn validate(&self, accounts: &[AccountInfo]) -> Result<(), VAError>; }`
  (the derive implements this).
- `enum VAError { MissingSigner { field: &'static str }, NotWritable { field: &'static str },
  WrongOwner { field: &'static str } }` — `Debug`/`Display`/`Error`.
- Re-exports `verified_anchor_macros::VerifiedAccounts`.
- `tests/behavior.rs`: build `AccountInfo` vectors and assert `validate` returns `Ok` for a
  fully-valid set and the specific `Err` variant for each single-constraint violation.
- `tests/lean_spec.rs`: assert `lean_spec()` equals the expected literal string for a sample
  struct (guards against codegen drift from the Lean AST).

---

## 4. Lean side (the proof)

New layer `VerifiedAnchor.Codegen`, importing the M1 `Contract`/`Decision` modules.

### 4.1 `Generated.lean` — the operational model of the generated code
```lean
def genSigner (a : AccountInfo) : Bool := a.isSigner
def genMut    (a : AccountInfo) : Bool := a.isWritable
def genOwner  (expected : Pubkey) (a : AccountInfo) : Bool := a.owner == expected

/-- Operational check for a single M2 constraint (mirrors one emitted Rust `if`). -/
def genConstraint (a : AccountInfo) : Constraint → Bool
  | .signer        => genSigner a
  | .mut           => genMut a
  | .owner e       => genOwner e a
  | _              => false   -- out of the M2 subset

/-- The generated validator: every field, every (implied ++ explicit) constraint, in order,
    short-circuiting. Mirrors the emitted Rust `validate`. -/
def genValidate (s : AccountsStruct) (c : Ctx) : Bool := …
```
`genValidate` walks `s.fields.zipIdx`, looks up each account via `Ctx.atField`, and folds the
constraint checks with `&&` (short-circuit). A missing account (index out of range) yields
`false`, matching both the contract's soundness and a real out-of-bounds failure.

### 4.2 `Soundness.lean` — the proof
- **Per-constraint lemmas** (the "proved once per constraint kind" deliverable):
  `genSigner a = true ↔ satisfies s c idx f .signer` (given `Ctx.atField s c idx = some a`),
  and likewise for `mut`, `owner`.
- **M2 subset predicate**: `M2Subset s : Prop` — every field's `ty ∈ {signer,
  uncheckedAccount, systemAccount, program}` and every explicit constraint `∈ {mut, signer,
  owner}`. (Decidable.) This excludes `.account` (which implies the out-of-subset
  `discriminator`), keeping M2 honest; typed accounts arrive in M3.
- **Generic soundness theorem**:
  `theorem genValidate_sound (s c) (h : M2Subset s) : genValidate s c = true ↔ validates s c`.
  Proof: under `M2Subset`, every constraint reaching the contract is in {signer,mut,owner};
  rewrite each via the per-constraint lemmas; the short-circuit `&&` fold equals the
  `List.all`/`Forall` structure of `validates`; conjoin with `WellFormed` (genValidate's
  index coverage encodes well-formedness). No `sorry`.

### 4.3 `ExampleGenerated.lean` — closing the loop
Paste the Rust `lean_spec()` output for one example struct (e.g. a `Transfer` with a `mut`
vault + `signer` authority + `owner` check), then:
- `#guard genValidate exStruct goodCtx = true`
- `#guard genValidate exStruct tamperedCtx = false`
- `theorem ex_sound : validates exStruct goodCtx := (genValidate_sound _ _ (by decide)).mp (by decide)`
This demonstrates the full pipeline: Rust struct → emitted Lean spec → checked contract
obligation discharged via the generic theorem.

---

## 5. Bridge & trust boundary (`docs/verified-anchor-bridge.md`)

A clause-by-clause table mapping each generated Rust `if` to its Lean `gen*` function and to
the M1 `satisfies` case it discharges. Explicit statement of the trust boundary:
- **Proven**: `genValidate` (the Lean model) ≡ `validates` (M1 contract) for the M2 subset.
- **Transcription (documented, tested, not proven)**: the Rust `validate` body matches
  `genValidate` clause-for-clause; backed by shared accept/reject test vectors run in both
  the Rust `tests/behavior.rs` and the Lean `#guard`s.
- **Not addressed**: rustc/LLVM/sBPF codegen fidelity (out of scope for all source-level
  verification; same as CompCert-style boundaries).

---

## 6. Scope / non-goals (M2)

**In scope.** `mut`, `signer`, `owner` constraints; the `#[derive(VerifiedAccounts)]`
proc-macro generating `validate` + `lean_spec`; the `verified-anchor` runtime crate and its
tests; the Lean `Codegen` layer with per-constraint lemmas and the generic `genValidate_sound`
theorem; one closed-loop `ExampleGenerated`; the bridge doc.

**Out of scope (M2).** `has_one` (M3), `init`/`close` (M3), `seeds`/`bump` (M4), typed
`Account<T>` discriminator codegen (M3); full `anchor-lang` API compatibility and the cargo
plugin (M5); empirical exploit study (M6); any rustc/sBPF operational semantics.

---

## 7. Done-bar for Milestone 2

1. `cargo build` and `cargo test` green in `rust/` (proc-macro crate + runtime crate; behavior
   and lean_spec tests pass).
2. `lake build` green with **zero `sorry`** including the new `Codegen` modules.
3. `genValidate_sound` proved; `#print axioms` shows no `sorryAx`.
4. The three per-constraint lemmas proved.
5. `ExampleGenerated.lean`: `genValidate` true on good / false on tampered, and `ex_sound`
   proved via the generic theorem.
6. `docs/verified-anchor-bridge.md` documents the Rust↔Lean correspondence and trust boundary.
7. Everything committed; M1 remains green (no regressions).
