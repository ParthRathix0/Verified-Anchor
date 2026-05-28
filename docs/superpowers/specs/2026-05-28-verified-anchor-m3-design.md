# Verified Anchor — Milestone 3 Design

*Relational and lifecycle constraints: `has_one`, `init`, `close`.*

Status: **approved design** (2026-05-28). Target: Milestone 3 of `verified_anchor_proposal.md`. Builds on M1 (Lean contract) and M2 (proof-producing macros for mut/signer/owner), both on `master`.

---

## 1. Goal and context

M3 covers the proposal's "substantially harder" constraints. They split into two distinct
mechanisms, which get different machinery:

- **Validation (pure checks that gate an instruction):** `has_one` — read a `Pubkey` field
  from a typed account's data and compare it to another declared account's key. Extends the
  M2 `genValidate` framework.
- **Lifecycle effects (state transitions):** `init` (create an account via a system-program
  CPI, fund rent, write the discriminator) and `close` (drain lamports to a destination,
  write the closed-account marker). These are *effects*, not checks — they need a new
  Hoare-style `{pre} effect {post}` framework, modeled and proven in Lean, with real
  effectful Rust codegen verified by runtime tests.

### Decisions locked during brainstorming
- **Scope:** all three (`has_one`, `init`, `close`), with the full Hoare framework for the
  effectful init/close (the most ambitious option).
- **Runtime tests:** real execution via litesvm. This requires the Solana SBF toolchain,
  which was **installed and verified** during brainstorming (see §6 recipe).
- Build on the existing Rust workspace (`rust/`) and Lean library (`lean/`).

### Toolchain feasibility (verified 2026-05-28)
- `solana-cli 4.0.0` (Agave) installed; `cargo-build-sbf` works with the **recipe in §6**
  (platform-tools rustc on PATH + `--no-rustup-override`; the default rustup path is broken
  by a version-like toolchain name that rustup 1.26.0 rejects).
- A minimal program compiled to `target/deploy/*.so` successfully.
- `litesvm 0.6.1` builds against rustc 1.93.1, **but only with a `openssl = { version =
  "0.10", features = ["vendored"] }` dev-dependency** — system `libssl-dev` headers are
  absent, so OpenSSL must be vendored (cc/perl/make are present).

---

## 2. Repository layout (M3 additions)

```
rust/
├── verified-anchor-macros/src/lib.rs        (MODIFY) add has_one / init / close codegen
├── verified-anchor/
│   ├── src/lib.rs                            (MODIFY) VAError: + WrongHasOne; init/close runtime helpers
│   ├── Cargo.toml                            (MODIFY) [dev-dependencies] litesvm 0.6 + openssl(vendored); solana-sdk for tx building
│   └── tests/
│       ├── behavior.rs                       (MODIFY) has_one accept/reject unit tests
│       └── runtime_lifecycle.rs              (NEW) litesvm: deploy a program, run init/close, assert state
└── verified-anchor-program/                  (NEW) a tiny on-chain program crate used by the litesvm test
    ├── Cargo.toml                            crate-type cdylib+lib; solana-program; the macro
    └── src/lib.rs                            entrypoint with instructions exercising generated init/close/has_one

lean/VerifiedAnchor/Codegen/
├── Generated.lean                            (MODIFY) generalize genConstraint to relational; add genHasOne/genDiscriminator
├── Soundness.lean                            (MODIFY) M3Subset; re-prove lemmas; extend genValidate_sound
├── Lifecycle.lean                            (NEW) applyInit / applyClose state transformers + Hoare theorems
└── ExampleGenerated.lean                     (MODIFY/extend) closed-loop has_one + a lifecycle example

docs/
├── superpowers/specs/2026-05-28-verified-anchor-m3-design.md   (this file)
└── verified-anchor-bridge.md                 (MODIFY) add has_one + init/close (Hoare) correspondence + trust boundary
```

---

## 3. Part 1 — `has_one` extends the validation framework

