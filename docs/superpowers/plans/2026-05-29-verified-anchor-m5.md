# Verified Anchor — Milestone 5 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the verified loop a usable tool — a `cargo verified-anchor` subcommand that auto-collects every `#[derive(VerifiedAccounts)]` struct, generates Lean proof obligations, and discharges them with `lake`; plus instant `compile_error!` feedback for unsupported constraints, a worked example crate, and migration docs.

**Architecture:** Per-struct obligations reduce to a single `decide` because the soundness theorems are generic: validation via the existing `genValidate_sound` (`decide M4Subset`), lifecycle via a NEW generic `lifecycle_sound` (`decide StructLifecycleWF`). The derive macro auto-registers each struct via `inventory`; a one-line `emit_specs!()` (in the user's lib) expands to a `#[test]` that writes each struct's `lean_spec()` to a file; the subcommand runs that test, generates a `check.lean`, and runs `lake env lean`.

**Tech Stack:** Lean 4.30 / Lake (M1–M4 lib); Rust 1.93.1, `syn`/`quote`, `inventory 0.3`; the subcommand shells out to `cargo` + `lake` (std only). `trybuild` for the compile-fail test.

---

## Conventions

- **Lean:** `export PATH="$HOME/.elan/bin:$PATH"`; work in `lean/`. Test = `lake build`. Zero `sorry`/`admit`. Confirm headline theorems via `#print axioms` = `[propext, Quot.sound]`.
- **Rust:** work in `rust/`; `cargo` on PATH. Test = `cargo test -p <crate>`.
- **Lean toolchain for the subcommand/example check:** the `lake env lean` discharge needs elan on PATH (`export PATH="$HOME/.elan/bin:$PATH"`). The CLI integration test (Task E2) is gated to skip cleanly if `lake` is absent.
- Commit after each task. `.gitignore` already covers `target/`, `lean/.lake/`.
- **Key fact:** the derive macro parses `init` as separate markers (`InitMarker`+`Payer`+`Space`); `close` as `Close(dest)`. `lean_constraint` handles ONE constraint at a time, so init/close → Lean text must be ASSEMBLED per-field in `lean_spec_string` (mirroring the existing `@@BUMP@@` substitution).

---

## File structure

| File | Responsibility |
|------|----------------|
| `lean/VerifiedAnchor/Codegen/StructLifecycle.lean` | (NEW) `StructLifecycleWF` + `lifecyclePost` + `lifecycle_sound` (generic lifecycle theorem) |
| `lean/VerifiedAnchor.lean` | (MODIFY) import `Codegen.StructLifecycle` |
| `rust/verified-anchor/Cargo.toml` | (MODIFY) + `inventory = "0.3"` |
| `rust/verified-anchor/src/lib.rs` | (MODIFY) `SpecEntry` + `inventory::collect!` + `pub use inventory` + `collect_specs`/`write_spec_files` + `emit_specs!` |
| `rust/verified-anchor-macros/src/lib.rs` | (MODIFY) emit `inventory::submit!` per derive; emit `Constraint.init/.close` in `lean_spec`; `compile_error!` for unsupported constraints |
| `rust/verified-anchor/tests/lean_spec.rs` | (MODIFY) assert an init struct's `lean_spec` includes `Constraint.init` |
| `rust/verified-anchor-macros/tests/ui/` | (NEW) `trybuild` compile-fail case for an unsupported constraint |
| `rust/cargo-verified-anchor/` | (NEW) the subcommand: `main.rs`, `generate.rs`, `collect.rs`, `discharge.rs`, `tests/cli.rs` |
| `rust/verified-anchor-example/` | (NEW) worked user crate (validation + lifecycle structs + `emit_specs!()`) |
| `rust/Cargo.toml` | (MODIFY) add the two new members |
| `docs/migrating-from-anchor.md` | (NEW) supported subset, workflow, attribute mapping, trust boundary |
| `docs/verified-anchor-bridge.md` | (MODIFY) automated-check + generic-lifecycle-theorem note |

---

# PART L — Lean: generic lifecycle theorem

## Task L1: `StructLifecycle.lean` (the probed theorem)

**Files:** Create `lean/VerifiedAnchor/Codegen/StructLifecycle.lean`; modify `lean/VerifiedAnchor.lean`.

- [ ] **Step 1: Create the module** (verbatim from the validated 2026-05-29 probe, in `namespace VerifiedAnchor`)

Create `lean/VerifiedAnchor/Codegen/StructLifecycle.lean`:
```lean
import VerifiedAnchor.Codegen.Lifecycle

namespace VerifiedAnchor

/-- The fixed 8-byte discriminator the codegen writes on init. -/
def initDisc : ByteArray := ByteArray.mk (Array.replicate 8 0)

/-- Post-condition obligation for one constraint at field index `idx` of struct `s`. -/
def lifecyclePost (s : AccountsStruct) (idx : Nat) : Constraint → Prop
  | .init payerName space owner =>
      ∀ payerIdx, List.findIdx? (·.name == payerName) s.fields = some payerIdx →
        ∀ rent c c', applyInit idx payerIdx space owner initDisc rent c = some c' →
          ∃ a, c'.accounts[idx]? = some a ∧ a.owner = owner ∧ space + 8 ≤ a.data.size
  | .close destName =>
      ∀ destIdx, List.findIdx? (·.name == destName) s.fields = some destIdx →
        ∀ c c', applyClose idx destIdx c = some c' →
          ∃ a, c'.accounts[idx]? = some a ∧ a.lamports = 0 ∧ hasDiscriminator a closedAccountDiscriminator
  | _ => True

/-- Decidable well-formedness: each init/close clause resolves payer/dest to a DIFFERENT
    index than the field itself (so the applyInit/applyClose idx≠… guard holds). -/
def lifecycleClauseWF (s : AccountsStruct) (idx : Nat) : Constraint → Bool
  | .init payerName _ _ =>
      match List.findIdx? (·.name == payerName) s.fields with
      | some pi => decide (idx ≠ pi)
      | none => true
  | .close destName =>
      match List.findIdx? (·.name == destName) s.fields with
      | some di => decide (idx ≠ di)
      | none => true
  | _ => true

def StructLifecycleWF (s : AccountsStruct) : Prop :=
  ∀ p ∈ s.fields.zipIdx, ∀ k ∈ p.1.constraints, lifecycleClauseWF s p.2 k = true

instance (s : AccountsStruct) : Decidable (StructLifecycleWF s) := by
  unfold StructLifecycleWF; infer_instance

/-- THE GENERIC LIFECYCLE THEOREM: one decidable well-formedness predicate implies the M1
    init/close post-conditions for every lifecycle field. Per-struct checking is then just
    `decide (StructLifecycleWF spec)`. -/
theorem lifecycle_sound (s : AccountsStruct) (h : StructLifecycleWF s) :
    ∀ p ∈ s.fields.zipIdx, ∀ k ∈ p.1.constraints, lifecyclePost s p.2 k := by
  intro p hp k hk
  have hwf := h p hp k hk
  cases k with
  | init payerName space owner =>
    intro payerIdx hpayer rent c c' heff
    simp only [lifecycleClauseWF, hpayer] at hwf
    have hne : p.2 ≠ payerIdx := of_decide_eq_true hwf
    exact init_establishes_post p.2 payerIdx space owner initDisc rent c c' hne (by decide) heff
  | close destName =>
    intro destIdx hdest c c' heff
    simp only [lifecycleClauseWF, hdest] at hwf
    have hne : p.2 ≠ destIdx := of_decide_eq_true hwf
    exact close_establishes_post p.2 destIdx c c' hne heff
  | signer => trivial
  | «mut» => trivial
  | owner e => trivial
  | hasOne f => trivial
  | discriminator d => trivial
  | seeds ss b => trivial

end VerifiedAnchor
```
NOTE: this exact file compiled clean with axioms `[propext, Quot.sound]` during brainstorming (probe). If any tactic name drifts, fix without `sorry`; the proof is mechanical (the hard content is in `init/close_establishes_post`).

- [ ] **Step 2: Import in the root**

In `lean/VerifiedAnchor.lean`, add after the `Codegen.Lifecycle` import line:
```lean
import VerifiedAnchor.Codegen.StructLifecycle
```

- [ ] **Step 3: Add a concrete non-vacuous `#guard`** (proves the predicate bites)

Append to `StructLifecycle.lean` before `end VerifiedAnchor`:
```lean
/-- Sanity: a struct whose `init` payer resolves to a different field is well-formed; one
    whose payer resolves to itself is not. (Crypto-free, so `decide` reduces.) -/
private def egInitGood : AccountsStruct :=
  { programId := Pubkey.zero
  , fields := [ { name := "new", ty := AccountType.uncheckedAccount,
                  constraints := [Constraint.init "payer" 0 Pubkey.zero] }
              , { name := "payer", ty := AccountType.uncheckedAccount, constraints := [] } ] }
private def egInitBad : AccountsStruct :=
  { programId := Pubkey.zero
  , fields := [ { name := "new", ty := AccountType.uncheckedAccount,
                  constraints := [Constraint.init "new" 0 Pubkey.zero] } ] }  -- payer = self
#guard decide (StructLifecycleWF egInitGood) = true
#guard decide (StructLifecycleWF egInitBad) = false
```

- [ ] **Step 4: Build + axioms**

Run:
```bash
export PATH="$HOME/.elan/bin:$PATH"; cd /home/parth/Desktop/PARTH/Verification/lean
lake build 2>&1 | tail -3
grep -rn "sorry\|admit" VerifiedAnchor/ || echo "clean"
printf 'import VerifiedAnchor.Codegen.StructLifecycle\n#print axioms VerifiedAnchor.lifecycle_sound\n' > VerifiedAnchor/AxTmp.lean
lake env lean VerifiedAnchor/AxTmp.lean; rm -f VerifiedAnchor/AxTmp.lean
```
Expected: build green (20 jobs); `clean`; `lifecycle_sound` axioms `[propext, Quot.sound]`. The two `#guard`s pass (true/false).

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/VerifiedAnchor/Codegen/StructLifecycle.lean lean/VerifiedAnchor.lean
git commit -m "feat(lean): generic lifecycle theorem (StructLifecycleWF + lifecycle_sound)"
```

---

# PART M — Rust lib + macro

## Task M1: `verified-anchor` lib — SpecEntry, inventory, `emit_specs!`

**Files:** Modify `rust/verified-anchor/Cargo.toml`, `rust/verified-anchor/src/lib.rs`.

- [ ] **Step 1: Add the inventory dependency**

In `rust/verified-anchor/Cargo.toml`, under `[dependencies]`, add:
```toml
inventory = "0.3"
```

- [ ] **Step 2: Add the collection API + macro to `src/lib.rs`**

Append to `rust/verified-anchor/src/lib.rs`:
```rust
/// Re-exported so the derive macro can emit `::verified_anchor::inventory::submit!`.
pub use inventory;

/// One registered `#[derive(VerifiedAccounts)]` struct.
pub struct SpecEntry {
    pub name: &'static str,
    /// The Milestone-1 `AccountsStruct` literal (Lean source).
    pub lean_spec: fn() -> String,
    /// True if any field carries an `init`/`close` constraint (selects the obligation kind).
    pub has_lifecycle: bool,
}

inventory::collect!(SpecEntry);

/// All registered structs in the current compilation artifact.
pub fn collect_specs() -> Vec<&'static SpecEntry> {
    inventory::iter::<SpecEntry>.into_iter().collect()
}

