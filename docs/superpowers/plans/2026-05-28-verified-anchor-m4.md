# Verified Anchor — Milestone 4 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Verify `#[account(seeds = [...], bump)]` PDA derivation — wire `.seeds` into the `genValidate` proof framework (canonical-only) with a `genSeeds` model + `M4Subset` soundness, expand the seed sources to include instruction-argument slices (concrete `Ctx.instrData`), and generate a PDA check in the Rust macro tested natively (real `find_program_address`) and under litesvm.

**Architecture:** `.seeds` is a *pure check* (like `has_one`), so it extends `genValidate` — not the Hoare layer. The M1 contract `satisfies .seeds` already exists (canonical-only via `findProgramAddress`); M4 adds the codegen mirror + soundness, makes `Ctx` a structure carrying raw instruction data, and adds a third `SeedSpec` variant (`instrArg off len`). The generated `validate` gains `instr_data: &[u8]` and `program_id: &Pubkey` (the runtime carriers of the Lean `c.instrData` / `s.programId`).

**Tech Stack:** Lean 4.30 / Lake (M1–M3 lib); Rust 1.93.1 / `solana-program 2.3.0`, `syn`/`quote`; SBF toolchain (`solana-cli 4.0.0`); `litesvm 0.6` + vendored OpenSSL for runtime tests (all installed in M3).

---

## Conventions

- **Lean:** prefix lake with `export PATH="$HOME/.elan/bin:$PATH"`; work in `lean/`. Test = `lake build` (a failing `#guard`/proof is a build error). Zero `sorry`/`admit`; prove theorems for real. After a headline theorem, confirm `#print axioms <thm>` shows no `sorryAx`.
- **Rust (native):** work in `rust/`; `cargo` on PATH; test = `cargo test -p verified-anchor`.
- **SBF build (Task R4 only) — the verified recipe:**
  ```bash
  export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
  cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-program && cargo-build-sbf --no-rustup-override
  # produces rust/target/deploy/verified_anchor_program.so
  ```
  The platform-tools rustc (has the `sbf-solana-solana` target) MUST be first on PATH; `--no-rustup-override` avoids the rustup toolchain-name bug.
- **litesvm dev-deps** are already in `rust/verified-anchor/Cargo.toml` (`litesvm`, `solana-sdk`, vendored `openssl`) from M3 — no Cargo.toml change needed for runtime tests.
- Commit after each task. `.gitignore` covers `target/`, `lean/.lake/`.
- **Key invariant — account access goes through helpers.** `satisfies`/`genConstraint` read accounts via `Ctx.atField`/`Ctx.lookup` and length via `c.length` (never `c[i]?` directly except in `Lifecycle.lean`). So making `Ctx` a structure with a `Ctx.length` def + `Ctx.atField`/`Ctx.lookup` reading `.accounts` keeps those files' *call sites* unchanged.

---

## File structure

| File | Responsibility |
|------|----------------|
| `lean/VerifiedAnchor/Constraints/Context.lean` | (MODIFY) `Ctx` → structure `{accounts, instrData}`; `Ctx.ofAccounts`; `Ctx.length`; `Ctx.lookup`/`atField`/`WellFormed` via `.accounts` |
| `lean/VerifiedAnchor/Constraints/Ast.lean` | (MODIFY) `SeedSpec` += `instrArg (off len : Nat)` |
| `lean/VerifiedAnchor/Contract/Satisfies.lean` | (MODIFY) `resolveSeeds` += `instrArg` case (reads `c.instrData`) |
| `lean/VerifiedAnchor/Codegen/Lifecycle.lean` | (MODIFY) `Ctx.update` via `.accounts` (preserves `instrData`); `Ctx.accounts_update` lemma; init/close theorems over `.accounts` |
| `lean/VerifiedAnchor/Codegen/Generated.lean` | (MODIFY) `bumpMatchesB`; `genSeeds`; `.seeds` case in `genConstraint` |
| `lean/VerifiedAnchor/Codegen/Soundness.lean` | (MODIFY) `bumpMatchesB_iff`; `genConstraint_seeds_iff`; `isM4Constraint`/`M4Subset`; `genValidate_sound` @ M4 |
| `lean/VerifiedAnchor/Codegen/ExampleGenerated.lean` | (MODIFY) `Ctx.ofAccounts` literals; `.accounts` in `lc_init_establishes`; a seeds example |
| `lean/VerifiedAnchor/Examples/Withdraw.lean` | (MODIFY) `Ctx.ofAccounts` literals (mechanical) |
| `rust/verified-anchor/src/lib.rs` | (MODIFY) `VAError` += `WrongPda`/`WrongBump`; `Validate::validate` gains `instr_data` + `program_id` |
| `rust/verified-anchor-macros/src/lib.rs` | (MODIFY) parse `seeds`/`bump`; emit PDA check; extend `validate` sig + `lean_spec` |
| `rust/verified-anchor/tests/behavior.rs` | (MODIFY) seeds accept/reject (native, real `find_program_address`) + update all `validate` calls |
| `rust/verified-anchor/tests/lean_spec.rs` | (MODIFY) a seeds-spec assertion |
| `rust/verified-anchor-program/src/lib.rs` | (MODIFY) a seeds-validated instruction (literal + instrArg) + update `validate` calls |
| `rust/verified-anchor/tests/runtime_seeds.rs` | (NEW) litesvm: good PDA → Ok, wrong PDA → on-chain error |
| `docs/verified-anchor-bridge.md` | (MODIFY) seeds row; canonical-only boundary; `validate` signature change |

---

# PART L — Lean: contract + model + soundness

## Task L1: `Ctx` becomes a structure (keep M1–M3 green)

**Files:** Modify `lean/VerifiedAnchor/Constraints/Context.lean`, `lean/VerifiedAnchor/Codegen/Lifecycle.lean`, `lean/VerifiedAnchor/Codegen/ExampleGenerated.lean`, `lean/VerifiedAnchor/Examples/Withdraw.lean`.

This is a pure refactor (no seeds yet): turn `Ctx` into a structure and re-green the whole build. Isolating it keeps the diff reviewable.

- [ ] **Step 1: Rewrite `Context.lean`**

Replace the body of `lean/VerifiedAnchor/Constraints/Context.lean` (keep the two imports + namespace) with:
```lean
import VerifiedAnchor.Constraints.Ast
import VerifiedAnchor.Solana.Account

namespace VerifiedAnchor

/-- The runtime context: accounts positionally aligned with `AccountsStruct.fields`,
    plus the raw instruction data (used by `seeds = [arg(..)]`). -/
structure Ctx where
  accounts  : List AccountInfo
  instrData : ByteArray := ByteArray.empty
  deriving Inhabited

/-- Build a Ctx from just accounts (instrData empty). Keeps existing examples terse. -/
def Ctx.ofAccounts (l : List AccountInfo) : Ctx := { accounts := l }

/-- Number of runtime accounts. -/
def Ctx.length (c : Ctx) : Nat := c.accounts.length

/-- Resolve a declared field name to its account, by matching field position. -/
def Ctx.lookup (s : AccountsStruct) (c : Ctx) (name : String) : Option AccountInfo := do
  let idx ← List.findIdx? (·.name == name) s.fields
  c.accounts[idx]?

/-- Resolve the account paired with a specific field (by index in the struct). -/
def Ctx.atField (_s : AccountsStruct) (c : Ctx) (idx : Nat) : Option AccountInfo :=
  c.accounts[idx]?

/-- Structural well-formedness: one account per declared field. -/
def WellFormed (s : AccountsStruct) (c : Ctx) : Prop :=
  c.length = s.fields.length

instance (s : AccountsStruct) (c : Ctx) : Decidable (WellFormed s c) :=
  inferInstanceAs (Decidable (c.length = s.fields.length))

end VerifiedAnchor
```
NOTE: `c.length` is now `Ctx.length c` (dot notation), so `genValidate` and `WellFormed`'s `c.length` keep working unchanged. `Ctx.atField`/`Ctx.lookup` read `.accounts`, so every caller in `Satisfies.lean`/`Generated.lean` is untouched.