### 3.1 Generalize `genConstraint`
M2's `genConstraint (a : AccountInfo) : Constraint → Bool` only sees one account — too weak
for relational constraints. Generalize to:
```lean
def genConstraint (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) :
    Constraint → Bool
  | .signer        => (Ctx.atField s c idx).any (·.isSigner)        -- via a none-safe helper
  | .mut           => (Ctx.atField s c idx).any (·.isWritable)
  | .owner e       => (Ctx.atField s c idx).any (fun a => decide (a.owner = e))
  | .discriminator d => (Ctx.atField s c idx).any (fun a => decide (hasDiscriminator a d))
  | .hasOne field  => genHasOne s c idx f field
  | _              => false   -- init/close/seeds handled outside genValidate (see Part 2 / M4)
```
where `genHasOne` mirrors the M1 `satisfies (.hasOne …)`: resolve `a` at `idx`, the offset via
`f.ty.layoutOffsetOf field`, read the Pubkey from `a.data`, and compare to `(Ctx.lookup s c
field).key` — all none-safe (any missing piece ⇒ `false`). (`Option.any`/a small
`optionAll` helper keeps each case a `Bool`.)

Re-prove the M2 per-constraint lemmas (`genConstraint_{signer,mut,owner}_iff`) against the
new 4-argument signature (mechanical — they already had `s c idx f` in scope), and add
`genHasOne_iff` and `genDiscriminator_iff` vs M1's `satisfies`.