/// Write one spec file per registered struct into `dir`. Filename is `<name>.<kind>` where
/// kind is `lifecycle` or `validation`; the file content is the `lean_spec()` literal.
/// (No JSON — the literal is the whole content, so there's nothing to escape.)
pub fn write_spec_files(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for e in collect_specs() {
        let kind = if e.has_lifecycle { "lifecycle" } else { "validation" };
        std::fs::write(dir.join(format!("{}.{}", e.name, kind)), (e.lean_spec)())?;
    }
    Ok(())
}

/// Drop ONE call in your crate's lib (e.g. bottom of `src/lib.rs`). Expands to a test that,
/// when `VERIFIED_ANCHOR_SPEC_DIR` is set (by `cargo verified-anchor check`), writes spec
/// files for every derived struct in this crate. Placing it in the lib is REQUIRED: the
/// emitter must be same-crate as the `inventory::submit!`s (cross-crate harnesses dead-strip).
#[macro_export]
macro_rules! emit_specs {
    () => {
        #[cfg(test)]
        #[test]
        fn __verified_anchor_emit_specs() {
            if let Ok(dir) = ::std::env::var("VERIFIED_ANCHOR_SPEC_DIR") {
                ::verified_anchor::write_spec_files(::std::path::Path::new(&dir)).unwrap();
            }
        }
    };
}
```

- [ ] **Step 3: Write a same-crate test for `write_spec_files`**

Append to `rust/verified-anchor/src/lib.rs`:
```rust
#[cfg(test)]
mod spec_collection_tests {
    use super::*;