- [ ] **Step 2: Rewrite `Lifecycle.lean`'s `Ctx.update` + read-back lemma to use `.accounts`**

In `lean/VerifiedAnchor/Codegen/Lifecycle.lean`, replace `Ctx.update` and `Ctx.getElem?_update` with `.accounts`-based versions. `Ctx.update`:
```lean
/-- Update the account at index `i` (no-op if out of range), preserving `instrData`. -/
def Ctx.update (c : Ctx) (i : Nat) (g : AccountInfo → AccountInfo) : Ctx :=
  match c.accounts[i]? with
  | some a => { c with accounts := c.accounts.set i (g a) }
  | none => c
```
Replace the `Ctx.getElem?_update` theorem with an `.accounts` read-back lemma:
```lean
theorem Ctx.accounts_update (c : Ctx) (i j : Nat) (g : AccountInfo → AccountInfo) :
    (c.update j g).accounts[i]? = if i = j then (c.accounts[i]?).map g else c.accounts[i]? := by
  unfold Ctx.update
  cases hj : c.accounts[j]? with
  | none =>
    have : ¬ j < c.accounts.length := by
      intro hlt; rw [List.getElem?_eq_getElem hlt] at hj; exact (Option.some_ne_none _) hj
    by_cases hij : i = j
    · subst hij; simp [hj]
    · simp [hij]
  | some a =>
    by_cases hij : i = j
    · subst hij
      have hlt : i < c.accounts.length := by
        rw [List.getElem?_eq_some_iff] at hj; exact hj.1
      simp [List.getElem?_set_self, hlt, hj]
    · simp [List.getElem?_set_ne, hij]
```
NOTE: the exact lemma names in Lean 4.30 are `List.getElem?_set_self` and `List.getElem?_set_ne`. If a name mismatches, use `simp [List.getElem?_set]` and split on `i = j`, or find the precise lemma with `exact?`. The proof obligation is purely the standard `List.set` read-back; do not introduce `sorry`.

- [ ] **Step 3: Update `applyInit`/`applyClose` and the two Hoare theorems to read `.accounts`**

In `applyInit`/`applyClose`, change every `c[idx]?`/`c[payerIdx]?`/`c[destIdx]?` to `c.accounts[idx]?` etc. In the two theorems `init_establishes_post`/`close_establishes_post`, change the post-state reads `c'[idx]?` to `c'.accounts[idx]?`, and in their proofs replace `Ctx.getElem?_update` with `Ctx.accounts_update`. Everything else in the proofs is unchanged (the `List.set` read-back facts are now delivered by `Ctx.accounts_update`).

Concretely, the do-blocks become (init shown; close analogous):
```lean
def applyInit (idx payerIdx : Nat) (space : Nat) (owner : Pubkey) (disc : ByteArray)
    (rent : UInt64) (c : Ctx) : Option Ctx := do
  guard (idx ≠ payerIdx)
  let a ← c.accounts[idx]?
  let p ← c.accounts[payerIdx]?
  guard (p.isSigner && p.isWritable && rent ≤ p.lamports && a.data.size == 0)
  let newData := disc ++ ByteArray.mk (Array.replicate (space + 8 - disc.size) 0)
  let c := c.update idx (fun a => { a with owner := owner, data := newData,
                                    lamports := a.lamports + rent })
  let c := c.update payerIdx (fun p => { p with lamports := p.lamports - rent })
  some c
```
and the theorem signatures change only their conclusion's read:
```lean
theorem init_establishes_post
    (idx payerIdx space owner disc rent c c') (hdisc : disc.size = 8)
    (h : applyInit idx payerIdx space owner disc rent c = some c') :
    ∃ a, c'.accounts[idx]? = some a ∧ a.owner = owner ∧ space + 8 ≤ a.data.size := ...
theorem close_establishes_post
    (idx destIdx c c')
    (h : applyClose idx destIdx c = some c') :
    ∃ a, c'.accounts[idx]? = some a ∧ a.lamports = 0
        ∧ hasDiscriminator a closedAccountDiscriminator := ...
```
NOTE: keep the existing proof bodies; only swap `c[..]?`→`c.accounts[..]?` and `Ctx.getElem?_update`→`Ctx.accounts_update`. If the do-notation `guard`/bind destructuring in `h` referenced `c[..]?` by name in a `simp`/`split`, update those references too.

- [ ] **Step 4: Fix example `Ctx` literals**

In `lean/VerifiedAnchor/Examples/Withdraw.lean`, change the four `def … : Ctx := [..]` (lines ~73, 76, 118, 122) to wrap with `Ctx.ofAccounts`, e.g.:
```lean
def goodCtx : Ctx := Ctx.ofAccounts [mkVault authKey progId, authorityAccount]
```
(apply to `goodCtx`, `tamperedCtx`, `goodCtxT`, `tamperedCtxT`).

In `lean/VerifiedAnchor/Codegen/ExampleGenerated.lean`, change `goodCtx`, `tamperedCtx`, `hoGood`, `hoBad`, `lcPre` to `Ctx.ofAccounts [..]`, and in `lc_init_establishes` change the conclusion `c'[0]?` to `c'.accounts[0]?`:
```lean
def goodCtx : Ctx := Ctx.ofAccounts [vaultAcct, authAcct]
def tamperedCtx : Ctx := Ctx.ofAccounts [vaultAcct, { authAcct with isSigner := false }]
...
def hoGood : Ctx := Ctx.ofAccounts [hoVault authKeyE, hoAuthority]
def hoBad : Ctx := Ctx.ofAccounts [hoVault (Pubkey.ofBytes (List.replicate 32 6)), hoAuthority]
def lcPre : Ctx := Ctx.ofAccounts
  [ { key := Pubkey.zero, lamports := 0, data := ByteArray.empty, owner := Pubkey.zero,
      rentEpoch := 0, isSigner := false, isWritable := false, executable := false }
  , { key := Pubkey.zero, lamports := 1000, data := ByteArray.empty, owner := Pubkey.zero,
      rentEpoch := 0, isSigner := true, isWritable := true, executable := false } ]
theorem lc_init_establishes :
    ∀ c', applyInit 0 1 0 Pubkey.zero lcDisc 500 lcPre = some c' →
      ∃ a, c'.accounts[0]? = some a ∧ a.owner = Pubkey.zero ∧ 0 + 8 ≤ a.data.size :=
  fun c' h => init_establishes_post 0 1 0 Pubkey.zero lcDisc 500 lcPre c' (by decide) (by decide) h
```

- [ ] **Step 5: Full build + zero-sorry + axioms unchanged**