### 3.2 `M3Subset` and extended soundness
```lean
def isM3Constraint : Constraint → Bool
  | .signer | .mut | .owner _ | .hasOne _ | .discriminator _ => true | _ => false
def isM3Type : AccountType → Bool := fun _ => true   -- .account now ALLOWED (needed for has_one layout)
def M3Subset (s) : Prop := ∀ f ∈ s.fields, isM3Constraint-only over (impliedConstraints ++ constraints)
```
Admitting `.account` brings its implied `discriminator (accountDiscriminator typeName)`,
which goes through the opaque `sha256`. Consequences (the same honest wall as M1's Withdraw):
- `genValidate_sound : M3Subset s → (genValidate s c = true ↔ validates s c)` still **holds
  and is provable** (both sides reference the same opaque `hasDiscriminator`).
- `genValidate` is **not fully `decide`-able to true/false** for discriminator-bearing
  structs. The closed-loop example therefore demonstrates `has_one` via `checkConstraint` on
  concrete data, and proves the full `genValidate_sound` instantiation symbolically.

`genValidate`/`genFieldValidate` are updated to thread `s c idx f` into the generalized
`genConstraint`.

---

## 4. Part 2 — the Hoare framework for `init` / `close` (the new core)

`Codegen/Lifecycle.lean`. State = `Ctx` (the account list). Effects are partial transformers
`Ctx → Option Ctx` (return `none` when a precondition fails).

### 4.1 Effect transformers (model the *specified effect* of the system-program CPI)
```lean
/-- Model of Anchor `init`: a system-program create_account CPI funded by `payer`, then the
    discriminator write. `rent` lamports move payer→target; target gets `owner`, `space+8`
    bytes, and the 8-byte discriminator. Fails if payer can't sign/pay or target is live. -/
def applyInit (idx payerIdx : Nat) (space : Nat) (owner : Pubkey) (disc : ByteArray)
    (rent : UInt64) (c : Ctx) : Option Ctx

/-- Model of Anchor `close`: move all of target's lamports to `dest`, write the closed-account
    marker, zero target lamports. Fails if dest missing. -/
def applyClose (idx destIdx : Nat) (c : Ctx) : Option Ctx
```
Each updates the two affected accounts in the list (target + payer/dest) and leaves others
unchanged.

### 4.2 Hoare theorems (the deliverable)
The post-state must satisfy the **same** M1 `satisfies` post-conditions the contract already
encodes for `.init`/`.close`:
```lean
/-- {precondition} applyInit {contract init post}. -/
theorem init_establishes_post
    (h : applyInit idx payerIdx space owner disc rent c = some c') :
    satisfies (structForInit …) c' idx (fieldForInit …) (Constraint.init payerName space owner)

theorem close_establishes_post
    (h : applyClose idx destIdx c = some c') :
    satisfies (structForClose …) c' idx (fieldForClose …) (Constraint.close destName)
```
(Statement shapes finalized in the plan; the essential content: *the modeled generated effect
establishes the exact M1 post-condition*, i.e. `{pre} generated_code {contract post}`.)
`#print axioms` on these must show no `sorryAx`.

---

## 5. Part 3 — Rust codegen + runtime tests

### 5.1 Codegen (`verified-anchor-macros`)
- `has_one`: generate the relational check — borrow `accounts[i].data`, read 32 bytes at the
  field offset, compare to `accounts[j].key`; `Err(VAError::WrongHasOne { field })` on
  mismatch. Pure → unit-testable. Add `WrongHasOne` to `VAError`.
- `init`: generate a `solana_program::system_instruction::create_account` CPI via
  `solana_program::program::invoke` (payer → new account, `space+8`, owner = program), then
  write the discriminator into the new account's data. Effectful.
- `close`: generate the lamport drain to `dest` + write the closed-account marker. Effectful.
- `lean_spec` extends to emit `Constraint.hasOne "field"` (and the typed account's
  `AccountType.account name layout programId` so the layout is present); `init`/`close` emit
  their `Constraint.init`/`.close` forms.

### 5.2 Runtime tests (litesvm) — the real-execution gate
- A small program crate `verified-anchor-program` with an entrypoint whose instructions use
  the generated `init`/`close`/`has_one`. Built to BPF `.so` via the **§6 recipe**.
- `tests/runtime_lifecycle.rs` (in `verified-anchor`, behind the litesvm + vendored-openssl
  dev-deps): a `build.rs` or test-time step invokes `cargo-build-sbf` for the program crate,
  then litesvm loads the `.so`, sends an `init` tx (asserts the account is created, owned by
  the program, funded, discriminator written), a `close` tx (asserts lamports moved to dest +
  marker), and a `has_one` tx (asserts accept on match / custom error on mismatch).
- `has_one` also has fast native unit tests in `behavior.rs` (no runtime needed).

---

## 6. Confirmed SBF build recipe (load-bearing for the plan)

```bash
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd <program-crate> && cargo-build-sbf --no-rustup-override
# -> target/deploy/<crate>.so
```
The platform-tools rustc (has the `sbf-solana-solana` target) MUST be first on PATH;
`--no-rustup-override` avoids the rustup toolchain-name bug. litesvm's transitive
`openssl-sys` needs `openssl = { version = "0.10", features = ["vendored"] }` as a
dev-dependency (no system `libssl-dev`).

---

## 7. Part 4 — trust boundary (bridge doc addendum)

- **Proven:** `genValidate ≡ validates` extended to `has_one`/discriminator (M3Subset); and
  `applyInit`/`applyClose` establish the M1 `init`/`close` post-conditions.
- **Modeled, trusted (new for M3):** that `solana_program::system_instruction::create_account`'s
  on-chain effect on account state matches `applyInit`/`applyClose` (its *documented* effect).
  We model the effect, not the CPI dispatch.
- **Transcription (documented + now runtime-tested):** the generated Rust `validate`/`init`/
  `close` match the Lean models — backed by litesvm execution, not just shared vectors.
- **Out of scope:** the validator/runtime correctness, rustc/LLVM/sBPF codegen fidelity,
  proving the CPI dispatch mechanism.

---

## 8. Scope / non-goals (M3)

**In.** `has_one` (codegen + soundness extension); the Hoare framework (`applyInit`/
`applyClose` + establishes-post theorems); effectful `init`/`close` codegen; litesvm runtime
tests; the bridge addendum; an extended closed-loop example.

**Out.** `seeds`/PDA derivation (M4); arbitrary `#[account(constraint = expr)]`; full
`anchor-lang` API compatibility + cargo plugin (M5); proving the system-program CPI dispatch
or validator; modeling reallocation / multiple inits in one ix.

---

## 9. Done-bar for Milestone 3

1. `cargo build`/`cargo test` green in `rust/` (has_one unit tests pass; macro compiles).
2. The `verified-anchor-program` crate builds to a `.so` via the §6 recipe.
3. litesvm `runtime_lifecycle.rs`: init creates+funds+marks the account, close drains+marks,
   has_one accepts match / rejects mismatch — all asserted against real execution.
4. `lake build` green, zero `sorry`, including the new/edited `Codegen` modules.
5. `genValidate_sound` re-proved at `M3Subset`; `genHasOne_iff`/`genDiscriminator_iff` proved;
   `init_establishes_post`/`close_establishes_post` proved; all with no `sorryAx`.
6. Extended closed-loop example: `has_one` via `checkConstraint` on concrete data + a
   `genValidate_sound` instantiation; a lifecycle example using `applyInit`/`applyClose`.
7. `docs/verified-anchor-bridge.md` updated with the has_one + init/close (Hoare) rows and
   the new modeled-effect trust statement.
8. M1 + M2 still green (no regressions).
