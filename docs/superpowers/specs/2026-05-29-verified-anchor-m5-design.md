# Verified Anchor — Milestone 5 Design

*Cargo integration & developer experience: make the verified loop a tool a developer actually uses.*

Status: **approved design** (2026-05-29). Target: Milestone 5 of `verified_anchor_proposal.md`. Builds on M1–M4 (all on `master`).

---

## 1. Goal and context

Through M4 the closed loop is **manual**: `#[derive(VerifiedAccounts)]` emits a `lean_spec()` *runtime String*, which a human copies into a Lean file and hand-proves. M5 automates that and packages it so a developer can:

1. write `#[derive(VerifiedAccounts)]` structs and `cargo build` normally (fast, no Lean needed),
2. get an **immediate** compile error if a struct uses a constraint outside the verified subset,
3. run **`cargo verified-anchor check`** to discharge the formal proof obligations via `lake` (locally + in CI),
4. follow **migration docs** to port a stock-Anchor program.

### Decisions locked during brainstorming

1. **Trigger = a `cargo verified-anchor` subcommand** (a `cargo-verified-anchor` binary), not a `build.rs`. Rationale: every production Rust verifier ships this way (`cargo kani`/`prusti`/`creusot`/`miri`); a `build.rs` that runs `lake` on every `cargo build` would break compilation for anyone without the Lean toolchain (downstream deps, docs.rs, rust-analyzer on every keystroke). `cargo build` stays fast and Lean-free; the proof check is a deliberate, CI-gateable step.
2. **Fast in-build feedback = macro `compile_error!`.** The derive rejects unsupported constraints at `cargo build` time (no Lean needed), pointing at the field span and listing the supported subset. The `lake` step is the formal confirmation.
3. **Auto-collection = `inventory` + a `cargo test` emitter.** The derive auto-registers each struct (`inventory`); a one-line `verified_anchor::emit_specs!()` (placed in the user's lib) expands to a `#[test]` that enumerates **all** registered structs and writes their specs. Per-struct is fully automatic (you cannot forget a struct). **Feasibility-probed (2026-05-29):** cross-crate inventory collection in a *standalone harness* dead-strips (the registering crate's statics get dropped); collection MUST run as a same-crate `#[test]` (test fns are linker roots). Hence `emit_specs!()` lives in the user's lib crate, not in `tests/`.
4. **Check scope = validation AND lifecycle**, via **generic theorems + a uniform per-struct `decide`** (chosen over both "Rust templates Lean theorem text" and "a bespoke Lean elaborator"). Validation is already generic (`genValidate_sound` at `M4Subset`). M5 adds the matching generic lifecycle theorem.

### Feasibility probed (2026-05-29) — both load-bearing assumptions confirmed

- **`inventory` cross-crate**: a standalone harness sees `[]` (dead-stripping); a **same-crate `#[test]`** sees all submissions. → emitter is a lib `#[test]`.
- **Generic lifecycle theorem**: `StructLifecycleWF s → <init/close posts for every lifecycle field>` was stated and **proved clean** (`[propext, Quot.sound]`, zero `sorry`, first try) directly from the existing `init/close_establishes_post`. So per-struct lifecycle checking is a `decide`, needing no new hard proof and no per-struct Lean templating.

---

## 2. Repository layout (M5 additions)