Run:
```bash
export PATH="$HOME/.elan/bin:$PATH"; cd /home/parth/Desktop/PARTH/Verification/lean
lake build 2>&1 | tail -5
grep -rn "sorry\|admit" VerifiedAnchor/ || echo "clean"
printf 'import VerifiedAnchor.Codegen.Soundness\nimport VerifiedAnchor.Codegen.Lifecycle\n#print axioms VerifiedAnchor.genValidate_sound\n#print axioms VerifiedAnchor.init_establishes_post\n' > VerifiedAnchor/AxTmp.lean
lake env lean VerifiedAnchor/AxTmp.lean; rm -f VerifiedAnchor/AxTmp.lean
```
Expected: build green (all M1–M3 `#guard`s/theorems still pass); `clean`; axioms `[propext, Quot.sound]`.

- [ ] **Step 6: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/VerifiedAnchor/Constraints/Context.lean lean/VerifiedAnchor/Codegen/Lifecycle.lean lean/VerifiedAnchor/Codegen/ExampleGenerated.lean lean/VerifiedAnchor/Examples/Withdraw.lean
git commit -m "refactor(lean): Ctx becomes a structure carrying instrData (M1-M3 still green)"
```

---

## Task L2: `SeedSpec.instrArg` + `resolveSeeds` case

**Files:** Modify `lean/VerifiedAnchor/Constraints/Ast.lean`, `lean/VerifiedAnchor/Contract/Satisfies.lean`.

- [ ] **Step 1: Add the `instrArg` variant**

In `lean/VerifiedAnchor/Constraints/Ast.lean`, replace the `SeedSpec` inductive with:
```lean
/-- A single seed in a PDA derivation. -/
inductive SeedSpec where
  | literal (bytes : ByteArray)        -- e.g. b"vault"
  | fieldKey (field : String)          -- another account's key bytes
  | instrArg (off : Nat) (len : Nat)   -- a concrete slice of the instruction data
  deriving Inhabited
```

- [ ] **Step 2: Add the `instrArg` case to `resolveSeeds`**

In `lean/VerifiedAnchor/Contract/Satisfies.lean`, the `resolveSeeds` function currently has two match arms (`.literal`, `.fieldKey`). Add a third:
```lean
def resolveSeeds (s : AccountsStruct) (c : Ctx) : List SeedSpec → List ByteArray
  | [] => []
  | .literal bytes :: rest => bytes :: resolveSeeds s c rest
  | .fieldKey name :: rest =>
      (match Ctx.lookup s c name with
       | some a => ByteArray.mk a.key.toArray
       | none => ByteArray.empty) :: resolveSeeds s c rest
  | .instrArg off len :: rest =>
      c.instrData.extract off (off + len) :: resolveSeeds s c rest
```
NOTE: `satisfies .seeds` itself is unchanged — it already calls `findProgramAddress (resolveSeeds s c ss) s.programId`. `ByteArray.extract a b e` returns bytes `[b, e)` (the same API used by `AccountInfo.dataPrefix`).

- [ ] **Step 3: Build the two files**

Run:
```bash
export PATH="$HOME/.elan/bin:$PATH"; cd /home/parth/Desktop/PARTH/Verification/lean
lake build VerifiedAnchor.Contract.Satisfies 2>&1 | tail -5
```
Expected: success (the `Decidable (satisfies …)` instance still derives — `.seeds` decidability is unaffected by the new resolve arm).

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/VerifiedAnchor/Constraints/Ast.lean lean/VerifiedAnchor/Contract/Satisfies.lean
git commit -m "feat(lean): SeedSpec.instrArg + resolveSeeds instruction-data slice"
```

---

## Task L3: `genSeeds` codegen model + `.seeds` wiring

**Files:** Modify `lean/VerifiedAnchor/Codegen/Generated.lean`.

- [ ] **Step 1: Write the failing example assertion (drives `genSeeds`)**

At the end of `lean/VerifiedAnchor/Codegen/Generated.lean` (inside `namespace VerifiedAnchor`, before `end`), temporarily add a `#guard` exercising the crypto-free part — `bumpMatchesB` — so the build fails until `bumpMatchesB`/`genSeeds` exist:
```lean
-- TEMP (removed in Step 4): drives bumpMatchesB into existence
#guard bumpMatchesB BumpSpec.canonical 7 = true
#guard bumpMatchesB (BumpSpec.declared 7) 7 = true
#guard bumpMatchesB (BumpSpec.declared 7) 8 = false
```
Run `lake build VerifiedAnchor.Codegen.Generated 2>&1 | tail` — expect FAIL (`unknown identifier bumpMatchesB`).

- [ ] **Step 2: Add `bumpMatchesB` and `genSeeds`, wire `.seeds`**

In `Generated.lean`, add (before `genConstraint`):
```lean
/-- Bool mirror of `bumpMatches`: declared bumps must match exactly; canonical accepts any. -/
def bumpMatchesB : BumpSpec → UInt8 → Bool
  | .declared db, actual => actual == db
  | .canonical,  _       => true

/-- Operational PDA check: derive the canonical PDA from the resolved seeds and the
    program id, require it equals the account key, and the bump matches. None-safe. -/
def genSeeds (s : AccountsStruct) (c : Ctx) (idx : Nat)
    (ss : List SeedSpec) (b : BumpSpec) : Bool :=
  (Ctx.atField s c idx).allB (fun a =>
    (findProgramAddress (resolveSeeds s c ss) s.programId).allB (fun pr =>
      decide (pr.1 = a.key) && bumpMatchesB b pr.2))
```
and in `genConstraint`, replace the catch-all `| _ => false` with:
```lean
  | .seeds ss b      => genSeeds s c idx ss b
  | _                => false
```
(`Generated.lean` already imports `Contract.Validates` → `Satisfies` → `Crypto`, so `findProgramAddress`/`resolveSeeds` are in scope.)

- [ ] **Step 3: Build, confirm the temp guards pass**

Run `lake build VerifiedAnchor.Codegen.Generated 2>&1 | tail` — expect success (the three `bumpMatchesB` `#guard`s evaluate).

- [ ] **Step 4: Remove the temp guards**

Delete the three `-- TEMP` `#guard` lines from Step 1.

- [ ] **Step 5: Build + commit**
```bash
export PATH="$HOME/.elan/bin:$PATH"; cd /home/parth/Desktop/PARTH/Verification/lean
lake build VerifiedAnchor.Codegen.Generated 2>&1 | tail -3
cd /home/parth/Desktop/PARTH/Verification
git add lean/VerifiedAnchor/Codegen/Generated.lean
git commit -m "feat(lean): genSeeds model + bumpMatchesB; wire .seeds into genConstraint"
```

---

## Task L4: seeds soundness — `M4Subset` + `genValidate_sound` @ M4

**Files:** Modify `lean/VerifiedAnchor/Codegen/Soundness.lean`.

Recall the current `Soundness.lean` defines `isM3Constraint`, `M3Subset`, `genConstraint_iff_satisfies_M3`, `genFieldValidate_iff` (parameterized by an `isM3Constraint`-membership hypothesis), and `genValidate_sound` (taking `M3Subset`). We extend to M4. The cleanest approach: keep the M3 names but ADD the seeds lemma and a NEW `isM4Constraint`/`M4Subset`, then re-point `genFieldValidate_iff`/`genValidate_sound` at M4.

- [ ] **Step 1: Prove `bumpMatchesB_iff` and `genConstraint_seeds_iff`**

