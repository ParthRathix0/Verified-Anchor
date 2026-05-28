# Verified Anchor — Milestone 3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `has_one` (relational validation) and `init`/`close` (lifecycle effects) to Verified Anchor — `has_one` extends the M2 `genValidate` proof framework; `init`/`close` get a new Lean Hoare framework (`applyInit`/`applyClose` proven to establish the M1 post-conditions) plus real effectful Rust codegen tested under litesvm.

**Architecture:** Two mechanisms. **Part A (validation):** generalize the Lean `genConstraint` to relational form, add `has_one`/`discriminator` checks + soundness, extend `genValidate_sound` to `M3Subset` (admitting typed `.account`), and generate the `has_one` Rust check. **Part B (lifecycle):** a Lean `Lifecycle` module with `applyInit`/`applyClose` state transformers + Hoare theorems, effectful `init`/`close` Rust codegen, a tiny BPF program crate, and litesvm runtime tests.

**Tech Stack:** Lean 4.30 / Lake (M1/M2 lib); Rust 1.93.1 / `solana-program 2.3.0`, `syn`/`quote`; SBF toolchain (`solana-cli 4.0.0`); `litesvm 0.6` + vendored OpenSSL for runtime tests.

---

## Conventions

- **Lean:** prefix lake with `export PATH="$HOME/.elan/bin:$PATH"`; work in `lean/`. Test = `lake build` (a failing `#guard`/proof is a build error). Zero `sorry`/`admit`; prove theorems for real (escalate, don't stub). After proving a headline theorem, confirm `#print axioms <thm>` shows no `sorryAx`.
- **Rust (native):** work in `rust/`; `cargo` on PATH; test = `cargo test`.
- **SBF build (Part B only) — the verified recipe:**
  ```bash
  export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
  cd <program-crate-dir> && cargo-build-sbf --no-rustup-override
  # produces target/deploy/<crate>.so
  ```
  The platform-tools rustc (has the `sbf-solana-solana` target) MUST be first on PATH; `--no-rustup-override` avoids the rustup toolchain-name bug.
- **litesvm dev-deps (Part B):** the `verified-anchor` crate's `[dev-dependencies]` need `litesvm = "0.6"`, `solana-sdk = "2"`, and `openssl = { version = "0.10", features = ["vendored"] }` (system `libssl-dev` is absent → OpenSSL must be vendored).
- Commit after each task. `.gitignore` covers `target/`, `lean/.lake/`.

---

## File structure

| File | Responsibility |
|------|----------------|
| `lean/VerifiedAnchor/Codegen/Generated.lean` | (MODIFY) relational `genConstraint`; `genHasOne`, `genDiscriminator`; updated `genFieldValidate`/`genValidate` |
| `lean/VerifiedAnchor/Codegen/Soundness.lean` | (MODIFY) re-prove per-constraint lemmas at new sig; `genHasOne_iff`/`genDiscriminator_iff`; `M3Subset`; `genValidate_sound` at M3Subset |
| `lean/VerifiedAnchor/Codegen/Lifecycle.lean` | (NEW) `applyInit`/`applyClose`; `init_establishes_post`/`close_establishes_post` |
| `lean/VerifiedAnchor/Codegen/ExampleGenerated.lean` | (MODIFY) has_one closed-loop + a lifecycle example |
| `lean/VerifiedAnchor.lean` | (MODIFY) import `Codegen.Lifecycle` |
| `rust/verified-anchor/src/lib.rs` | (MODIFY) `VAError::WrongHasOne`; lifecycle runtime helpers if needed |
| `rust/verified-anchor-macros/src/lib.rs` | (MODIFY) parse + codegen `has_one`, `init`, `close`; extend `lean_spec` |
| `rust/verified-anchor/tests/behavior.rs` | (MODIFY) has_one native unit tests |
| `rust/verified-anchor/tests/runtime_lifecycle.rs` | (NEW) litesvm: init/close/has_one against a deployed program |
| `rust/verified-anchor-program/` | (NEW) BPF program crate exercising generated code |
| `docs/verified-anchor-bridge.md` | (MODIFY) has_one + init/close rows + modeled-effect trust |

---

# PART A — `has_one` validation

## Task A1: Generalize `genConstraint` to relational signature (keep M2 green)

**Files:** Modify `lean/VerifiedAnchor/Codegen/Generated.lean`, `lean/VerifiedAnchor/Codegen/Soundness.lean`.

Currently `genConstraint (a : AccountInfo) : Constraint → Bool`. Relational constraints need the whole context. Generalize to `genConstraint (s c idx f) : Constraint → Bool`, resolving the account internally. Do NOT add has_one yet — just refactor and re-green, isolating the signature change.

- [ ] **Step 1: Add a none-safe Option helper + rewrite `genConstraint`/`genFieldValidate`/`genValidate`**

In `Generated.lean`, replace the `genConstraint`/`genFieldValidate`/`genValidate` block with:
```lean
/-- `o` holds a value satisfying the Bool predicate `p` (false if `o` is none). -/
def Option.allB {α} (o : Option α) (p : α → Bool) : Bool :=
  match o with | none => false | some a => p a

/-- Operational check of one constraint, resolving accounts from the full context.
    (mut/signer/owner/discriminator handled here; has_one added in Task A2; init/close/seeds
    are NOT validation constraints and return false.) -/
def genConstraint (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) :
    Constraint → Bool
  | .signer          => (Ctx.atField s c idx).allB (fun a => a.isSigner)
  | .mut             => (Ctx.atField s c idx).allB (fun a => a.isWritable)
  | .owner e         => (Ctx.atField s c idx).allB (fun a => decide (a.owner = e))
  | .discriminator d => (Ctx.atField s c idx).allB (fun a => decide (hasDiscriminator a d))
  | _                => false

def genFieldValidate (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) : Bool :=
  (f.ty.impliedConstraints ++ f.constraints).all (genConstraint s c idx f)

def genValidate (s : AccountsStruct) (c : Ctx) : Bool :=
  decide (c.length = s.fields.length) &&
    s.fields.zipIdx.all (fun p => genFieldValidate s c p.2 p.1)
```
NOTE: `genFieldValidate` no longer needs the `match Ctx.atField` wrapper (the per-constraint `allB` already returns false on a missing account). `(list).all (genConstraint s c idx f)` partially applies the 4-arg function to get the `Constraint → Bool` predicate.

- [ ] **Step 2: Re-prove the three per-constraint lemmas at the new signature**

In `Soundness.lean`, the lemmas now relate `genConstraint s c idx f .signer` to `satisfies`. Replace the three `genConstraint_*_iff` lemmas with:
```lean
theorem genConstraint_signer_iff (s c idx f) :
    genConstraint s c idx f Constraint.signer = true ↔ satisfies s c idx f Constraint.signer := by
  simp only [genConstraint, satisfies, Option.allB, Option.satisfiesSome]
  cases Ctx.atField s c idx <;> simp

theorem genConstraint_mut_iff (s c idx f) :
    genConstraint s c idx f Constraint.mut = true ↔ satisfies s c idx f Constraint.mut := by
  simp only [genConstraint, satisfies, Option.allB, Option.satisfiesSome]
  cases Ctx.atField s c idx <;> simp

theorem genConstraint_owner_iff (s c idx f e) :
    genConstraint s c idx f (Constraint.owner e) = true ↔ satisfies s c idx f (Constraint.owner e) := by
  simp only [genConstraint, satisfies, Option.allB, Option.satisfiesSome]
  cases Ctx.atField s c idx <;> simp [decide_eq_true_iff]
```
NOTE: after `cases Ctx.atField s c idx`, the `none` branch is `false = true ↔ ∃ …` (both false), the `some a` branch reduces to `a.isSigner = true ↔ a.isSigner = true`. If `simp` leaves a residue, add `Option.some.injEq`, `exists_eq_left'`. The `genConstraint_iff_satisfies` dispatcher and `genFieldValidate_iff` must be updated to the new lemma signatures (drop the `a`/`h` args — resolution is now internal). For `genFieldValidate_iff`, since `genFieldValidate` no longer pattern-matches the account, the proof simplifies: it's `(impl++expl).all (genConstraint …) = true ↔ ∀ k ∈ (impl++expl), satisfies …`, closed element-wise via the per-constraint lemmas + the `hmem` "all constraints are in-subset" argument (unchanged from M2).

- [ ] **Step 3: Build the whole library, confirm M2 still green**

Run:
```bash
export PATH="$HOME/.elan/bin:$PATH"; cd /home/parth/Desktop/PARTH/Verification/lean
lake build
grep -rn "sorry\|admit" VerifiedAnchor/ || echo "clean"
```
Expected: full build green (ExampleGenerated's M2 `transfer_*` still compile — `genValidate` external signature is unchanged); `clean`.

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/VerifiedAnchor/Codegen/Generated.lean lean/VerifiedAnchor/Codegen/Soundness.lean
git commit -m "refactor(codegen): generalize genConstraint to relational signature (M2 still green)"
```

---

## Task A2: `genHasOne` + `genDiscriminator` and their soundness lemmas

**Files:** Modify `lean/VerifiedAnchor/Codegen/Generated.lean`, `lean/VerifiedAnchor/Codegen/Soundness.lean`.

Recall M1 `satisfies (.hasOne field)`:
```
(Ctx.atField s c idx).satisfiesSome (fun a =>
  (f.ty.layoutOffsetOf field).satisfiesSome (fun off =>
    (readPubkey a.data off).satisfiesSome (fun val =>
      (Ctx.lookup s c field).satisfiesSome (fun target => val = target.key))))
```

- [ ] **Step 1: Add the `has_one` case to `genConstraint`**

In `Generated.lean`, add a `genHasOne` def and wire the `.hasOne` case:
```lean
/-- Relational has_one check: the Pubkey stored at the field's layout offset in this
    account's data equals the looked-up field account's key. None-safe. -/
def genHasOne (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) (field : String) : Bool :=
  (Ctx.atField s c idx).allB (fun a =>
    (f.ty.layoutOffsetOf field).allB (fun off =>
      (readPubkey a.data off).allB (fun val =>
        (Ctx.lookup s c field).allB (fun target => decide (val = target.key)))))
```
and change the `genConstraint` `| _ => false` to add (before it):
```lean
  | .hasOne field    => genHasOne s c idx f field
```

- [ ] **Step 2: Build `Generated`**

Run: `lake build VerifiedAnchor.Codegen.Generated` — expect success.

- [ ] **Step 3: Prove `genHasOne_iff` and `genDiscriminator_iff`**

In `Soundness.lean`, add:
```lean
theorem genConstraint_discriminator_iff (s c idx f d) :
    genConstraint s c idx f (Constraint.discriminator d) = true
      ↔ satisfies s c idx f (Constraint.discriminator d) := by
  simp only [genConstraint, satisfies, Option.allB, Option.satisfiesSome]
  cases Ctx.atField s c idx <;> simp [decide_eq_true_iff]

theorem genConstraint_hasOne_iff (s c idx f field) :
    genConstraint s c idx f (Constraint.hasOne field) = true
      ↔ satisfies s c idx f (Constraint.hasOne field) := by
  simp only [genConstraint, genHasOne, satisfies, Option.allB, Option.satisfiesSome]
  cases Ctx.atField s c idx <;> simp only []
  case some a =>
    cases f.ty.layoutOffsetOf field <;> simp only []
    case some off =>
      cases readPubkey a.data off <;> simp only []
      case some val =>
        cases Ctx.lookup s c field <;> simp [decide_eq_true_iff]
```
NOTE: this is nested `Option` case analysis mirroring the four `satisfiesSome` layers; each `none` makes both sides false, the innermost `some` reduces to `decide (val = target.key) = true ↔ val = target.key` (via `decide_eq_true_iff`). If the nested `cases`/`simp` is brittle, prove a general lemma `Option.allB o p = true ↔ o.satisfiesSome (fun a => p a = true)` once and rewrite all four layers with it (cleaner). Add that helper lemma if needed:
```lean
theorem Option.allB_iff {α} (o : Option α) (p : α → Bool) :
    o.allB p = true ↔ o.satisfiesSome (fun a => p a = true) := by
  cases o <;> simp [Option.allB, Option.satisfiesSome]
```
Then `genHasOne_iff`/`genDiscriminator_iff`/the three A1 lemmas all follow by rewriting with `Option.allB_iff` repeatedly + `decide_eq_true_iff`. PREFER this helper approach — it makes all five per-constraint lemmas uniform. No `sorry`.

- [ ] **Step 4: Build `Soundness`**

Run: `lake build VerifiedAnchor.Codegen.Soundness` — expect success, `clean`.

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/VerifiedAnchor/Codegen/Generated.lean lean/VerifiedAnchor/Codegen/Soundness.lean
git commit -m "feat(codegen): genHasOne/genDiscriminator + soundness lemmas"
```

---

## Task A3: `M3Subset` and `genValidate_sound` at M3

**Files:** Modify `lean/VerifiedAnchor/Codegen/Soundness.lean`.

- [ ] **Step 1: Define `M3Subset` and extend the dispatcher**

Add:
```lean
def isM3Constraint : Constraint → Bool
  | .signer | .mut | .owner _ | .hasOne _ | .discriminator _ => true
  | _ => false

/-- The M3 subset: every field's (implied ++ explicit) constraints are M3 validation
    constraints. Typed `.account` is allowed (its implied owner+discriminator are M3). -/
def M3Subset (s : AccountsStruct) : Prop :=
  ∀ f ∈ s.fields, ∀ k ∈ (f.ty.impliedConstraints ++ f.constraints), isM3Constraint k = true

instance (s : AccountsStruct) : Decidable (M3Subset s) := by unfold M3Subset; infer_instance

/-- Dispatcher: under M3, the generated check of any constraint agrees with `satisfies`. -/
theorem genConstraint_iff_satisfies_M3 (s c idx f k) (hk : isM3Constraint k = true) :
    genConstraint s c idx f k = true ↔ satisfies s c idx f k := by
  cases k with
  | signer        => exact genConstraint_signer_iff s c idx f
  | «mut»         => exact genConstraint_mut_iff s c idx f
  | owner e       => exact genConstraint_owner_iff s c idx f e
  | hasOne field  => exact genConstraint_hasOne_iff s c idx f field
  | discriminator d => exact genConstraint_discriminator_iff s c idx f d
  | _             => simp [isM3Constraint] at hk
```
NOTE: `.account`'s implied list is `[owner pid, discriminator (accountDiscriminator tn)]`, both M3 constraints, so `M3Subset` no longer needs to case-split on the account type the way M2 did — the membership predicate directly quantifies over `impliedConstraints ++ constraints`. This SIMPLIFIES `genFieldValidate_iff`.

- [ ] **Step 2: Update `genFieldValidate_iff` and prove `genValidate_sound` at M3**

Replace `genFieldValidate_iff` and `genValidate_sound` with the M3 versions:
```lean
theorem genFieldValidate_iff (s c idx f)
    (hcons : ∀ k ∈ (f.ty.impliedConstraints ++ f.constraints), isM3Constraint k = true) :
    genFieldValidate s c idx f = true ↔ fieldValidates s c idx f := by
  unfold genFieldValidate fieldValidates
  rw [List.all_eq_true]
  constructor
  · intro hall k hk; exact (genConstraint_iff_satisfies_M3 s c idx f k (hcons k hk)).mp (hall k hk)
  · intro hall k hk; exact (genConstraint_iff_satisfies_M3 s c idx f k (hcons k hk)).mpr (hall k hk)

theorem genValidate_sound (s : AccountsStruct) (c : Ctx) (h : M3Subset s) :
    genValidate s c = true ↔ validates s c := by
  unfold genValidate validates
  rw [Bool.and_eq_true, decide_eq_true_iff]
  constructor
  · rintro ⟨hwf, hall⟩
    refine ⟨hwf, ?_⟩
    rw [List.all_eq_true] at hall
    intro p hp
    have hmemf : p.1 ∈ s.fields := List.fst_mem_of_mem_zipIdx hp
    exact (genFieldValidate_iff s c p.2 p.1 (h p.1 hmemf)).mp (hall p hp)
  · rintro ⟨hwf, hall⟩
    refine ⟨hwf, ?_⟩
    rw [List.all_eq_true]
    intro p hp
    have hmemf : p.1 ∈ s.fields := List.fst_mem_of_mem_zipIdx hp
    exact (genFieldValidate_iff s c p.2 p.1 (h p.1 hmemf)).mpr (hall p hp)
```
NOTE: because the new `genConstraint`/`genFieldValidate` are none-safe (no separate account-resolution obligation), the M2 `hwf`-threading for resolution is no longer needed — the proof is shorter. Keep `hwf` only to satisfy the `WellFormed` conjunct. Delete the now-obsolete M2 `genConstraint_iff_satisfies`/old `genFieldValidate_iff` if they remain. `M2Subset` may be removed or left; if `ExampleGenerated` referenced `M2Subset`/`transfer_M2`, update them to `M3Subset` (see Task F1).

- [ ] **Step 3: Build + axioms**

Run:
```bash
lake build VerifiedAnchor.Codegen.Soundness
printf 'import VerifiedAnchor.Codegen.Soundness\n#print axioms VerifiedAnchor.genValidate_sound\n' > VerifiedAnchor/AxTmp.lean
lake env lean VerifiedAnchor/AxTmp.lean; rm -f VerifiedAnchor/AxTmp.lean
```
Expected: build green; axioms `[propext, Quot.sound]` (no `sorryAx`).

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/VerifiedAnchor/Codegen/Soundness.lean
git commit -m "feat(codegen): M3Subset + genValidate_sound covering has_one/discriminator"
```

---

## Task A4: Rust `has_one` codegen + native unit tests

**Files:** Modify `rust/verified-anchor/src/lib.rs`, `rust/verified-anchor-macros/src/lib.rs`, `rust/verified-anchor/tests/behavior.rs`.

- [ ] **Step 1: Add `WrongHasOne` to `VAError`**

In `rust/verified-anchor/src/lib.rs`, add the variant to the enum and its `Display` arm:
```rust
    WrongHasOne { field: &'static str, target: &'static str },
```
```rust
            VAError::WrongHasOne { field, target } =>
                write!(f, "account `{field}` field does not match `{target}`"),
```

- [ ] **Step 2: Write the failing has_one test**

In `rust/verified-anchor/tests/behavior.rs`, add (the vault stores a 32-byte authority pubkey at offset 8, after an 8-byte discriminator):
```rust
#[derive(VerifiedAccounts)]
struct CheckOwner {
    #[account(has_one = authority)]
    vault: u8,
    authority: u8,
}

fn acct_with_data(data: Vec<u8>) -> Acct {
    Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1, data, is_signer: false, is_writable: false }
}

#[test]
fn has_one_accepts_match() {
    let auth_key = Pubkey::new_unique();
    let mut data = vec![0u8; 8];                 // 8-byte discriminator
    data.extend_from_slice(auth_key.as_ref());   // authority field at offset 8
    let mut vault = acct_with_data(data);
    let mut authority = Acct { key: auth_key, owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer: false, is_writable: false };
    let accts = [vault.info(), authority.info()];
    assert_eq!(CheckOwner::validate(&accts), Ok(()));
}

#[test]
fn has_one_rejects_mismatch() {
    let mut data = vec![0u8; 8];
    data.extend_from_slice(Pubkey::new_unique().as_ref());   // wrong stored authority
    let mut vault = acct_with_data(data);
    let mut authority = acct(false, false);
    let accts = [vault.info(), authority.info()];
    assert_eq!(CheckOwner::validate(&accts), Err(VAError::WrongHasOne { field: "vault", target: "authority" }));
}
```
Run `cd rust && cargo test -p verified-anchor --test behavior 2>&1 | tail` — expect FAIL (`has_one` not yet generated; `validate` ignores it so `has_one_rejects_mismatch` returns `Ok` ≠ expected `Err`).

- [ ] **Step 3: Parse + generate `has_one`**

In `rust/verified-anchor-macros/src/lib.rs`, extend the `Constraint` enum + `Parse` impl + `validate_body` + `lean_spec`:
- `Constraint` enum: add `HasOne(syn::Ident)` (the target field name).
- `Parse`: after the `mut`/ident match, handle `has_one`:
  ```rust
  "has_one" => {
      input.parse::<Token![=]>()?;
      let target: syn::Ident = input.parse()?;
      Ok(Constraint::HasOne(target))
  }
  ```
  (Add it as an arm in the existing `match ident.to_string().as_str()`.)
- The macro must know each field's INDEX to resolve the target. In `collect_fields`, after building `specs`, build a name→index map. In `validate_body`, for `Constraint::HasOne(target)`:
  ```rust
  Constraint::HasOne(target) => {
      let tname = target.to_string();
      let tidx = specs.iter().position(|s| s.name == tname)
          .unwrap_or_else(|| panic!("has_one target `{tname}` is not a field"));
      let fname = name;            // current field name
      quote! {
          {
              let data = accounts[#i].try_borrow_data()
                  .map_err(|_| ::verified_anchor::VAError::WrongHasOne { field: #fname, target: #tname })?;
              if data.len() < 8 + 32 || data[8..8+32] != accounts[#tidx].key.as_ref()[..] {
                  return Err(::verified_anchor::VAError::WrongHasOne { field: #fname, target: #tname });
              }
          }
      }
  }
  ```
  NOTE: layout offset is fixed at 8 for M3 (the M1 model's `vaultLayout` convention: 8-byte discriminator then the first Pubkey field). The macro emits offset 8 for has_one. (Multi-field layouts are future work.)
- `lean_constraint`: `Constraint::HasOne(t) => format!("Constraint.hasOne \"{}\"", t)`.
- `lean_spec_string`: emit the vault field's `ty` as `AccountType.account "Vault" [("<target>", 8)] Pubkey.zero` when the field has a `has_one` (so the Lean layout resolves); other fields stay `uncheckedAccount`. (Keep it minimal: if a field has any `HasOne(t)`, emit `AccountType.account "<FieldNameCapitalized>" [("<t>", 8)] Pubkey.zero`.)

- [ ] **Step 4: Run has_one tests + full suite**

Run: `cd rust && cargo test -p verified-anchor 2>&1 | tail -15` — expect all tests pass (M2 tests + the two new has_one tests).

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor/src/lib.rs rust/verified-anchor-macros/src/lib.rs rust/verified-anchor/tests/behavior.rs
git commit -m "feat(macros): generate has_one relational check + tests"
```

**✅ PART A CHECKPOINT:** `lake build` green (has_one proven), `cargo test` green (has_one generated + tested). has_one is shippable.

---

# PART B — `init` / `close` lifecycle

## Task B1: Lean Hoare framework — `applyInit`/`applyClose` + establishes-post theorems

**Files:** Create `lean/VerifiedAnchor/Codegen/Lifecycle.lean`.

- [ ] **Step 1: Write the state transformers + helpers**

Create `lean/VerifiedAnchor/Codegen/Lifecycle.lean`:
```lean
import VerifiedAnchor.Contract.Satisfies

namespace VerifiedAnchor

/-- Update the account at index `i` (no-op if out of range). Uses `List.set` so the
    standard `List.getElem?_set_self`/`_ne` lemmas give clean read-back. -/
def Ctx.update (c : Ctx) (i : Nat) (g : AccountInfo → AccountInfo) : Ctx :=
  match c[i]? with
  | some a => c.set i (g a)
  | none => c

/-- Model of Anchor `init`: a system create_account funded by `payer`, then the discriminator
    write. Preconditions (else `none`): payer ≠ target indices, both in range, payer is
    signer+writable with ≥ `rent` lamports, target currently system-owned with empty data.
    Effect: target.owner := owner, target.data := disc ++ zeros to size (space+8),
    target.lamports += rent; payer.lamports -= rent. -/
def applyInit (idx payerIdx : Nat) (space : Nat) (owner : Pubkey) (disc : ByteArray)
    (rent : UInt64) (c : Ctx) : Option Ctx := do
  guard (idx ≠ payerIdx)
  let a ← c[idx]?
  let p ← c[payerIdx]?
  guard (p.isSigner && p.isWritable && rent ≤ p.lamports && a.data.size == 0)
  let newData := disc ++ ByteArray.mk (Array.replicate (space + 8 - disc.size) 0)
  let c := c.update idx (fun a => { a with owner := owner, data := newData,
                                    lamports := a.lamports + rent })
  let c := c.update payerIdx (fun p => { p with lamports := p.lamports - rent })
  some c

/-- Model of Anchor `close`: move all target lamports to `dest`, write the closed marker. -/
def applyClose (idx destIdx : Nat) (c : Ctx) : Option Ctx := do
  guard (idx ≠ destIdx)
  let a ← c[idx]?
  let _ ← c[destIdx]?
  let bal := a.lamports
  let c := c.update destIdx (fun d => { d with lamports := d.lamports + bal })
  let c := c.update idx (fun a => { a with lamports := 0, data := closedAccountDiscriminator })
  some c

end VerifiedAnchor
```
NOTE: `disc` is the 8-byte discriminator; `space + 8 - disc.size` = `space` when `disc.size = 8`. `Array.replicate` may be `Array.mkArray` in 4.30 — use whichever exists. `Ctx.update` via `zipIdx.map` keeps length. The `do`/`guard` in `Option` needs `guard : Bool → Option Unit` (from `Option`/`Functor` — `guard (b : Prop) [Decidable b]` exists; for a `Bool` use `if b then some () else none` or `guard (b = true)`). Adjust to compile.

- [ ] **Step 2: Build the transformers**

Run: `lake build VerifiedAnchor.Codegen.Lifecycle` — expect success.

- [ ] **Step 3: Prove the Hoare theorems**

This is the hard, headline task — real proof engineering (use opus; iterate with `lake build` + `exact?`). The post-conditions must match M1 `satisfies (.init …)` / `satisfies (.close …)`: init post = `a.owner = owner ∧ payer signer+writable ∧ space+8 ≤ a.data.size`; close post = `dest exists ∧ a.lamports = 0 ∧ hasDiscriminator a closedAccountDiscriminator`.

**Proof obligations** — add these to `Lifecycle.lean` and PROVE them (these are signatures; you write the bodies). No `sorry`/`admit` may remain; the acceptance gate (Step 4) checks `#print axioms` has no `sorryAx`.

First a read-back helper for `Ctx.update` (`List.set`-based, so `getElem?` is clean):
```lean
theorem Ctx.getElem?_update (c : Ctx) (i j : Nat) (g : AccountInfo → AccountInfo) :
    (c.update j g)[i]? = if i = j then (c[i]?).map g else c[i]?
```
Then the two Hoare theorems:
```lean
theorem init_establishes_post
    (idx payerIdx space owner disc rent c c') (hdisc : disc.size = 8)
    (h : applyInit idx payerIdx space owner disc rent c = some c') :
    ∃ a, c'[idx]? = some a ∧ a.owner = owner ∧ space + 8 ≤ a.data.size

theorem close_establishes_post
    (idx destIdx c c')
    (h : applyClose idx destIdx c = some c') :
    ∃ a, c'[idx]? = some a ∧ a.lamports = 0 ∧ hasDiscriminator a closedAccountDiscriminator
```

PROOF STRATEGY:
- `Ctx.getElem?_update`: unfold `Ctx.update`; `cases c[j]?`; the `some` branch is `List.getElem?_set` (in 4.30: `List.getElem?_set_self : i < c.length → (c.set i x)[i]? = some x` and `List.getElem?_set_ne : i ≠ j → (c.set j x)[i]? = c[i]?`); the `none` branch (j out of range) makes `update` a no-op and `c[j]? = none` ⇒ for `i=j`, `(c[i]?).map g = none.map g = none = c[i]?`. Use `exact?`/`simp [List.getElem?_set]` to find the precise names.
- The two theorems: `simp only [applyInit, bind, Option.bind, guard]` (or `Option.bind_eq_some`) `at h` to extract the guard facts and the two `Ctx.update` rewrites that produce `c'`. Compute `c'[idx]?` with two applications of `Ctx.getElem?_update` (the payer update at `payerIdx ≠ idx` is transparent; the target update at `idx` applies the map). Then:
  - init: `a.owner = owner` and `a.data = disc ++ Array.replicate (space+8-disc.size) 0` by construction; size: `ByteArray.size_append` gives `disc.size + (space+8-disc.size)`; with `hdisc : disc.size = 8` and `8 ≤ space+8`, this is `space+8`, so `space + 8 ≤ a.data.size` holds (equality). Find `ByteArray.size_append`/`Array.size_replicate` (or `Array.size_mkArray`) via `exact?`.
  - close: `a.lamports = 0` and `a.data = closedAccountDiscriminator` by construction; `hasDiscriminator a closedAccountDiscriminator` unfolds to `bytesAgreePrefix a.data closedAccountDiscriminator 8` = `∀ i<8, a.data[i]? = closedAccountDiscriminator[i]?`, which after rewriting `a.data = closedAccountDiscriminator` is `∀ i<8, x[i]? = x[i]?` — true by `fun _ _ => rfl` (`bytesAgreePrefix_refl`-style).
- If extracting facts from the `Option` `do`-block is fiddly, rewrite `applyInit`/`applyClose` without `do` (explicit nested `match`/`if`) so `h` destructs cleanly with `split at h`.

- [ ] **Step 4: Build + axioms**

Run:
```bash
lake build VerifiedAnchor.Codegen.Lifecycle
grep -n "sorry\|admit" VerifiedAnchor/Codegen/Lifecycle.lean || echo "clean"
printf 'import VerifiedAnchor.Codegen.Lifecycle\n#print axioms VerifiedAnchor.init_establishes_post\n#print axioms VerifiedAnchor.close_establishes_post\n' > VerifiedAnchor/AxTmp.lean
lake env lean VerifiedAnchor/AxTmp.lean; rm -f VerifiedAnchor/AxTmp.lean
```
Expected: green; `clean`; axioms contain no `sorryAx`.

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/VerifiedAnchor/Codegen/Lifecycle.lean
git commit -m "feat(codegen): Hoare framework - applyInit/applyClose establish M1 post-conditions"
```

---

## Task B2: Rust `init`/`close` effectful codegen

**Files:** Modify `rust/verified-anchor-macros/src/lib.rs`, `rust/verified-anchor/src/lib.rs`.

The derive generates an additional method `execute_lifecycle(accounts, program_id, rent_lamports)` performing the init/close effects. Validation (`validate`) stays separate.

- [ ] **Step 1: Parse `init`/`close`**

In the macro `Constraint` enum + `Parse`:
- `Constraint::Init { payer: syn::Ident, space: usize }` — parse `init` then expect following `payer = <ident>` and `space = <int>` (Anchor groups these; for M3 accept `#[account(init, payer = p, space = N)]` as three comma-separated args, collecting `payer`/`space` into the most recent `Init`). Simpler: parse `init` as a marker and parse sibling `payer = X` / `space = N` args, then assemble. Implement: collect raw args (`Signer`, `Mut`, `Owner`, `HasOne`, `InitMarker`, `Payer(Ident)`, `Space(usize)`, `CloseMarker`, `CloseDest(Ident)`) then post-process each field's arg list into final constraints (an `init` marker + a `payer` + a `space` → one `Init{payer,space}`; a `close` marker + `Close = dest`? Anchor uses `close = dest`. Use `close = <ident>` → `Constraint::Close(ident)`).
- Final field constraints: `Init { payer, space }` and `Close(dest)`.

- [ ] **Step 2: Generate `execute_lifecycle`**

In the derive output, add an `impl #name { pub fn execute_lifecycle(...) }`. For each `Init { payer, space }` at field index `i` with payer index `pi`:
```rust
quote! {
    {
        let space: usize = #space + 8;
        let ix = ::solana_program::system_instruction::create_account(
            accounts[#pi].key, accounts[#i].key, rent_lamports, space as u64, program_id);
        ::solana_program::program::invoke(&ix, accounts)
            .map_err(|_| ::verified_anchor::VAError::InitFailed { field: #fname })?;
        // write the 8-byte discriminator (zeros here; real disc is type-dependent, M5)
        let mut data = accounts[#i].try_borrow_mut_data()
            .map_err(|_| ::verified_anchor::VAError::InitFailed { field: #fname })?;
        for b in data[0..8].iter_mut() { *b = 0; }
    }
}
```
For each `Close(dest)` at field index `i` with dest index `di`:
```rust
quote! {
    {
        let bal = accounts[#i].lamports();
        **accounts[#di].try_borrow_mut_lamports()
            .map_err(|_| ::verified_anchor::VAError::CloseFailed { field: #fname })? += bal;
        **accounts[#i].try_borrow_mut_lamports()
            .map_err(|_| ::verified_anchor::VAError::CloseFailed { field: #fname })? = 0;
        let mut data = accounts[#i].try_borrow_mut_data()
            .map_err(|_| ::verified_anchor::VAError::CloseFailed { field: #fname })?;
        for b in data.iter_mut().take(8) { *b = 0xff; }   // closed-account marker
    }
}
```
Wrap in:
```rust
pub fn execute_lifecycle(
    accounts: &[::solana_program::account_info::AccountInfo],
    program_id: &::solana_program::pubkey::Pubkey,
    rent_lamports: u64,
) -> ::core::result::Result<(), ::verified_anchor::VAError> {
    #(#lifecycle_steps)*
    Ok(())
}
```
Add `VAError::InitFailed { field: &'static str }` and `CloseFailed { field: &'static str }` (+ Display arms) to `rust/verified-anchor/src/lib.rs`.

- [ ] **Step 3: Native compile check**

Run: `cd rust && cargo build 2>&1 | tail -5` and `cargo test -p verified-anchor 2>&1 | tail -8`.
Expected: compiles; existing tests still pass. (The effectful code isn't run natively here — `invoke` only works under a runtime; it's exercised in Task B4.)

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-macros/src/lib.rs rust/verified-anchor/src/lib.rs
git commit -m "feat(macros): generate effectful init/close (execute_lifecycle)"
```

---

## Task B3: BPF program crate + verified SBF build

**Files:** Create `rust/verified-anchor-program/Cargo.toml`, `rust/verified-anchor-program/src/lib.rs`; modify `rust/Cargo.toml` (workspace members — see note).

- [ ] **Step 1: Program crate manifest**

Create `rust/verified-anchor-program/Cargo.toml`:
```toml
[package]
name = "verified-anchor-program"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
solana-program = "2"
verified-anchor = { path = "../verified-anchor" }
```
NOTE: do NOT add this crate to the `[workspace] members` in `rust/Cargo.toml` if its `cdylib` build interferes with the native workspace `cargo test`. Instead add it as a workspace member but build it ONLY via `cargo-build-sbf` (which builds the single crate). If `cargo build` at the workspace root tries to cdylib-build it natively and fails, exclude it: add `exclude = ["verified-anchor-program"]` to `[workspace]` and build it standalone. Pick whichever keeps `cargo test` green; document the choice.

- [ ] **Step 2: Program entrypoint exercising generated code**

Create `rust/verified-anchor-program/src/lib.rs`:
```rust
use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult,
    pubkey::Pubkey, program_error::ProgramError,
};
use verified_anchor::{Validate, VerifiedAccounts};

// Instruction 0: init a new account (accounts: [new, payer, system_program]).
#[derive(VerifiedAccounts)]
struct InitOne {
    #[account(init, payer = payer, space = 0)]
    new: u8,
    #[account(mut, signer)]
    payer: u8,
    system_program: u8,
}

// Instruction 1: close an account (accounts: [target, dest]).
#[derive(VerifiedAccounts)]
struct CloseOne {
    #[account(close = dest)]
    target: u8,
    #[account(mut)]
    dest: u8,
}

entrypoint!(process);
pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    match data.first() {
        Some(0) => {
            InitOne::validate(accounts).map_err(|_| ProgramError::InvalidArgument)?;
            // rent-exempt minimum for 8 bytes; litesvm test passes enough lamports to payer
            let rent_lamports = 1_000_000u64;
            InitOne::execute_lifecycle(accounts, program_id, rent_lamports)
                .map_err(|_| ProgramError::InvalidArgument)?;
            Ok(())
        }
        Some(1) => {
            CloseOne::validate(accounts).map_err(|_| ProgramError::InvalidArgument)?;
            CloseOne::execute_lifecycle(accounts, program_id, 0)
                .map_err(|_| ProgramError::InvalidArgument)?;
            Ok(())
        }
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
```
NOTE: `space = 0` ⇒ allocate 8 bytes (discriminator only). `system_program` field is a spec carrier; `invoke` of `create_account` needs the system program AccountInfo present in `accounts` (it is — index 2). If `validate` rejects because `system_program`/`new` have no constraints, that's fine (no constraint ⇒ pass).

- [ ] **Step 3: Build to BPF with the verified recipe**

Run:
```bash
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-program && cargo-build-sbf --no-rustup-override 2>&1 | tail -8
ls -la target/deploy/verified_anchor_program.so
```
Expected: a `.so` is produced (warnings OK). Also confirm native `cargo test` at the workspace still works (Step 1's exclude/member decision).

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-program/ rust/Cargo.toml
git commit -m "feat(rust): BPF program crate exercising generated init/close; builds via cargo-build-sbf"
```

---

## Task B4: litesvm runtime tests

**Files:** Modify `rust/verified-anchor/Cargo.toml`; create `rust/verified-anchor/tests/runtime_lifecycle.rs`.

- [ ] **Step 1: Add dev-dependencies (with vendored OpenSSL)**

In `rust/verified-anchor/Cargo.toml` add:
```toml
[dev-dependencies]
litesvm = "0.6"
solana-sdk = "2"
openssl = { version = "0.10", features = ["vendored"] }
```

- [ ] **Step 2: Write the runtime test**

Create `rust/verified-anchor/tests/runtime_lifecycle.rs`. The test loads the program `.so` (built in B3) and runs init/close. Build the `.so` path from `CARGO_MANIFEST_DIR`:
```rust
use litesvm::LiteSVM;
use solana_sdk::{
    account::Account, instruction::{AccountMeta, Instruction}, message::Message,
    pubkey::Pubkey, signature::{Keypair, Signer}, system_program, transaction::Transaction,
};
use std::path::PathBuf;

fn so_path() -> PathBuf {
    // rust/verified-anchor/  ->  rust/verified-anchor-program/target/deploy/verified_anchor_program.so
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.push("verified-anchor-program/target/deploy/verified_anchor_program.so");
    p
}

#[test]
fn init_creates_and_funds_account() {
    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    svm.add_program_from_file(program_id, so_path()).expect("load .so (run cargo-build-sbf first)");

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000).unwrap();
    let new_acct = Keypair::new();

    let ix = Instruction::new_with_bytes(
        program_id,
        &[0u8],   // instruction 0 = init
        vec![
            AccountMeta::new(new_acct.pubkey(), true),    // new (signer for create_account)
            AccountMeta::new(payer.pubkey(), true),       // payer
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer, &new_acct], msg, svm.latest_blockhash());
    svm.send_transaction(tx).expect("init tx succeeds");

    let created = svm.get_account(&new_acct.pubkey()).expect("account exists");
    assert_eq!(created.owner, program_id, "new account owned by program");
    assert!(created.lamports > 0, "new account funded");
    assert_eq!(created.data.len(), 8, "8-byte discriminator space");
}

#[test]
fn close_drains_to_dest() {
    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    svm.add_program_from_file(program_id, so_path()).unwrap();

    // Seed a program-owned target account with lamports + a dest.
    let target = Pubkey::new_unique();
    let dest = Pubkey::new_unique();
    svm.set_account(target, Account { lamports: 5_000_000, data: vec![1u8; 8], owner: program_id, executable: false, rent_epoch: 0 }).unwrap();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000).unwrap();
    svm.set_account(dest, Account { lamports: 0, data: vec![], owner: system_program::id(), executable: false, rent_epoch: 0 }).unwrap();

    let ix = Instruction::new_with_bytes(
        program_id, &[1u8],   // instruction 1 = close
        vec![AccountMeta::new(target, false), AccountMeta::new(dest, false)],
    );
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer], msg, svm.latest_blockhash());
    svm.send_transaction(tx).expect("close tx succeeds");

    let d = svm.get_account(&dest).unwrap();
    assert_eq!(d.lamports, 5_000_000, "lamports moved to dest");
    let t = svm.get_account(&target).map(|a| a.lamports).unwrap_or(0);
    assert_eq!(t, 0, "target drained");
}
```
NOTE: this test REQUIRES the `.so` from Task B3 to exist. The litesvm API names (`add_program_from_file`, `set_account`, `airdrop`, `send_transaction`, `get_account`, `latest_blockhash`) are for litesvm 0.6 — verify against the actual 0.6 API and adjust (use `cargo doc -p litesvm --no-deps` or the crate source if a name differs). If `close` requires the target to be writable and the program to own it for the lamport mutation to persist, the `set_account` owner=program_id handles that. If a program-owned account's lamports can't be set directly, fund via a transfer instead.

- [ ] **Step 3: Run the runtime tests**

Run (must build the .so first, then run native tests which compile vendored OpenSSL — slow first time):
```bash
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-program && cargo-build-sbf --no-rustup-override 2>&1 | tail -3
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test runtime_lifecycle 2>&1 | tail -20
```
Expected: both runtime tests pass (init creates+funds the account; close moves lamports). If litesvm API mismatches surface, fix the test calls (this is the most likely iteration point). If `send_transaction` returns a structured result, assert `.is_ok()` / unwrap the `TransactionMetadata`.

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor/Cargo.toml rust/verified-anchor/tests/runtime_lifecycle.rs
git commit -m "test(runtime): litesvm tests for generated init/close effects"
```

---

## Task F1: Bridge doc, extended example, root wiring, full builds

**Files:** Modify `lean/VerifiedAnchor.lean`, `lean/VerifiedAnchor/Codegen/ExampleGenerated.lean`, `docs/verified-anchor-bridge.md`.

- [ ] **Step 1: Wire `Lifecycle` into the root + fix any M2→M3 example references**

In `lean/VerifiedAnchor.lean` add `import VerifiedAnchor.Codegen.Lifecycle`. In `ExampleGenerated.lean`, if `transfer_M2`/`M2Subset` were referenced, rename to `M3Subset` (the `transfer` struct is still in the subset). Confirm the existing `transfer_good_validates` proof still goes through `genValidate_sound` (now M3-typed).

- [ ] **Step 2: Add a has_one closed-loop + a lifecycle example**

Append to `ExampleGenerated.lean` (namespace `VerifiedAnchor.Codegen.Examples`):
```lean
/-- has_one example: vault stores authority at offset 8; checkConstraint bites on concrete data. -/
def vaultLayout : FieldLayout := [("authority", 8)]
def withHasOne : AccountsStruct :=
  { programId := Pubkey.zero
  , fields := [ { name := "vault", ty := AccountType.account "Vault" vaultLayout Pubkey.zero,
                  constraints := [Constraint.hasOne "authority"] }
              , { name := "authority", ty := AccountType.uncheckedAccount, constraints := [] } ] }
def authKeyE : Pubkey := Pubkey.ofBytes (List.replicate 32 5)
def vaultDataE (stored : Pubkey) : ByteArray := (ByteArray.mk (Array.replicate 8 0)) ++ ByteArray.mk stored.toArray
def hoGood : Ctx :=
  [ { key := Pubkey.zero, lamports := 0, data := vaultDataE authKeyE, owner := Pubkey.zero, rentEpoch := 0, isSigner := false, isWritable := false, executable := false }
  , { key := authKeyE, lamports := 0, data := ByteArray.empty, owner := Pubkey.zero, rentEpoch := 0, isSigner := false, isWritable := false, executable := false } ]
-- has_one is crypto-free, so its per-constraint check reduces:
#guard checkConstraint withHasOne hoGood 0 (withHasOne.fields.head!) (Constraint.hasOne "authority") = true

/-- lifecycle example: applyInit then the M1 init post-condition holds (via the Hoare theorem). -/
def lcPre : Ctx :=
  [ { key := Pubkey.zero, lamports := 0, data := ByteArray.empty, owner := Pubkey.zero, rentEpoch := 0, isSigner := false, isWritable := false, executable := false }
  , { key := Pubkey.zero, lamports := 1000, data := ByteArray.empty, owner := Pubkey.zero, rentEpoch := 0, isSigner := true, isWritable := true, executable := false } ]
#guard (applyInit 0 1 0 Pubkey.zero (ByteArray.mk (Array.replicate 8 0)) 500 lcPre).isSome
```
NOTE: `checkConstraint` is the M1 per-constraint Bool checker (exists from M1 `Decision/Check.lean`). If `withHasOne.fields.head!` is awkward, bind the field to a `def`. Ensure `Array.replicate`/`mkArray` matches your Lean version. These `#guard`s must evaluate (all data concrete; no opaque sha256 in has_one/init paths).

- [ ] **Step 3: Update the bridge doc**

In `docs/verified-anchor-bridge.md`, add to the correspondence table:
```markdown
| `if data[8..40] != target.key { Err(WrongHasOne) }` | `genHasOne` (read 32B @ offset 8, compare) | `satisfies … (.hasOne field)` |
| `invoke(create_account(...)) + write disc` | `applyInit` (state transformer) | `init_establishes_post` ⇒ `satisfies … (.init …)` |
| `dest.lamports += t.lamports; t.lamports=0; mark` | `applyClose` (state transformer) | `close_establishes_post` ⇒ `satisfies … (.close dest)` |
```
And add a "Lifecycle / Hoare framework (M3)" section stating: init/close are modeled as
`Ctx → Option Ctx` transformers whose post-state provably satisfies the M1 post-conditions;
the **new trusted modeling assumption** is that `system_instruction::create_account`'s
on-chain effect matches `applyInit` (its documented effect on account state); the generated
effectful Rust is exercised under litesvm (`tests/runtime_lifecycle.rs`), not just shared
vectors; still out of scope: the CPI dispatch/validator and rustc/sBPF codegen.

- [ ] **Step 4: Full builds + all gates**

Run:
```bash
export PATH="$HOME/.elan/bin:$PATH"; cd /home/parth/Desktop/PARTH/Verification/lean && lake build 2>&1 | tail -3
grep -rn "sorry\|admit" VerifiedAnchor/ || echo "PASS lean zero-sorry"
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test behavior --test lean_spec 2>&1 | grep "test result"
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-program && cargo-build-sbf --no-rustup-override 2>&1 | tail -2
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test runtime_lifecycle 2>&1 | grep "test result"
```
Expected: lake green + zero sorry; native tests pass; .so builds; runtime tests pass.

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/ docs/verified-anchor-bridge.md
git commit -m "docs+wiring: M3 bridge rows, has_one+lifecycle examples, Lifecycle root import; M3 green"
```

---

## Done-bar verification (after F1)

1. `cargo test` (behavior + lean_spec) green; has_one unit tests pass. ✅ (A4, F1)
2. `verified-anchor-program` builds to `.so` via the §recipe. ✅ (B3, F1)
3. litesvm `runtime_lifecycle`: init creates+funds+sizes; close drains. ✅ (B4, F1)
4. `lake build` green, zero `sorry`, incl. new Codegen modules. ✅ (F1)
5. `genValidate_sound` @ M3Subset; `genHasOne_iff`/`genDiscriminator_iff`; `init_establishes_post`/`close_establishes_post` — all proved, no `sorryAx`. ✅ (A2,A3,B1)
6. Extended closed-loop + lifecycle examples build. ✅ (F1)
7. Bridge doc has has_one + init/close rows + modeled-effect trust. ✅ (F1)
8. M1+M2 still green. ✅ (F1 full `lake build` + native tests)
```