```
rust/
├── cargo-verified-anchor/            (NEW) the subcommand
│   ├── Cargo.toml                    [[bin]] name = "cargo-verified-anchor"
│   ├── src/main.rs                   `cargo verified-anchor check [--json] [--lean-dir P] [-p CRATE]`
│   ├── src/collect.rs                run the emitter test, read spec files
│   ├── src/generate.rs               specs -> check.lean (unit-tested)
│   ├── src/discharge.rs              lake build + lake env lean; parse pass/fail
│   └── tests/cli.rs                  integration test against verified-anchor-example
├── verified-anchor/
│   ├── Cargo.toml                    (MODIFY) + inventory = "0.3"
│   └── src/lib.rs                    (MODIFY) SpecEntry + inventory::collect! + pub use inventory
│                                              + emit_specs! macro + collect_specs()/write_spec_files()
├── verified-anchor-macros/src/lib.rs (MODIFY) emit inventory::submit! per derive (name, lean_spec fn,
│                                              has_lifecycle); compile_error! for unsupported constraints
└── verified-anchor-example/          (NEW) worked end-to-end user crate
    ├── Cargo.toml                    lib; depends on verified-anchor; has emit_specs!()
    └── src/lib.rs                    a PDA-validated struct + an init/close struct

lean/VerifiedAnchor/
├── Codegen/StructLifecycle.lean      (NEW) StructLifecycleWF + lifecyclePost + lifecycle_sound (the probe)
└── (root) VerifiedAnchor.lean        (MODIFY) import Codegen.StructLifecycle

docs/
├── migrating-from-anchor.md          (NEW) supported subset, workflow, attr mapping, trust boundary
├── verified-anchor-bridge.md         (MODIFY) note automated check + generic lifecycle theorem
└── superpowers/specs/2026-05-29-verified-anchor-m5-design.md  (this file)
```

---

## 3. Part A — Lean: the generic lifecycle theorem

`lean/VerifiedAnchor/Codegen/StructLifecycle.lean` (verbatim from the validated probe; namespace `VerifiedAnchor`):

- `initDisc : ByteArray := ByteArray.mk (Array.replicate 8 0)` — the fixed 8-byte discriminator the codegen writes.
- `lifecyclePost (s) (idx) : Constraint → Prop` — for `.init payer space owner`: for the resolved `payerIdx`, `∀ rent c c', applyInit idx payerIdx space owner initDisc rent c = some c' → ∃ a, c'.accounts[idx]? = some a ∧ a.owner = owner ∧ space + 8 ≤ a.data.size`; for `.close dest`: the analogous `applyClose` post; else `True`.
- `lifecycleClauseWF (s) (idx) : Constraint → Bool` — init/close clauses require the resolved payer/dest index `≠ idx` (so the `applyInit/applyClose` guard holds); else `true`.
- `StructLifecycleWF (s) : Prop := ∀ p ∈ s.fields.zipIdx, ∀ k ∈ p.1.constraints, lifecycleClauseWF s p.2 k = true` (+ `Decidable` instance).
- `theorem lifecycle_sound (s) (h : StructLifecycleWF s) : ∀ p ∈ s.fields.zipIdx, ∀ k ∈ p.1.constraints, lifecyclePost s p.2 k` — proved by `cases` on the constraint, discharging `idx ≠ payerIdx`/`idx ≠ destIdx` from the WF bool (`of_decide_eq_true`) and `initDisc.size = 8` by `decide`, then applying `init/close_establishes_post`.

Imported by the root `VerifiedAnchor.lean`. `#print axioms lifecycle_sound` must be `[propext, Quot.sound]`.

**Per-struct obligation shape** (what the tool generates, §4C): for a struct with no lifecycle fields, `example : M4Subset <spec> := by decide` (validation soundness via the existing `genValidate_sound`); for a struct with init/close fields, `example : StructLifecycleWF <spec> := by decide` (lifecycle posts via `lifecycle_sound`). No per-struct theorem text beyond these two decidable forms.

---

## 4. Part B — Rust: collection + the subcommand

### 4.1 `verified-anchor` lib additions
```rust
pub use inventory;   // so the macro can emit ::verified_anchor::inventory::submit!

pub struct SpecEntry {
    pub name: &'static str,
    pub lean_spec: fn() -> String,   // the AccountsStruct literal
    pub has_lifecycle: bool,         // true if any field has init/close
}
inventory::collect!(SpecEntry);

pub fn collect_specs() -> Vec<&'static SpecEntry> { inventory::iter::<SpecEntry>.into_iter().collect() }

/// Write one `<name>.json` per registered struct into `dir` ({name, lean_spec, has_lifecycle}).
pub fn write_spec_files(dir: &std::path::Path) -> std::io::Result<()> { /* serialize collect_specs() */ }

/// Drop ONE call in your lib (e.g. bottom of src/lib.rs). Expands to a #[cfg(test)] #[test]
/// that, when VERIFIED_ANCHOR_SPEC_DIR is set, writes spec files for every derived struct.
#[macro_export] macro_rules! emit_specs { () => {
    #[cfg(test)] #[test] fn __verified_anchor_emit_specs() {
        if let Ok(dir) = std::env::var("VERIFIED_ANCHOR_SPEC_DIR") {
            ::verified_anchor::write_spec_files(std::path::Path::new(&dir)).unwrap();
        }
    }
}; }
```
(JSON via a tiny hand-rolled writer or `serde_json` as a dev/normal dep — decided in the plan; hand-rolled keeps deps minimal since the shape is trivial.)