In `Soundness.lean`, after the existing `genConstraint_hasOne_iff`, add:
```lean
theorem bumpMatchesB_iff (b : BumpSpec) (x : UInt8) :
    bumpMatchesB b x = true ↔ bumpMatches b x := by
  cases b with
  | declared db => simp [bumpMatchesB, bumpMatches]
  | canonical   => simp [bumpMatchesB, bumpMatches]

theorem genConstraint_seeds_iff (s c idx f ss b) :
    genConstraint s c idx f (Constraint.seeds ss b) = true
      ↔ satisfies s c idx f (Constraint.seeds ss b) := by
  simp only [genConstraint, genSeeds, satisfies]
  rw [Option.allB_iff]
  apply Option.satisfiesSome_congr   -- see NOTE
  intro a
  rw [Option.allB_iff]
  constructor
  · rintro ⟨pr, hpr, hand⟩
    rw [Bool.and_eq_true, decide_eq_true_iff] at hand
    exact ⟨pr, hpr, hand.1, (bumpMatchesB_iff b pr.2).mp hand.2⟩
  · rintro ⟨pr, hpr, hk, hbump⟩
    refine ⟨pr, hpr, ?_⟩
    rw [Bool.and_eq_true, decide_eq_true_iff]
    exact ⟨hk, (bumpMatchesB_iff b pr.2).mpr hbump⟩
```
NOTE: `satisfies .seeds` is `(Ctx.atField …).satisfiesSome (fun a => (findProgramAddress …).satisfiesSome (fun pr => pr.1 = a.key ∧ bumpMatches b pr.2))`, and `genSeeds` is the `allB` analogue with `decide (pr.1 = a.key) && bumpMatchesB b pr.2`. There may be no `Option.satisfiesSome_congr` lemma in the codebase; if so, do NOT call it — instead unfold both `Option.allB`/`Option.satisfiesSome` and case-split the two `Option`s directly, mirroring `genConstraint_hasOne_iff`'s nested structure:
```lean
theorem genConstraint_seeds_iff (s c idx f ss b) :
    genConstraint s c idx f (Constraint.seeds ss b) = true
      ↔ satisfies s c idx f (Constraint.seeds ss b) := by
  simp only [genConstraint, genSeeds, satisfies, Option.allB, Option.satisfiesSome]
  cases Ctx.atField s c idx with
  | none => simp
  | some a =>
    cases findProgramAddress (resolveSeeds s c ss) s.programId with
    | none => simp
    | some pr =>
      simp only [Bool.and_eq_true, decide_eq_true_iff, Option.some.injEq, exists_eq_left']
      rw [bumpMatchesB_iff]
```
Use whichever form compiles; the second is self-contained. If `simp` leaves a residual `∃`, add `exists_eq_left'`/`Option.some.injEq`. No `sorry`.

- [ ] **Step 2: Add `isM4Constraint` / `M4Subset` and the M4 dispatcher**

After `genConstraint_iff_satisfies_M3` (or replacing the M3 subset block — keep M3 names for back-compat with any references), add:
```lean
/-- Constraint kinds M4's generated validator handles (M3 + seeds). -/
def isM4Constraint : Constraint → Bool
  | .signer | .mut | .owner _ | .hasOne _ | .discriminator _ | .seeds _ _ => true
  | _ => false

/-- The M4 subset: every field's (implied ++ explicit) constraints are M4 validation
    constraints. Admits typed `.account` (implied owner+discriminator) AND `.seeds`. -/
def M4Subset (s : AccountsStruct) : Prop :=
  ∀ f ∈ s.fields, ∀ k ∈ (f.ty.impliedConstraints ++ f.constraints), isM4Constraint k = true

instance (s : AccountsStruct) : Decidable (M4Subset s) := by unfold M4Subset; infer_instance

/-- Dispatcher: under M4, the generated check of any constraint agrees with `satisfies`. -/
theorem genConstraint_iff_satisfies_M4 (s c idx f k) (hk : isM4Constraint k = true) :
    genConstraint s c idx f k = true ↔ satisfies s c idx f k := by
  cases k with
  | signer          => exact genConstraint_signer_iff s c idx f
  | «mut»           => exact genConstraint_mut_iff s c idx f
  | owner e         => exact genConstraint_owner_iff s c idx f e
  | hasOne field    => exact genConstraint_hasOne_iff s c idx f field
  | discriminator d => exact genConstraint_discriminator_iff s c idx f d
  | seeds ss b      => exact genConstraint_seeds_iff s c idx f ss b
  | _               => simp [isM4Constraint] at hk
```

- [ ] **Step 3: Re-point `genFieldValidate_iff` and `genValidate_sound` at M4**

Replace the existing `genFieldValidate_iff` and `genValidate_sound` with M4 versions (identical proof bodies, M4 names):
```lean
theorem genFieldValidate_iff (s c idx f)
    (hcons : ∀ k ∈ (f.ty.impliedConstraints ++ f.constraints), isM4Constraint k = true) :
    genFieldValidate s c idx f = true ↔ fieldValidates s c idx f := by
  unfold genFieldValidate fieldValidates
  rw [List.all_eq_true]
  constructor
  · intro hall k hk; exact (genConstraint_iff_satisfies_M4 s c idx f k (hcons k hk)).mp (hall k hk)
  · intro hall k hk; exact (genConstraint_iff_satisfies_M4 s c idx f k (hcons k hk)).mpr (hall k hk)

theorem genValidate_sound (s : AccountsStruct) (c : Ctx) (h : M4Subset s) :
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
NOTE: this REPLACES the M3-typed `genFieldValidate_iff`/`genValidate_sound`. `ExampleGenerated.lean` references `genValidate_sound` and `transfer_M3 : M3Subset transfer`. Keep `isM3Constraint`/`M3Subset`/`genConstraint_iff_satisfies_M3` in the file (harmless) so `transfer_M3` still compiles; Task L5 updates the example to `M4Subset`. Since `M3Subset ⊆ M4Subset` definitionally is NOT automatic, the example must switch to `M4Subset` (L5). Do not delete M3 defs in this task to avoid breaking the build mid-task.

- [ ] **Step 4: Build + axioms**

Run:
```bash
export PATH="$HOME/.elan/bin:$PATH"; cd /home/parth/Desktop/PARTH/Verification/lean
lake build VerifiedAnchor.Codegen.Soundness 2>&1 | tail -5
printf 'import VerifiedAnchor.Codegen.Soundness\n#print axioms VerifiedAnchor.genValidate_sound\n#print axioms VerifiedAnchor.genConstraint_seeds_iff\n' > VerifiedAnchor/AxTmp.lean
lake env lean VerifiedAnchor/AxTmp.lean; rm -f VerifiedAnchor/AxTmp.lean
```
Expected: `Soundness` builds (note: `ExampleGenerated` may now fail because `transfer_M3` feeds `M3Subset` into the M4-typed `genValidate_sound` — that is fixed in L5; building just `Soundness` here isolates this task). Axioms `[propext, Quot.sound]`.

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/VerifiedAnchor/Codegen/Soundness.lean
git commit -m "feat(lean): genConstraint_seeds_iff + M4Subset; genValidate_sound @ M4"
```

---

## Task L5: seeds closed-loop example + root build green

**Files:** Modify `lean/VerifiedAnchor/Codegen/ExampleGenerated.lean`.