    // A manually-registered entry (same crate → inventory sees it).
    inventory::submit! { SpecEntry { name: "FakeStruct", lean_spec: || "FAKE-SPEC".to_string(), has_lifecycle: false } }

    #[test]
    fn write_spec_files_emits_one_file_per_entry() {
        let dir = std::env::temp_dir().join("va-m1-spec-test");
        let _ = std::fs::remove_dir_all(&dir);
        write_spec_files(&dir).unwrap();
        let f = dir.join("FakeStruct.validation");
        assert!(f.exists(), "expected {f:?}");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "FAKE-SPEC");
    }
}
```

- [ ] **Step 4: Build + test**

Run:
```bash
cd /home/parth/Desktop/PARTH/Verification/rust
cargo test -p verified-anchor --lib 2>&1 | tail -15
```
Expected: compiles; `write_spec_files_emits_one_file_per_entry` passes.

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor/Cargo.toml rust/verified-anchor/src/lib.rs
git commit -m "feat(verified-anchor): SpecEntry + inventory collection + emit_specs! macro"
```

---

## Task M2: macro — register structs + emit `Constraint.init/.close` in `lean_spec`

**Files:** Modify `rust/verified-anchor-macros/src/lib.rs`, `rust/verified-anchor/tests/lean_spec.rs`.

- [ ] **Step 1: Write the failing `lean_spec` test for init/close**

In `rust/verified-anchor/tests/lean_spec.rs`, append:
```rust
#[derive(VerifiedAccounts)]
struct InitClose {
    #[account(init, payer = payer, space = 0)]
    new: u8,
    #[account(mut)]
    payer: u8,
    #[account(close = payer)]
    old: u8,
}

#[test]
fn lean_spec_emits_lifecycle_constraints() {
    let s = InitClose::lean_spec();
    assert!(s.contains("Constraint.init \"payer\" 0 Pubkey.zero"), "init missing: {s}");
    assert!(s.contains("Constraint.close \"payer\""), "close missing: {s}");
}
```
Run `cd rust && cargo test -p verified-anchor --test lean_spec 2>&1 | tail` — expect FAIL (current macro emits nothing for init/close).

- [ ] **Step 2: Emit init/close in `lean_spec_string`**