### 4.2 macro changes (`verified-anchor-macros`)
- **Emit lifecycle constraints in `lean_spec` (currently skipped — load-bearing fix).** Today `lean_constraint` returns `String::new()` for `Init`/`Payer`/`Space`/`Close`, so an init/close struct's emitted `AccountsStruct` carries NO `Constraint.init`/`.close`. That would make `StructLifecycleWF <spec>` *vacuously* true → a meaningless lifecycle obligation (false confidence). M5 must emit `Constraint.init "<payer>" <space> Pubkey.zero` and `Constraint.close "<dest>"` into the field's constraint list (owner is the `Pubkey.zero` placeholder, matching `ExampleGenerated`'s lifecycle example; the real-program-id correspondence is the documented transcription gap). Add a `lean_spec` test asserting an init struct's emitted literal includes `Constraint.init`.
- Per derived struct, additionally emit `::verified_anchor::inventory::submit! { ::verified_anchor::SpecEntry { name: "<Ident>", lean_spec: || <Ident>::lean_spec(), has_lifecycle: <bool> } }`. `has_lifecycle` = any field carries `Init`/`Close`.
- **`compile_error!` for unsupported constraints.** Today the parser errors on unknown idents generically. Strengthen: recognize the common stock-Anchor constraints verified-anchor does NOT support (`realloc`, `zero`, `rent_exempt`, `constraint`, `token`, `mint`, `associated_token`, `seeds::program`, `executable`, `address`, …) and emit a span-located `compile_error!`: ``constraint `realloc` is not supported by verified-anchor (supported: signer, mut, owner, has_one, seeds, bump, init, payer, space, close); see docs/migrating-from-anchor.md``. Unknown idents get the same message. Malformed combos (`init` without `payer`+`space`) already/also error clearly.

### 4.3 the `cargo verified-anchor check` pipeline (`cargo-verified-anchor`)
1. **collect**: clear `target/verified-anchor/specs/`; set `VERIFIED_ANCHOR_SPEC_DIR` to its absolute path; run `cargo test [-p CRATE] --lib __verified_anchor_emit_specs` (filter to the emitter; `--lib` so it runs in the registering crate where inventory sees same-crate submits). Read the resulting `*.json`.
2. **generate** (`generate.rs`, unit-tested): write `target/verified-anchor/check.lean` = `import VerifiedAnchor\nopen VerifiedAnchor\n` + per struct one `example` chosen by `has_lifecycle` (M4Subset vs StructLifecycleWF), each `:= by decide`, with a trailing comment naming the struct.
3. **discharge** (`discharge.rs`): locate the Lean project (`--lean-dir`, else `$VERIFIED_ANCHOR_LEAN_DIR`, else a sensible default relative to the workspace); `lake build` (cached) then `lake env lean <check.lean>`. Exit 0 ⇒ all obligations discharged.
4. **report**: per struct, `✓ <name> (validation|lifecycle)` or, on a `decide` failure, the struct + which obligation failed. `--json` emits a machine-readable summary for CI. Missing `lake`/`elan` ⇒ an actionable "install the Lean toolchain (see HANDOVER)" message, non-zero exit.

---

## 5. Part C — example crate + migration docs

- **`verified-anchor-example`**: a small Solana-style lib using `#[derive(VerifiedAccounts)]` — at least one PDA-validated struct (`seeds`+`bump`) and one init/close struct — plus one `emit_specs!()`. It (a) `cargo build`s, (b) passes `cargo verified-anchor check`. This is the "a developer can actually use it" artifact and the fixture for the CLI integration test.
- **`docs/migrating-from-anchor.md`**: the supported subset; the three-step workflow (`cargo build` → fix `compile_error!`s → `cargo verified-anchor check`); a table mapping stock-Anchor `#[account(...)]` attributes to supported / unsupported (with the verified-anchor equivalent or "not yet"); and the honest trust boundary (link `verified-anchor-bridge.md`). State plainly what is NOT covered.

---

## 6. Testing

- **Lean**: `lake build` green incl. `StructLifecycle.lean`; `#print axioms lifecycle_sound = [propext, Quot.sound]`; a `#guard`/example exercising `StructLifecycleWF` on a concrete lifecycle struct (crypto-free → reduces).
- **generator unit tests** (`cargo-verified-anchor`): given spec JSON (validation and lifecycle cases), assert the exact `check.lean` lines.
- **CLI integration test** (`tests/cli.rs`): run `cargo verified-anchor check -p verified-anchor-example` against the example crate; assert exit 0 and the expected per-struct report. (Marked `#[ignore]`-able / gated behind the Lean toolchain being present, like the litesvm tests gate on SBF.)
- **negative compile-fail test** (`trybuild`): a struct using `#[account(realloc = …)]` (or another unsupported constraint) must fail to compile with the verified-anchor message.
- **native suite stays green**: existing behavior/lean_spec tests unaffected (the macro's added `submit!` + `compile_error!` must not regress M2–M4 structs).

---

## 7. Trust boundary (bridge-doc addendum)

M5 changes **how obligations are produced and checked**, not what is proven. The bridge doc gains: (a) the automated flow (Rust `lean_spec` → generated `check.lean` → `lake`) replaces hand-copying — but the Rust↔Lean correspondence is still transcription (now mechanically regenerated); (b) the generic lifecycle theorem `lifecycle_sound` (per-struct lifecycle = `decide StructLifecycleWF`), alongside the existing per-struct validation = `decide M4Subset`. No new modeling axioms.

---

## 8. Scope / non-goals (M5)

**In.** The `cargo verified-anchor` subcommand (collect → generate → discharge → report); `inventory` auto-registration + `emit_specs!`; the macro `compile_error!` for unsupported constraints; the generic lifecycle theorem; a worked example crate; migration docs; the bridge addendum; tests (generator unit, CLI integration, compile-fail, axiom hygiene).

**Out.** Publishing the Lean library as a standalone lake dependency for fully-external consumers (M5 locates a bundled/configured `lean/` dir; true crates.io-style distribution is later). Widening the verified constraint subset. A `cargo verified-anchor` that *auto-installs* the Lean toolchain. IDE/LSP surfacing of obligations. Multi-crate workspaces beyond running `emit_specs!()` per crate. Empirical exploit study (M6); release/QEDGen (M7).

---

## 9. Done-bar for Milestone 5

1. `lake build` green incl. `Codegen/StructLifecycle.lean`; `lifecycle_sound` proved, `#print axioms = [propext, Quot.sound]`, zero `sorry`. M1–M4 still green.
2. The derive auto-registers structs (`inventory`); `verified_anchor::emit_specs!()` in a lib writes spec JSON for every derived struct when `VERIFIED_ANCHOR_SPEC_DIR` is set.
3. `verified-anchor-macros` emits a clear `compile_error!` for unsupported constraints (covered by the `trybuild` negative test); M2–M4 structs still compile.
4. `cargo verified-anchor check -p verified-anchor-example` exits 0, reporting each struct's discharged guarantee (validation or lifecycle); the example's init/close struct's emitted `lean_spec` includes `Constraint.init/.close` so its `StructLifecycleWF` discharge is **non-vacuous**; generator unit tests pass.
5. `verified-anchor-example` builds with `cargo build` and passes the check end-to-end.
6. `docs/migrating-from-anchor.md` exists (subset, workflow, attribute mapping, trust boundary); bridge doc updated.
7. `cargo test -p verified-anchor` (behavior + lean_spec) still green (no regression from the macro additions).