- [ ] **Step 1: Switch the existing example to `M4Subset`**

In `ExampleGenerated.lean`, replace `transfer_M3`/its use with M4:
```lean
/-- `transfer` is in the M4 subset (only unchecked types, only mut/signer). -/
theorem transfer_M4 : M4Subset transfer := by decide

theorem transfer_good_validates : validates transfer goodCtx :=
  (genValidate_sound transfer goodCtx transfer_M4).mp (by decide)
```
(Delete the old `transfer_M3` def and update the `transfer_good_validates` proof to pass `transfer_M4`.)

- [ ] **Step 2: Add the seeds example (concrete resolve + symbolic soundness)**

Append to `ExampleGenerated.lean` (before `end VerifiedAnchor.Codegen.Examples`):
```lean
/-! ## seeds / PDA closed-loop (M4)

PDA derivation hashes through the opaque `sha256`, so `genSeeds` does NOT reduce under
`decide` (same wall as `discriminator`). We therefore demonstrate two honest halves:
* `resolveSeeds` is crypto-free, so the instruction-arg slice + literal + fieldKey
  resolution reduces concretely (the new M4 seed plumbing, computed);
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
#guard ((resolveSeeds withSeeds seedCtx [SeedSpec.instrArg 0 4]).head!).toList = [10, 20, 30, 40]

/-- `withSeeds` is in the M4 subset. -/
theorem withSeeds_M4 : M4Subset withSeeds := by decide

/-- THE SEEDS CLOSED LOOP (symbolic): for any context, the generated PDA validator agrees
    with the M1 contract — the soundness theorem instantiated at the seeds-bearing struct. -/
theorem withSeeds_sound (c : Ctx) : genValidate withSeeds c = true ↔ validates withSeeds c :=
  genValidate_sound withSeeds c withSeeds_M4
```
NOTE: `"vault".toUTF8 : ByteArray` is the UTF-8 encoding of the literal. `ByteArray.head!`/`.toList` exist; if `head!` is awkward, use `(resolveSeeds … )[1]?` and compare to `some (⟨#[10,20,30,40]⟩ : ByteArray)` via `decide`. The two `#guard`s confirm the instrArg slicing concretely. `withSeeds_M4` is `by decide` (only a `.seeds` constraint, which `isM4Constraint` accepts). `withSeeds_sound` typechecks but is not `decide`-reduced (opaque sha256) — that is the point.

- [ ] **Step 3: Full build + zero-sorry + axioms**