In `rust/verified-anchor-macros/src/lib.rs`, inside `lean_spec_string`'s per-field loop, after `let cs: Vec<String> = …` and before building `cs_joined`, assemble lifecycle constraints from the markers and append them to `cs`:
```rust
        let mut cs = cs;   // make mutable
        // init: assemble InitMarker + Payer + Space -> Constraint.init "<payer>" <space> Pubkey.zero
        if spec.constraints.iter().any(|c| matches!(c, Constraint::InitMarker)) {
            let payer = spec.constraints.iter().find_map(|c|
                if let Constraint::Payer(p) = c { Some(p.to_string()) } else { None });
            let space = spec.constraints.iter().find_map(|c|
                if let Constraint::Space(n) = c { Some(*n) } else { None });
            if let (Some(payer), Some(space)) = (payer, space) {
                cs.push(format!("Constraint.init \"{}\" {} Pubkey.zero", payer, space));
            }
        }
        // close: Close(dest) -> Constraint.close "<dest>"
        if let Some(dest) = spec.constraints.iter().find_map(|c|
            if let Constraint::Close(d) = c { Some(d.to_string()) } else { None }) {
            cs.push(format!("Constraint.close \"{}\"", dest));
        }
```
NOTE: `lean_constraint` still returns `""` for the raw markers (they're filtered out); the assembled `Constraint.init/.close` strings are appended here. The owner is the `Pubkey.zero` placeholder (matches the Lean lifecycle model; the real program-id correspondence is the documented transcription gap). Leave the `cs_joined`/`@@BUMP@@` logic below unchanged (it now joins the appended init/close too).

- [ ] **Step 3: Run the lean_spec test**

Run `cd rust && cargo test -p verified-anchor --test lean_spec 2>&1 | tail` — expect PASS (both `lean_spec_matches`, `lean_spec_seeds`, and the new `lean_spec_emits_lifecycle_constraints`).

- [ ] **Step 4: Emit the `inventory::submit!` per derived struct**

In `rust/verified-anchor-macros/src/lib.rs`, in `derive_verified_accounts`, compute `has_lifecycle`, capture the struct name as a string, and add a `submit!` to the `expanded` quote. The `lean_spec: #name::lean_spec` field is a function pointer to the inherent `fn() -> String` method (matching `SpecEntry.lean_spec`'s type). Replace the existing `let expanded = quote! { … };` with:
```rust
    let has_lifecycle = specs.iter().any(|s| s.constraints.iter().any(|c|
        matches!(c, Constraint::InitMarker | Constraint::Close(_))));
    let name_str = name.to_string();
    let expanded = quote! {
        impl ::verified_anchor::Validate for #name {
            #body
        }
        impl #name {
            /// The Milestone-1 `AccountsStruct` literal for this struct (Lean source).
            pub fn lean_spec() -> ::std::string::String {
                #lean.to_string()
            }

            #lifecycle
        }
        ::verified_anchor::inventory::submit! {
            ::verified_anchor::SpecEntry {
                name: #name_str,
                lean_spec: #name::lean_spec,
                has_lifecycle: #has_lifecycle,
            }
        }
    };
```

- [ ] **Step 5: Build the workspace, confirm no regressions**

Run:
```bash
cd /home/parth/Desktop/PARTH/Verification/rust
cargo build -p verified-anchor-macros 2>&1 | tail -5
cargo test -p verified-anchor --test behavior --test lean_spec 2>&1 | grep "test result"
```
Expected: macro compiles; behavior + lean_spec green (the `submit!` references `::verified_anchor::SpecEntry`/`inventory`, available from M1).

- [ ] **Step 6: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-macros/src/lib.rs rust/verified-anchor/tests/lean_spec.rs
git commit -m "feat(macros): register structs via inventory; emit Constraint.init/.close in lean_spec"
```

---

## Task M3: macro — `compile_error!` for unsupported constraints

**Files:** Modify `rust/verified-anchor-macros/src/lib.rs`; create `rust/verified-anchor-macros/tests/ui/unsupported_constraint.rs` + `.stderr`; modify `rust/verified-anchor-macros/Cargo.toml`.

- [ ] **Step 1: Improve the parser's error for known-but-unsupported Anchor constraints**

In `rust/verified-anchor-macros/src/lib.rs`, in `impl Parse for Constraint`, replace the final `other => Err(...)` arm with one that names common stock-Anchor constraints explicitly:
```rust
            other => {
                let known_unsupported = [
                    "realloc", "zero", "rent_exempt", "constraint", "token", "mint",
                    "associated_token", "executable", "address", "owner_program",
                    "token_program", "seeds_program",
                ];
                let hint = if known_unsupported.contains(&other) {
                    format!("`{other}` is a stock-Anchor constraint that verified-anchor does not support")
                } else {
                    format!("unknown constraint `{other}`")
                };
                Err(syn::Error::new(
                    ident.span(),
                    format!("{hint}; verified-anchor supports: signer, mut, owner, has_one, init, payer, space, close, seeds, bump. See docs/migrating-from-anchor.md"),
                ))
            }
```

- [ ] **Step 2: Add `trybuild` as a dev-dependency**

In `rust/verified-anchor-macros/Cargo.toml`, add:
```toml
[dev-dependencies]
trybuild = "1.0"
```

- [ ] **Step 3: Create the compile-fail fixture**

Create `rust/verified-anchor-macros/tests/ui/unsupported_constraint.rs`:
```rust
use verified_anchor::VerifiedAccounts;

#[derive(VerifiedAccounts)]
struct Bad {
    #[account(realloc = 8)]
    vault: u8,
}

fn main() {}
```
NOTE: this requires `verified-anchor` available to the ui test crate. Add to `rust/verified-anchor-macros/Cargo.toml` `[dev-dependencies]`: `verified-anchor = { path = "../verified-anchor" }`.

- [ ] **Step 4: Create the trybuild runner test**

Create `rust/verified-anchor-macros/tests/compile_fail.rs`:
```rust
#[test]
fn unsupported_constraints_are_rejected() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
```

- [ ] **Step 5: Generate the expected stderr, then run**

Run (first pass writes the `.stderr` snapshot for THIS compiler; second pass verifies):
```bash
cd /home/parth/Desktop/PARTH/Verification/rust
TRYBUILD=overwrite cargo test -p verified-anchor-macros --test compile_fail 2>&1 | tail -5
cargo test -p verified-anchor-macros --test compile_fail 2>&1 | tail -5
```
Expected: after overwrite, `tests/ui/unsupported_constraint.stderr` exists and contains the "verified-anchor supports: …" message; the second run PASSES. Open the `.stderr` and confirm it shows the helpful message (not a generic parse error).

- [ ] **Step 6: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-macros/src/lib.rs rust/verified-anchor-macros/Cargo.toml rust/verified-anchor-macros/tests/
git commit -m "feat(macros): clear compile_error for unsupported constraints + trybuild test"
```

---

# PART C — the `cargo verified-anchor` subcommand

## Task C1: subcommand crate skeleton + arg parsing

**Files:** Create `rust/cargo-verified-anchor/Cargo.toml`, `rust/cargo-verified-anchor/src/main.rs`; modify `rust/Cargo.toml`.

- [ ] **Step 1: Add the crate to the workspace**

In `rust/Cargo.toml`, add ONLY `cargo-verified-anchor` to `members` (it is created in this task; `verified-anchor-example` is added later by Task E1 when its directory exists — do NOT add a member whose directory is missing, or every workspace cargo command breaks):
```toml
members = ["verified-anchor-macros", "verified-anchor", "verified-anchor-program", "cargo-verified-anchor"]
```

- [ ] **Step 2: Crate manifest**

Create `rust/cargo-verified-anchor/Cargo.toml`:
```toml
[package]
name = "cargo-verified-anchor"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "cargo-verified-anchor"
path = "src/main.rs"
```
(No dependencies — std only.)

- [ ] **Step 3: `main.rs` with arg parsing + module wiring**

Create `rust/cargo-verified-anchor/src/main.rs`:
```rust
mod collect;
mod generate;
mod discharge;

use std::path::PathBuf;
use std::process::exit;

struct Args {
    crate_name: Option<String>,
    lean_dir: Option<PathBuf>,
    json: bool,
}

fn parse_args() -> Result<Args, String> {
    // Invoked as `cargo verified-anchor check ...` => argv: [bin, "verified-anchor", "check", ...]
    let mut it = std::env::args().skip(1).peekable();
    if it.peek().map(|s| s == "verified-anchor").unwrap_or(false) {
        it.next();
    }
    match it.next().as_deref() {
        Some("check") => {}
        other => return Err(format!("expected subcommand `check`, got {other:?}")),
    }
    let mut args = Args { crate_name: None, lean_dir: None, json: false };
    while let Some(a) = it.next() {
        match a.as_str() {
            "-p" | "--package" => args.crate_name = it.next(),
            "--lean-dir" => args.lean_dir = it.next().map(PathBuf::from),
            "--json" => args.json = true,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(args)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => { eprintln!("cargo-verified-anchor: {e}"); exit(2); }
    };
    match run(args) {
        Ok(report) => { print!("{report}"); }
        Err(e) => { eprintln!("cargo-verified-anchor: {e}"); exit(1); }
    }
}

fn run(args: Args) -> Result<String, String> {
    // Pipeline lives in run_pipeline (Task C3); stub for now so the crate builds.
    let _ = args;
    Ok(String::new())
}
```

- [ ] **Step 4: Stub the three modules so it compiles**

Create `rust/cargo-verified-anchor/src/generate.rs`, `src/collect.rs`, `src/discharge.rs` each with a placeholder so `mod` resolves:
```rust
// generate.rs
#![allow(dead_code)]
```
(Repeat the single `#![allow(dead_code)]` line in `collect.rs` and `discharge.rs`. Real contents land in C2/C3.)

- [ ] **Step 5: Build**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo build -p cargo-verified-anchor 2>&1 | tail -5` — expect success.

- [ ] **Step 6: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/Cargo.toml rust/cargo-verified-anchor/
git commit -m "feat(cli): cargo-verified-anchor crate skeleton + arg parsing"
```

---

## Task C2: `generate.rs` — specs → `check.lean`

**Files:** Modify `rust/cargo-verified-anchor/src/generate.rs`.

- [ ] **Step 1: Write the generator + its unit tests**

Replace `rust/cargo-verified-anchor/src/generate.rs` with:
```rust
//! Turn collected specs into a Lean `check.lean` of per-struct `decide` obligations.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind { Validation, Lifecycle }

pub struct Spec {
    pub name: String,
    pub kind: Kind,
    pub lean_spec: String,
}

/// One obligation per struct: validation -> `M4Subset`, lifecycle -> `StructLifecycleWF`.
pub fn generate_check_lean(specs: &[Spec]) -> String {
    let mut out = String::from("import VerifiedAnchor\nopen VerifiedAnchor\n\ndef ownerPlaceholder : Pubkey := Pubkey.zero\n");
    let mut specs: Vec<&Spec> = specs.iter().collect();
    specs.sort_by(|a, b| a.name.cmp(&b.name));   // deterministic output
    for s in specs {
        let pred = match s.kind { Kind::Validation => "M4Subset", Kind::Lifecycle => "StructLifecycleWF" };
        let kind_str = match s.kind { Kind::Validation => "validation", Kind::Lifecycle => "lifecycle" };
        out.push_str(&format!("\n-- {} ({})\nexample : {} ({}) := by decide\n", s.name, kind_str, pred, s.lean_spec));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_struct_emits_m4subset() {
        let specs = vec![Spec { name: "T".into(), kind: Kind::Validation, lean_spec: "SPEC_T".into() }];
        let out = generate_check_lean(&specs);
        assert!(out.contains("import VerifiedAnchor"));
        assert!(out.contains("def ownerPlaceholder : Pubkey := Pubkey.zero"));
        assert!(out.contains("-- T (validation)\nexample : M4Subset (SPEC_T) := by decide"));
    }

    #[test]
    fn lifecycle_struct_emits_structlifecyclewf() {
        let specs = vec![Spec { name: "V".into(), kind: Kind::Lifecycle, lean_spec: "SPEC_V".into() }];
        let out = generate_check_lean(&specs);
        assert!(out.contains("-- V (lifecycle)\nexample : StructLifecycleWF (SPEC_V) := by decide"));
    }

    #[test]
    fn output_is_sorted_by_name() {
        let specs = vec![
            Spec { name: "B".into(), kind: Kind::Validation, lean_spec: "SB".into() },
            Spec { name: "A".into(), kind: Kind::Validation, lean_spec: "SA".into() },
        ];
        let out = generate_check_lean(&specs);
        assert!(out.find("-- A (").unwrap() < out.find("-- B (").unwrap());
    }
}
```

- [ ] **Step 2: Run the generator unit tests**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p cargo-verified-anchor generate 2>&1 | tail -10` — expect all three pass.

- [ ] **Step 3: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/cargo-verified-anchor/src/generate.rs
git commit -m "feat(cli): generate check.lean obligations from specs (+ unit tests)"
```

---

## Task C3: `collect.rs` + `discharge.rs` + wire the pipeline

**Files:** Modify `rust/cargo-verified-anchor/src/collect.rs`, `src/discharge.rs`, `src/main.rs`.

- [ ] **Step 1: `collect.rs` — run the emitter test, read spec files**

Replace `rust/cargo-verified-anchor/src/collect.rs` with:
```rust
//! Run the target crate's `emit_specs!()` test and read the resulting spec files.
use crate::generate::{Kind, Spec};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run `cargo test --lib __verified_anchor_emit_specs` with VERIFIED_ANCHOR_SPEC_DIR set,
/// then read every `<name>.{validation,lifecycle}` file written into `spec_dir`.
pub fn collect(crate_name: Option<&str>, spec_dir: &Path) -> Result<Vec<Spec>, String> {
    let _ = std::fs::remove_dir_all(spec_dir);
    std::fs::create_dir_all(spec_dir).map_err(|e| format!("mkdir {spec_dir:?}: {e}"))?;

    let mut cmd = Command::new("cargo");
    cmd.arg("test");
    if let Some(c) = crate_name { cmd.args(["-p", c]); }
    cmd.args(["--lib", "__verified_anchor_emit_specs"]);
    cmd.env("VERIFIED_ANCHOR_SPEC_DIR", spec_dir);
    let out = cmd.output().map_err(|e| format!("running cargo test: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo test (spec emitter) failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }

    let mut specs = Vec::new();
    for entry in std::fs::read_dir(spec_dir).map_err(|e| format!("read {spec_dir:?}: {e}"))? {
        let path: PathBuf = entry.map_err(|e| e.to_string())?.path();
        let kind = match path.extension().and_then(|s| s.to_str()) {
            Some("validation") => Kind::Validation,
            Some("lifecycle") => Kind::Lifecycle,
            _ => continue,
        };
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string();
        let lean_spec = std::fs::read_to_string(&path).map_err(|e| format!("read {path:?}: {e}"))?;
        specs.push(Spec { name, kind, lean_spec });
    }
    Ok(specs)
}
```

- [ ] **Step 2: `discharge.rs` — locate lean, run lake**

Replace `rust/cargo-verified-anchor/src/discharge.rs` with:
```rust
//! Build the Lean library and check the generated obligations file.
use std::path::{Path, PathBuf};
use std::process::Command;

/// Find the Lean project dir: explicit `--lean-dir`, else $VERIFIED_ANCHOR_LEAN_DIR, else a
/// sibling `lean/` walking up from the current dir.
pub fn locate_lean_dir(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = explicit { return Ok(p.to_path_buf()); }
    if let Ok(p) = std::env::var("VERIFIED_ANCHOR_LEAN_DIR") { return Ok(PathBuf::from(p)); }
    let mut dir = std::env::current_dir().map_err(|e| e.to_string())?;
    loop {
        let cand = dir.join("lean");
        if cand.join("lakefile.toml").exists() { return Ok(cand); }
        if !dir.pop() { return Err("could not locate the verified-anchor Lean project (pass --lean-dir)".into()); }
    }
}

/// `lake build` (cached) then `lake env lean <check_file>`. Returns the lean stderr on failure.
pub fn discharge(lean_dir: &Path, check_file: &Path) -> Result<(), String> {
    let build = Command::new("lake").arg("build").current_dir(lean_dir).output()
        .map_err(|e| format!("running `lake build` (is elan/lake on PATH?): {e}"))?;
    if !build.status.success() {
        return Err(format!("lake build failed:\n{}", String::from_utf8_lossy(&build.stderr)));
    }
    let chk = Command::new("lake").arg("env").arg("lean").arg(check_file)
        .current_dir(lean_dir).output()
        .map_err(|e| format!("running `lake env lean`: {e}"))?;
    if !chk.status.success() {
        return Err(format!("proof obligations NOT discharged:\n{}{}",
            String::from_utf8_lossy(&chk.stdout), String::from_utf8_lossy(&chk.stderr)));
    }
    Ok(())
}
```

- [ ] **Step 3: Wire the pipeline in `main.rs`**

In `rust/cargo-verified-anchor/src/main.rs`, replace the stub `run` with:
```rust
fn run(args: Args) -> Result<String, String> {
    let spec_dir = std::env::current_dir().map_err(|e| e.to_string())?
        .join("target").join("verified-anchor").join("specs");
    let specs = collect::collect(args.crate_name.as_deref(), &spec_dir)?;
    if specs.is_empty() {
        return Err("no #[derive(VerifiedAccounts)] structs found — did you add `verified_anchor::emit_specs!();` to your lib?".into());
    }
    let check_lean = generate::generate_check_lean(&specs);
    let check_file = spec_dir.join("check.lean");
    std::fs::write(&check_file, &check_lean).map_err(|e| format!("write {check_file:?}: {e}"))?;

    let lean_dir = discharge::locate_lean_dir(args.lean_dir.as_deref())?;
    discharge::discharge(&lean_dir, &check_file)?;

    let mut report = String::new();
    if args.json {
        report.push_str("{\"ok\":true,\"structs\":[");
        for (i, s) in specs.iter().enumerate() {
            if i > 0 { report.push(','); }
            let k = match s.kind { generate::Kind::Validation => "validation", generate::Kind::Lifecycle => "lifecycle" };
            report.push_str(&format!("{{\"name\":\"{}\",\"kind\":\"{}\"}}", s.name, k));
        }
        report.push_str("]}\n");
    } else {
        for s in &specs {
            let k = match s.kind { generate::Kind::Validation => "validation", generate::Kind::Lifecycle => "lifecycle" };
            report.push_str(&format!("  \u{2713} {} ({})\n", s.name, k));
        }
        report.push_str(&format!("All {} proof obligation(s) discharged.\n", specs.len()));
    }
    Ok(report)
}
```
Add the missing `use` at the top of `main.rs` if needed (the modules are referenced as `collect::`, `generate::`, `discharge::` — already `mod`-declared, so no `use` needed).

- [ ] **Step 4: Build**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo build -p cargo-verified-anchor 2>&1 | tail -5` — expect success. (End-to-end run is exercised in E2.)

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/cargo-verified-anchor/src/
git commit -m "feat(cli): collect (run emitter) + discharge (lake) + wire check pipeline"
```

---

# PART E — example crate + integration + docs

## Task E1: `verified-anchor-example` crate

**Files:** Create `rust/verified-anchor-example/Cargo.toml`, `rust/verified-anchor-example/src/lib.rs`; modify `rust/Cargo.toml`.

- [ ] **Step 1: Manifest + add to workspace members**

Create `rust/verified-anchor-example/Cargo.toml`:
```toml
[package]
name = "verified-anchor-example"
version = "0.1.0"
edition = "2021"

[dependencies]
verified-anchor = { path = "../verified-anchor" }
solana-program = "2"
```
Then in `rust/Cargo.toml` add `"verified-anchor-example"` to `members` (its directory now exists):
```toml
members = ["verified-anchor-macros", "verified-anchor", "verified-anchor-program", "cargo-verified-anchor", "verified-anchor-example"]
```

- [ ] **Step 2: The example structs + `emit_specs!()`**

Create `rust/verified-anchor-example/src/lib.rs`:
```rust
//! A worked verified-anchor user crate. `cargo build` compiles it; `cargo verified-anchor
//! check -p verified-anchor-example` discharges every struct's proof obligation via Lean.
use verified_anchor::VerifiedAccounts;

/// Validation: a PDA account derived from a literal + an instruction-arg seed.
#[derive(VerifiedAccounts)]
pub struct CheckPda {
    #[account(seeds = [b"vault", arg(0, 4)], bump)]
    pub pda: u8,
}

/// Validation: signer + writable.
#[derive(VerifiedAccounts)]
pub struct Transfer {
    #[account(mut)]
    pub vault: u8,
    #[account(signer)]
    pub authority: u8,
}

/// Lifecycle: init a new account, and close one to a destination.
#[derive(VerifiedAccounts)]
pub struct Lifecycle {
    #[account(init, payer = payer, space = 0)]
    pub new_acct: u8,
    #[account(mut, signer)]
    pub payer: u8,
    #[account(close = payer)]
    pub old_acct: u8,
}

verified_anchor::emit_specs!();
```

- [ ] **Step 3: Build the example + run its emitter manually**

Run:
```bash
cd /home/parth/Desktop/PARTH/Verification/rust
cargo build -p verified-anchor-example 2>&1 | tail -5
VERIFIED_ANCHOR_SPEC_DIR=/tmp/va-e1-specs cargo test -p verified-anchor-example --lib __verified_anchor_emit_specs 2>&1 | tail -8
ls /tmp/va-e1-specs
```
Expected: builds; the emitter test passes; `/tmp/va-e1-specs` contains `CheckPda.validation`, `Transfer.validation`, `Lifecycle.lifecycle`. (Confirms inventory same-crate collection works end-to-end through the real macro.)

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-example/
git commit -m "feat(example): worked verified-anchor user crate (validation + lifecycle)"
```

---

## Task E2: CLI integration test (end-to-end check on the example)

**Files:** Create `rust/cargo-verified-anchor/tests/cli.rs`.

- [ ] **Step 1: Write the integration test**

Create `rust/cargo-verified-anchor/tests/cli.rs`:
```rust
//! End-to-end: run the built `cargo-verified-anchor` binary against verified-anchor-example
//! and assert every obligation is discharged. Gated on the Lean toolchain being present.
use std::path::PathBuf;
use std::process::Command;

fn lean_dir() -> PathBuf {
    // rust/cargo-verified-anchor -> repo root -> lean/
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // rust/
    p.pop(); // repo root
    p.push("lean");
    p
}

fn lake_available() -> bool {
    Command::new("lake").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

#[test]
fn check_discharges_example_obligations() {
    if !lake_available() {
        eprintln!("SKIP: lake not on PATH");
        return;
    }
    let bin = env!("CARGO_BIN_EXE_cargo-verified-anchor");
    let out = Command::new(bin)
        .args(["verified-anchor", "check", "-p", "verified-anchor-example",
               "--lean-dir", lean_dir().to_str().unwrap()])
        .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap()) // rust/
        .output()
        .expect("run cargo-verified-anchor");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "check failed:\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}");
    assert!(stdout.contains("CheckPda (validation)"), "missing CheckPda: {stdout}");
    assert!(stdout.contains("Lifecycle (lifecycle)"), "missing Lifecycle: {stdout}");
    assert!(stdout.contains("discharged"), "missing summary: {stdout}");
}
```
NOTE: this test shells out to `cargo test` (the emitter) and `lake` — it needs the Lean toolchain and may be slow on first lake build. It skips cleanly when `lake` is absent (CI without Lean still green). The nested `cargo test` is run after the outer test harness has finished building, so the build lock is free.

- [ ] **Step 2: Run it (with the Lean toolchain on PATH)**

Run:
```bash
export PATH="$HOME/.elan/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust
cargo test -p cargo-verified-anchor --test cli 2>&1 | tail -20
```
Expected: PASS — the report lists `CheckPda (validation)`, `Transfer (validation)`, `Lifecycle (lifecycle)`, and "All 3 proof obligation(s) discharged."

- [ ] **Step 3: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/cargo-verified-anchor/tests/cli.rs
git commit -m "test(cli): end-to-end cargo verified-anchor check on the example crate"
```

---

## Task F1: migration docs + bridge addendum + final gates

**Files:** Create `docs/migrating-from-anchor.md`; modify `docs/verified-anchor-bridge.md`.

- [ ] **Step 1: Write the migration guide**

Create `docs/migrating-from-anchor.md`:
```markdown
# Migrating a stock-Anchor program to verified-anchor

verified-anchor verifies a **subset** of Anchor's `#[derive(Accounts)]` account validation.
Programs in the subset get a machine-checked guarantee that the generated validation/lifecycle
code implements the formal contract.

## Workflow

1. Replace `#[derive(Accounts)]` with `#[derive(verified_anchor::VerifiedAccounts)]` and add
   `verified_anchor::emit_specs!();` once in your crate's `src/lib.rs`.
2. `cargo build` — unsupported constraints fail here with a clear message.
3. `cargo verified-anchor check` — discharges the proof obligations via Lean (`lake`). Run it
   locally before committing and as a CI gate.

## Supported constraints

| Anchor attribute | verified-anchor | Guarantee |
|---|---|---|
| `signer` | yes | validation (`genValidate_sound`) |
| `mut` | yes | validation |
| `owner = X` | yes | validation |
| `has_one = f` | yes | validation |
| `seeds = [..], bump` | yes (canonical-only) | validation |
| `init, payer = p, space = n` | yes | lifecycle (`lifecycle_sound`) |
| `close = d` | yes | lifecycle |
| `realloc`, `zero`, `constraint = expr`, `token::*`, `mint::*`, `associated_token::*`, `address`, `executable`, `rent_exempt` | **no** | rejected at compile time |

## Boundaries (be honest with yourself)

- `seeds`/`bump` is **canonical-only**: a declared `bump = n` must equal the canonical bump
  (stricter than Anchor's stored-bump form).
- The Rust↔Lean correspondence is **transcription** (mechanically regenerated, runtime-tested),
  not a cross-language proof. See `docs/verified-anchor-bridge.md`.
- `init`/`close` model the documented effect, not the CPI dispatch or rustc/sBPF codegen.
```

- [ ] **Step 2: Bridge-doc addendum**

In `docs/verified-anchor-bridge.md`, append a section:
```markdown
## Automated checking (M5)

The Rust→Lean flow is now mechanical: `#[derive(VerifiedAccounts)]` auto-registers each struct
(`inventory`); `verified_anchor::emit_specs!()` writes each struct's `lean_spec()`; and
`cargo verified-anchor check` generates a `check.lean` of per-struct obligations and runs
`lake env lean`. Each obligation is a single `decide`:
- validation structs → `M4Subset spec` (the generic `genValidate_sound` applies);
- lifecycle structs → `StructLifecycleWF spec` (the generic `lifecycle_sound` applies).

This automates *generation + checking* of obligations that were always the spec; it does not
widen the proven surface. The hand-copying of `lean_spec` into Lean is gone; the correspondence
remains transcription (now regenerated each run). No new modeling axioms.
```

- [ ] **Step 3: Run ALL gates**

Run:
```bash
export PATH="$HOME/.elan/bin:$PATH"; cd /home/parth/Desktop/PARTH/Verification/lean && lake build 2>&1 | tail -2
grep -rn "sorry\|admit" VerifiedAnchor/ || echo "PASS lean zero-sorry"
cd /home/parth/Desktop/PARTH/Verification/rust
cargo test -p verified-anchor --test behavior --test lean_spec 2>&1 | grep "test result"
cargo test -p verified-anchor-macros --test compile_fail 2>&1 | grep "test result"
cargo test -p cargo-verified-anchor 2>&1 | grep -E "test result|SKIP"
cargo build -p verified-anchor-example 2>&1 | tail -2
```
Expected: lake green + zero sorry; behavior+lean_spec pass; compile_fail passes; cargo-verified-anchor generate unit tests + cli (PASS or SKIP) ; example builds.

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add docs/migrating-from-anchor.md docs/verified-anchor-bridge.md
git commit -m "docs(m5): migration guide + bridge addendum (automated checking)"
```

---

## Done-bar verification (after F1)

1. `lake build` green incl. `StructLifecycle.lean`; `lifecycle_sound` axioms `[propext, Quot.sound]`; M1–M4 green. ✅ (L1, F1)
2. derive auto-registers via inventory; `emit_specs!()` writes spec files when env set. ✅ (M1, M2, E1)
3. macro `compile_error!` for unsupported constraints; M2–M4 structs still compile. ✅ (M3)
4. `cargo verified-anchor check -p verified-anchor-example` exits 0, per-struct report; Lifecycle's `StructLifecycleWF` discharge is non-vacuous (lean_spec emits `Constraint.init/.close`). ✅ (M2, C3, E2)
5. example builds with `cargo build` and passes the check end-to-end. ✅ (E1, E2)
6. `docs/migrating-from-anchor.md` + bridge addendum. ✅ (F1)
7. `cargo test -p verified-anchor` (behavior + lean_spec) green — no regression. ✅ (M2, F1)
```