Run:
```bash
export PATH="$HOME/.elan/bin:$PATH"; cd /home/parth/Desktop/PARTH/Verification/lean
lake build 2>&1 | tail -5
grep -rn "sorry\|admit" VerifiedAnchor/ || echo "clean"
printf 'import VerifiedAnchor.Codegen.ExampleGenerated\n#print axioms VerifiedAnchor.Codegen.Examples.withSeeds_sound\n' > VerifiedAnchor/AxTmp.lean
lake env lean VerifiedAnchor/AxTmp.lean; rm -f VerifiedAnchor/AxTmp.lean
```
Expected: full library green; `clean`; `withSeeds_sound` axioms `[propext, Quot.sound]`.

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/VerifiedAnchor/Codegen/ExampleGenerated.lean
git commit -m "feat(lean): seeds closed-loop example; example uses M4Subset; full lake build green"
```

**✅ PART L CHECKPOINT:** `lake build` green, zero sorry; `genValidate_sound` @ M4Subset covers `.seeds`; seeds example builds. The Lean side of M4 is complete and shippable.

---

# PART R — Rust: macro + runtime tests

## Task R1: `validate` signature change (`instr_data` + `program_id`), keep M2/M3 green

**Files:** Modify `rust/verified-anchor/src/lib.rs`, `rust/verified-anchor-macros/src/lib.rs`, `rust/verified-anchor/tests/behavior.rs`, `rust/verified-anchor-program/src/lib.rs`.

Pure signature refactor (no seeds yet): thread two new params through `validate` and update all call sites. Isolating it keeps the seeds diff (R2) focused.

- [ ] **Step 1: Update the `Validate` trait**

In `rust/verified-anchor/src/lib.rs`, change the trait + add `Pubkey` import:
```rust
use solana_program::account_info::AccountInfo;
use solana_program::pubkey::Pubkey;
...
pub trait Validate {
    fn validate(
        accounts: &[AccountInfo],
        instr_data: &[u8],
        program_id: &Pubkey,
    ) -> Result<(), VAError>;
}
```

- [ ] **Step 2: Update the macro's `validate_body`**

In `rust/verified-anchor-macros/src/lib.rs`, change the generated `validate` signature and ignore the new params (seeds wiring comes in R2):
```rust
    quote! {
        fn validate(
            accounts: &[::solana_program::account_info::AccountInfo],
            instr_data: &[u8],
            program_id: &::solana_program::pubkey::Pubkey,
        ) -> ::core::result::Result<(), ::verified_anchor::VAError> {
            let _ = (instr_data, program_id);
            if accounts.len() < #n {
                return Err(::verified_anchor::VAError::NotEnoughAccounts { expected: #n, got: accounts.len() });
            }
            #(#checks)*
            Ok(())
        }
    }
```

- [ ] **Step 3: Update all `validate` call sites in tests + program**

In `rust/verified-anchor/tests/behavior.rs`, every `X::validate(&accts)` becomes `X::validate(&accts, &[], &Pubkey::new_unique())`. There are calls in: `accepts_valid`, `rejects_non_writable_vault`, `rejects_non_signer_authority`, `rejects_too_few_accounts`, `accepts_surplus_accounts`, `accepts_matching_owner`, `rejects_wrong_owner`, `has_one_accepts_match`, `has_one_rejects_mismatch`. Use a shared throwaway program id for clarity, e.g. add near the top:
```rust
fn any_pid() -> Pubkey { Pubkey::new_unique() }
```
and call `Transfer::validate(&accts, &[], &any_pid())` etc.

In `rust/verified-anchor-program/src/lib.rs`, update the two calls:
```rust
            InitOne::validate(accounts, &[], program_id).map_err(|_| ProgramError::InvalidArgument)?;
...
            CloseOne::validate(accounts, &[], program_id).map_err(|_| ProgramError::InvalidArgument)?;
```

- [ ] **Step 4: Build + test native**

Run:
```bash
cd /home/parth/Desktop/PARTH/Verification/rust
cargo test -p verified-anchor --test behavior --test lean_spec 2>&1 | tail -15
cargo build -p verified-anchor-program 2>&1 | tail -5
```
Expected: behavior + lean_spec tests pass; program crate compiles natively (lib target). NOTE: if `cargo build -p verified-anchor-program` fails because it is excluded from the workspace or only builds under SBF, skip the program native build here — R4 covers the SBF build; the macro/trait change is validated by the behavior tests.

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor/src/lib.rs rust/verified-anchor-macros/src/lib.rs rust/verified-anchor/tests/behavior.rs rust/verified-anchor-program/src/lib.rs
git commit -m "refactor(rust): validate gains instr_data + program_id params (M2/M3 still green)"
```

---

## Task R2: parse + generate `seeds`/`bump`

**Files:** Modify `rust/verified-anchor/src/lib.rs`, `rust/verified-anchor-macros/src/lib.rs`.

- [ ] **Step 1: Add `VAError` variants**

In `rust/verified-anchor/src/lib.rs`, add to the enum + `Display`:
```rust
    WrongPda { field: &'static str },
    WrongBump { field: &'static str },
```
```rust
            VAError::WrongPda { field } => write!(f, "account `{field}` is not the expected PDA"),
            VAError::WrongBump { field } => write!(f, "account `{field}` has a non-canonical bump"),
```

- [ ] **Step 2: Add seed/bump parsing to the macro**

In `rust/verified-anchor-macros/src/lib.rs`, add a `SeedElem` enum and extend `Constraint` + `Parse`:
```rust
/// One element of a `seeds = [...]` list.
enum SeedElem {
    Literal(syn::LitByteStr),   // b"vault"
    FieldKey(syn::Ident),       // field.key()
    InstrArg(usize, usize),     // arg(off, len)
}

// add to `enum Constraint`:
    Seeds(Vec<SeedElem>),
    BumpCanonical,
    BumpDeclared(u8),
```
In `impl Parse for Constraint`, add arms to the `match ident.to_string().as_str()` (after `"close"`):
```rust
            "seeds" => {
                input.parse::<Token![=]>()?;
                let arr: syn::ExprArray = input.parse()?;
                let mut elems = Vec::new();
                for e in arr.elems {
                    elems.push(parse_seed_elem(e)?);
                }
                Ok(Constraint::Seeds(elems))
            }
            "bump" => {
                if input.peek(Token![=]) {
                    input.parse::<Token![=]>()?;
                    let lit: syn::LitInt = input.parse()?;
                    Ok(Constraint::BumpDeclared(lit.base10_parse()?))
                } else {
                    Ok(Constraint::BumpCanonical)
                }
            }
```
and add the seed-element parser (top-level fn):
```rust
fn parse_seed_elem(e: Expr) -> syn::Result<SeedElem> {
    match e {
        // b"vault"
        Expr::Lit(syn::ExprLit { lit: syn::Lit::ByteStr(b), .. }) => Ok(SeedElem::Literal(b)),
        // field.key()  (method call `key` with no args on a bare ident)
        Expr::MethodCall(mc) if mc.method == "key" && mc.args.is_empty() => {
            if let Expr::Path(p) = mc.receiver.as_ref() {
                if let Some(id) = p.path.get_ident() {
                    return Ok(SeedElem::FieldKey(id.clone()));
                }
            }
            Err(syn::Error::new_spanned(mc.receiver, "seed `.key()` must be on a field name"))
        }
        // arg(off, len)
        Expr::Call(call) => {
            let is_arg = matches!(call.func.as_ref(),
                Expr::Path(p) if p.path.is_ident("arg"));
            if !is_arg {
                return Err(syn::Error::new_spanned(call.func, "unsupported seed call (expected `arg(off, len)`)"));
            }
            let mut it = call.args.iter();
            let off = lit_usize(it.next())?;
            let len = lit_usize(it.next())?;
            Ok(SeedElem::InstrArg(off, len))
        }
        other => Err(syn::Error::new_spanned(other,
            "unsupported seed (expected b\"..\", field.key(), or arg(off, len))")),
    }
}

fn lit_usize(e: Option<&Expr>) -> syn::Result<usize> {
    match e {
        Some(Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(i), .. })) => i.base10_parse(),
        _ => Err(syn::Error::new(proc_macro2::Span::call_site(),
            "arg(off, len) needs two integer literals")),
    }
}
```
NOTE: `Expr` is already imported. Add `syn::ExprArray`/`ExprLit`/`ExprCall`/`ExprMethodCall` are reachable via the `syn::` path used above; no new `use` needed beyond `Expr` (already imported). If the compiler wants explicit imports, add them to the existing `use syn::{...}`.

- [ ] **Step 3: Generate the PDA check in `validate_body`**

In `validate_body`, the per-field loop iterates each field's `constraints`. Seeds need the bump from the SAME field's constraint list, so handle seeds OUTSIDE the per-constraint `match` (which sees one constraint at a time). After the existing `for c in &spec.constraints { ... }` inner loop, add a seeds block per field:
```rust
        // seeds/bump: emit one PDA check per field that declares `seeds`.
        if let Some(Constraint::Seeds(elems)) = spec.constraints.iter()
            .find(|c| matches!(c, Constraint::Seeds(_)))
        {
            let fname = name;
            // resolve each seed element to a `&[u8]` expression
            let seed_exprs: Vec<TokenStream2> = elems.iter().map(|se| match se {
                SeedElem::Literal(b) => quote! { &#b[..] },
                SeedElem::FieldKey(id) => {
                    let fi = *index_of.get(&id.to_string())
                        .unwrap_or_else(|| panic!("seed field `{}` is not a field of this struct", id));
                    quote! { accounts[#fi].key.as_ref() }
                }
                SeedElem::InstrArg(off, len) => {
                    let end = off + len;
                    quote! { &instr_data[#off..#end] }
                }
            }).collect();
            // bump handling
            let bump_check = match spec.constraints.iter().find_map(|c| match c {
                Constraint::BumpCanonical => Some(None),
                Constraint::BumpDeclared(d) => Some(Some(*d)),
                _ => None,
            }) {
                Some(Some(d)) => quote! {
                    if __bump != #d {
                        return Err(::verified_anchor::VAError::WrongBump { field: #fname });
                    }
                },
                _ => quote! {},   // canonical (or no bump): accept any bump
            };
            checks.push(quote! {
                {
                    let __seeds: &[&[u8]] = &[ #(#seed_exprs),* ];
                    let (__pda, __bump) = ::solana_program::pubkey::Pubkey::find_program_address(__seeds, program_id);
                    if accounts[#i].key != &__pda {
                        return Err(::verified_anchor::VAError::WrongPda { field: #fname });
                    }
                    #bump_check
                }
            });
        }
```
Also add `Seeds`/`BumpCanonical`/`BumpDeclared` arms to the per-constraint `match c` inner loop so they `continue` (they are handled by the block above, not per-constraint):
```rust
                Constraint::Seeds(_) | Constraint::BumpCanonical | Constraint::BumpDeclared(_) => {
                    continue;
                }
```

- [ ] **Step 4: Extend `lean_constraint` / `lean_spec`**

In `lean_constraint`, add arms (seeds emits the full `Constraint.seeds`; bump markers emit empty — folded into seeds):
```rust
        Constraint::Seeds(elems) => {
            let seeds: Vec<String> = elems.iter().map(|se| match se {
                SeedElem::Literal(b) => {
                    let bytes: Vec<String> = b.value().iter().map(|x| x.to_string()).collect();
                    format!("SeedSpec.literal (ByteArray.mk #[{}])", bytes.join(", "))
                }
                SeedElem::FieldKey(id) => format!("SeedSpec.fieldKey \"{}\"", id),
                SeedElem::InstrArg(off, len) => format!("SeedSpec.instrArg {} {}", off, len),
            }).collect();
            // bump is folded in lean_spec_string (it needs the sibling bump constraint),
            // so emit a placeholder the folder replaces; see NOTE.
            format!("Constraint.seeds [{}] @@BUMP@@", seeds.join(", "))
        }
        Constraint::BumpCanonical | Constraint::BumpDeclared(_) => String::new(),
```
NOTE: because `lean_constraint` sees one constraint at a time but `Constraint.seeds` needs the sibling bump, do the bump substitution in `lean_spec_string`: after building each field's `cs` strings, if a field has a `Seeds`, compute its bump string (`BumpSpec.canonical` or `BumpSpec.declared <n>`) from the field's constraints and `replace("@@BUMP@@", &bump_str)` in the joined constraints. Add to `lean_spec_string`'s per-field loop:
```rust
        let bump_str = spec.constraints.iter().find_map(|c| match c {
            Constraint::BumpCanonical => Some("BumpSpec.canonical".to_string()),
            Constraint::BumpDeclared(d) => Some(format!("BumpSpec.declared {}", d)),
            _ => None,
        }).unwrap_or_else(|| "BumpSpec.canonical".to_string());
        let cs_joined = cs.join(", ").replace("@@BUMP@@", &bump_str);
```
and use `cs_joined` in the `format!` instead of `cs.join(", ")`.

- [ ] **Step 5: Build the macro**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo build -p verified-anchor-macros 2>&1 | tail -10` — expect success (warnings OK).

- [ ] **Step 6: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor/src/lib.rs rust/verified-anchor-macros/src/lib.rs
git commit -m "feat(macros): parse seeds/bump and generate find_program_address PDA check"
```

---

## Task R3: native seeds tests (real `find_program_address`) + lean_spec

**Files:** Modify `rust/verified-anchor/tests/behavior.rs`, `rust/verified-anchor/tests/lean_spec.rs`.

- [ ] **Step 1: Write the failing native seeds tests**

In `rust/verified-anchor/tests/behavior.rs`, append:
```rust
#[derive(VerifiedAccounts)]
struct PdaAccount {
    #[account(seeds = [b"vault", arg(0, 4)], bump)]
    pda: u8,
}

#[test]
fn seeds_accepts_canonical_pda() {
    let program_id = Pubkey::new_unique();
    let arg = [1u8, 2, 3, 4];
    let (pda, _bump) = Pubkey::find_program_address(&[b"vault", &arg], &program_id);
    let mut a = Acct { key: pda, owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer: false, is_writable: false };
    let accts = [a.info()];
    assert_eq!(PdaAccount::validate(&accts, &arg, &program_id), Ok(()));
}

#[test]
fn seeds_rejects_wrong_pda() {
    let program_id = Pubkey::new_unique();
    let arg = [1u8, 2, 3, 4];
    let mut a = Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer: false, is_writable: false };
    let accts = [a.info()];
    assert_eq!(PdaAccount::validate(&accts, &arg, &program_id), Err(VAError::WrongPda { field: "pda" }));
}

#[derive(VerifiedAccounts)]
struct PdaDeclaredBump {
    #[account(seeds = [b"vault"], bump = 0)]
    pda: u8,
}

#[test]
fn seeds_declared_bump_rejects_non_canonical() {
    let program_id = Pubkey::new_unique();
    let (pda, bump) = Pubkey::find_program_address(&[b"vault"], &program_id);
    // declared bump is 0; this fails unless the canonical bump happens to be 0.
    let mut a = Acct { key: pda, owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer: false, is_writable: false };
    let accts = [a.info()];
    let res = PdaDeclaredBump::validate(&accts, &[], &program_id);
    if bump == 0 {
        assert_eq!(res, Ok(()));
    } else {
        assert_eq!(res, Err(VAError::WrongBump { field: "pda" }));
    }
}
```
Run `cd rust && cargo test -p verified-anchor --test behavior 2>&1 | tail -20` — expect FAIL only if the macro is wrong; since R2 implemented it, this step verifies. (If R2/R3 are done together by one worker, run after R3 Step 1.) Expected after R2: PASS.

- [ ] **Step 2: Add the seeds `lean_spec` assertion**

In `rust/verified-anchor/tests/lean_spec.rs`, append a second test:
```rust
#[derive(VerifiedAccounts)]
struct PdaSpec {
    #[account(seeds = [b"vault", arg(0, 4)], bump)]
    pda: u8,
}

#[test]
fn lean_spec_seeds() {
    let expected = "\
{ programId := Pubkey.zero
, fields :=
  [ { name := \"pda\", ty := AccountType.uncheckedAccount, constraints := [Constraint.seeds [SeedSpec.literal (ByteArray.mk #[118, 97, 117, 108, 116]), SeedSpec.instrArg 0 4] BumpSpec.canonical] } ] }";
    assert_eq!(PdaSpec::lean_spec(), expected);
}
```
NOTE: `b"vault"` = bytes `[118, 97, 117, 108, 116]`. If `lean_spec_string`'s exact spacing differs (e.g. a trailing space or comma layout), run the test once, copy the ACTUAL output from the assertion-failure diff, and paste it as `expected` (the goal is round-trip fidelity, not guessing whitespace). Verify the pasted spec is also valid Lean by checking it parses (it mirrors `ExampleGenerated.withSeeds`).

- [ ] **Step 3: Run native suite**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test behavior --test lean_spec 2>&1 | tail -20` — expect all pass (M2/M3 + the 3 new seeds behavior tests + 2 lean_spec tests).

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor/tests/behavior.rs rust/verified-anchor/tests/lean_spec.rs
git commit -m "test(rust): native seeds accept/reject (real find_program_address) + lean_spec"
```

---

## Task R4: litesvm seeds runtime test (on-chain PDA accept/reject)

**Files:** Modify `rust/verified-anchor-program/src/lib.rs`; create `rust/verified-anchor/tests/runtime_seeds.rs`.

- [ ] **Step 1: Add a seeds instruction to the program**

In `rust/verified-anchor-program/src/lib.rs`, add a struct + an instruction arm `2`:
```rust
/// validate a PDA. Accounts: [pda]. Instruction data: [2, arg0, arg1, arg2, arg3].
#[derive(VerifiedAccounts)]
struct CheckPda {
    #[account(seeds = [b"vault", arg(0, 4)], bump)]
    pda: u8,
}
```
and in `process`, add before the `_ =>` arm:
```rust
        Some(2) => {
            // instr_data after the 1-byte tag carries the 4-byte seed arg
            CheckPda::validate(accounts, &data[1..], program_id)
                .map_err(|_| ProgramError::InvalidArgument)?;
            Ok(())
        }
```
NOTE: `&data[1..]` so `arg(0,4)` reads `data[1..5]` — the test must put the 4 seed bytes right after the tag, and derive the PDA from those same 4 bytes.

- [ ] **Step 2: Build the program to BPF (verified recipe)**

Run:
```bash
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-program && cargo-build-sbf --no-rustup-override 2>&1 | tail -8
ls -la /home/parth/Desktop/PARTH/Verification/rust/target/deploy/verified_anchor_program.so
```
Expected: a fresh `.so` is produced (warnings OK).

- [ ] **Step 3: Write the litesvm runtime test**

Create `rust/verified-anchor/tests/runtime_seeds.rs`:
```rust
use litesvm::LiteSVM;
use solana_sdk::{
    instruction::{AccountMeta, Instruction}, message::Message,
    pubkey::Pubkey, signature::{Keypair, Signer}, transaction::Transaction,
};
use std::path::PathBuf;

fn so_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.push("verified-anchor-program/target/deploy/verified_anchor_program.so");
    if !p.exists() {
        // workspace-shared target dir fallback
        let mut q = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        q.pop();
        q.push("target/deploy/verified_anchor_program.so");
        return q;
    }
    p
}

fn setup() -> (LiteSVM, Pubkey, Keypair) {
    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    svm.add_program_from_file(program_id, so_path()).expect("load .so (run cargo-build-sbf first)");
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000).unwrap();
    (svm, program_id, payer)
}

#[test]
fn seeds_good_pda_accepted_onchain() {
    let (mut svm, program_id, payer) = setup();
    let arg = [1u8, 2, 3, 4];
    let (pda, _bump) = Pubkey::find_program_address(&[b"vault", &arg], &program_id);

    let mut data = vec![2u8];          // instruction tag
    data.extend_from_slice(&arg);      // 4-byte seed arg

    let ix = Instruction::new_with_bytes(
        program_id, &data,
        vec![AccountMeta::new_readonly(pda, false)],
    );
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer], msg, svm.latest_blockhash());
    assert!(svm.send_transaction(tx).is_ok(), "correct PDA should validate on-chain");
}

#[test]
fn seeds_wrong_pda_rejected_onchain() {
    let (mut svm, program_id, payer) = setup();
    let arg = [1u8, 2, 3, 4];
    let wrong = Pubkey::new_unique();   // not the derived PDA

    let mut data = vec![2u8];
    data.extend_from_slice(&arg);

    let ix = Instruction::new_with_bytes(
        program_id, &data,
        vec![AccountMeta::new_readonly(wrong, false)],
    );
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer], msg, svm.latest_blockhash());
    assert!(svm.send_transaction(tx).is_err(), "wrong PDA must be rejected on-chain");
}
```
NOTE: the litesvm 0.6 API names (`add_program_from_file`, `airdrop`, `send_transaction`, `latest_blockhash`) match those already used in `tests/runtime_lifecycle.rs` — copy the exact import/call forms from that file if any name differs in this environment. The PDA account need not exist on-chain for a read-only key check (the program only reads `accounts[0].key`); passing it as `AccountMeta::new_readonly(pda, false)` provides the key. If litesvm requires the account to exist, `svm.set_account(pda, Account{ lamports:1, data:vec![], owner: system_program::id(), ..})` first (mirror runtime_lifecycle.rs).

- [ ] **Step 4: Run the runtime seeds tests**

Run:
```bash
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-program && cargo-build-sbf --no-rustup-override 2>&1 | tail -3
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test runtime_seeds 2>&1 | tail -20
```
Expected: both tests pass (good PDA accepted, wrong PDA rejected on-chain). If the account-existence note bites, add the `set_account` lines and rerun.

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-program/src/lib.rs rust/verified-anchor/tests/runtime_seeds.rs
git commit -m "test(runtime): litesvm seeds test - good PDA accepted, wrong PDA rejected on-chain"
```

**✅ PART R CHECKPOINT:** macro generates the PDA check; native tests cross-check the real `find_program_address`; litesvm proves on-chain accept/reject. The Rust side of M4 is complete.

---

## Task F1: bridge doc + final full-stack gates

**Files:** Modify `docs/verified-anchor-bridge.md`.

- [ ] **Step 1: Update the bridge doc**

In `docs/verified-anchor-bridge.md`, add a seeds row to the clause-by-clause table (after the has_one row):
```markdown
| `let (pda,_)=find_program_address(seeds,program_id); if accounts[i].key != pda { Err(WrongPda) }` (M4) | `genSeeds` (canonical PDA = key, bump matches) | `satisfies … (.seeds ss bump)` |
```
Then add a section:
```markdown
## PDA derivation / seeds (M4)

`seeds`/`bump` is a pure validation check, so it extends `genValidate`: `genSeeds` mirrors
`satisfies (.seeds ss bump)` and `genValidate_sound` now holds at `M4Subset` (= M3 + `.seeds`),
`[propext, Quot.sound]` only. PDA derivation runs through the concrete `findProgramAddress`
over opaque `sha256`/`isOnCurve` — **no new axioms** — so `.seeds` is decidable but does not
reduce under `decide` (the same wall as `discriminator`); the Lean example shows the crypto-free
`resolveSeeds` slicing concretely and the soundness arrow symbolically.

**Canonical-only (stricter than stock Anchor).** The verified subset derives via
`find_program_address` (the canonical bump) and a `declared` bump must equal that canonical
bump. Anchor's `bump = <stored>` (re-derive via `create_program_address` with a possibly
non-canonical bump) is intentionally outside the subset.

**Instruction-arg seeds.** A seed may be a concrete slice of the instruction data
(`SeedSpec.instrArg off len`, Lean `Ctx.instrData`; Rust `arg(off, len)` → `&instr_data[off..off+len]`).
Offsets into fixed-size leading Borsh fields are deterministic, so this adds no new trusted
assumption.

**Signature change.** The generated `validate` is now
`validate(accounts: &[AccountInfo], instr_data: &[u8], program_id: &Pubkey)` — `instr_data`
and `program_id` carry the Lean `c.instrData` and `s.programId` that `genValidate`/`genSeeds`
consume. Unused for structs without seeds/instr-arg.

**Transcription (documented + runtime-tested):** the generated PDA check matches `genSeeds`;
the macro's seed-element mapping (`arg(off,len)` → offset/length) is transcription — backed by
native tests against the real `find_program_address` and a litesvm on-chain accept/reject
(`tests/runtime_seeds.rs`), not proven across the language boundary.
```
Also update the existing "What is proven" line that says `genValidate_sound … at M2Subset/M3Subset` to mention `M4Subset` (or add a sentence: "extended to `M4Subset` (adds `.seeds`) in M4").

- [ ] **Step 2: Run ALL gates**

Run:
```bash
export PATH="$HOME/.elan/bin:$PATH"; cd /home/parth/Desktop/PARTH/Verification/lean && lake build 2>&1 | tail -3
grep -rn "sorry\|admit" VerifiedAnchor/ || echo "PASS lean zero-sorry"
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test behavior --test lean_spec 2>&1 | grep "test result"
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-program && cargo-build-sbf --no-rustup-override 2>&1 | tail -2
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test runtime_lifecycle --test runtime_seeds 2>&1 | grep "test result"
```
Expected: lake green + zero sorry; native tests pass; `.so` builds; BOTH runtime test files pass (no M3 regression).

- [ ] **Step 3: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add docs/verified-anchor-bridge.md
git commit -m "docs(bridge): M4 seeds correspondence, canonical-only boundary, validate signature change"
```

---

## Done-bar verification (after F1)

1. `lake build` green, zero `sorry`/`admit`. ✅ (L1–L5, F1)
2. `Ctx` is a structure with `instrData`; M1–M3 theorems/examples still green. ✅ (L1, F1)
3. `genConstraint_seeds_iff` + `bumpMatchesB_iff` proved; `genValidate_sound` @ `M4Subset`; `#print axioms` = `[propext, Quot.sound]`. ✅ (L3, L4)
4. seeds closed-loop example (`resolveSeeds` concrete + `withSeeds_sound` symbolic). ✅ (L5)
5. native `cargo test` green: seeds accept/reject (real `find_program_address`) + `lean_spec` shape. ✅ (R3)
6. `.so` builds; `runtime_seeds` litesvm: good PDA → Ok, wrong PDA → on-chain error. ✅ (R4)
7. bridge doc has the seeds row, canonical-only boundary, and `validate(accounts, instr_data, program_id)` signature change. ✅ (F1)
8. M1+M2+M3 still green (native + `runtime_lifecycle`). ✅ (F1)
```
