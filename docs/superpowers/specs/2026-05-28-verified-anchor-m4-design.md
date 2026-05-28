# Verified Anchor — Milestone 4 Design

*Verified PDA derivation: `seeds` and `bump`.*

Status: **approved design** (2026-05-28). Target: Milestone 4 of `verified_anchor_proposal.md`. Builds on M1 (Lean contract), M2 (proof-producing macros for mut/signer/owner), M3 (has_one + init/close), all on `master`.

---

## 1. Goal and context

M4 verifies `#[account(seeds = [...], bump)]`. PDA misuse is one of the largest single bug classes on Solana, so a machine-checked guarantee that the generated validator's PDA derivation matches the declared seeds is high-impact.

Unusually for this project, **most of the contract groundwork already exists** from M1:
- The AST seam already has `SeedSpec` (`literal`, `fieldKey`), `BumpSpec` (`declared`, `canonical`), and `Constraint.seeds` ([`Constraints/Ast.lean`](../../../lean/VerifiedAnchor/Constraints/Ast.lean)).
- The crypto model already has concrete `createProgramAddress` / `findProgramAddress` over the opaque `sha256` / `isOnCurve` ([`Solana/Crypto.lean`](../../../lean/VerifiedAnchor/Solana/Crypto.lean)).
- The M1 contract `satisfies .seeds` is **already fully specified** ([`Contract/Satisfies.lean`](../../../lean/VerifiedAnchor/Contract/Satisfies.lean)): resolve the seeds, call `findProgramAddress`, and require the derived address = the account key ∧ the bump matches.

So `.seeds` is a *check* (like has_one), not an effect (like init/close): it extends the `genValidate` framework. M4 is therefore primarily (a) wiring `.seeds` into the codegen model + soundness, (b) a scope expansion to instruction-argument seeds, and (c) the Rust macro + runtime tests.

### Decisions locked during brainstorming

1. **Bump semantics — canonical-only (keep current M1 contract semantics).** `satisfies .seeds` always derives via `findProgramAddress` (the *canonical* bump). For `BumpSpec.declared db`, the account key must equal the canonical PDA **and** the canonical bump must equal `db`. This is *stricter* than stock Anchor's `bump = <stored>` (which uses `create_program_address` with that exact bump and does not require canonicity). We deliberately keep the safe canonical-only subset: the meaning of `satisfies .seeds` is **unchanged** (the only edit to that file is the new `resolveSeeds` instr-arg case, §4.2). The stricter-than-Anchor boundary is documented in the bridge doc.
2. **Seed sources — `literal` + `fieldKey` + instruction-argument (new).**
3. **Instruction-arg model — raw-data-slice (concrete).** `Ctx` carries the raw instruction data `ByteArray`; `SeedSpec.instrArg off len` reads a concrete slice `instrData.extract off (off+len)`, mirroring how has_one reads a Pubkey from account data at a layout offset. No new trusted assumption (offsets into fixed-size leading Borsh fields are deterministic and concrete).
4. **Testing — native + litesvm (thorough).** Native `behavior.rs` (the real `Pubkey::find_program_address` cross-checks our crypto model) + `lean_spec.rs` + a litesvm `runtime_seeds.rs` (good PDA accepted, wrong/tampered PDA rejected on-chain).

### `Ctx` representation (the one architectural choice)

`Ctx` is currently `abbrev Ctx := List AccountInfo`. To carry instruction data we make it a **structure** (Approach A, chosen over threading an extra parameter through every contract/codegen function and soundness theorem signature):

```lean
structure Ctx where
  accounts  : List AccountInfo
  instrData : ByteArray := ByteArray.empty
```

Rationale: instruction data genuinely *is* part of the runtime context, and future instruction-data-derived constraints reuse it. The threading alternative (Approach B) is strictly more invasive — because `.seeds` lives inside a field's constraint list, the parameter would have to plumb through `satisfies`, `validates`, `genValidate`, and **every soundness theorem signature** (`genValidate_sound`, `validates_iff_validatesBool`, …). Rejected.

---

## 2. Repository layout (M4 additions)

```
lean/VerifiedAnchor/
├── Constraints/Context.lean          (MODIFY) Ctx → structure {accounts, instrData};
│                                               Ctx.lookup/atField/WellFormed via .accounts;
│                                               add Ctx.ofAccounts smart constructor
├── Constraints/Ast.lean              (MODIFY) SeedSpec += instrArg (off len : Nat)
├── Contract/Satisfies.lean           (MODIFY) resolveSeeds += instrArg case (c.instrData slice).
│                                               satisfies .seeds logic UNCHANGED.
├── Codegen/Generated.lean            (MODIFY) bumpMatchesB; genSeeds; genConstraint .seeds case;
│                                               genValidate uses c.accounts.length
├── Codegen/Soundness.lean            (MODIFY) bumpMatchesB_iff; genConstraint_seeds_iff;
│                                               isM4Constraint / M4Subset; genValidate_sound @ M4Subset
├── Codegen/Lifecycle.lean            (MODIFY) Ctx.update preserves instrData;
│                                               Ctx.getElem?_update re-proved over .accounts
├── Codegen/ExampleGenerated.lean     (MODIFY) Ctx literals via Ctx.ofAccounts; a seeds example
└── Examples/Withdraw.lean            (MODIFY) Ctx literals via Ctx.ofAccounts (mechanical)

rust/
├── verified-anchor-macros/src/lib.rs (MODIFY) parse seeds/bump; emit PDA check + lean_spec
├── verified-anchor/
│   ├── src/lib.rs                     (MODIFY) VAError += WrongPda/WrongBump; validate gains instr_data
│   └── tests/
│       ├── behavior.rs               (MODIFY) seeds accept/reject (native, real find_program_address)
│       ├── lean_spec.rs              (MODIFY) emitted Constraint.seeds shape
│       └── runtime_seeds.rs          (NEW) litesvm: good PDA -> Ok, wrong PDA -> Err on-chain
└── verified-anchor-program/src/lib.rs (MODIFY) add a seeds-validated instruction (incl. instr-arg seed)

docs/
├── superpowers/specs/2026-05-28-verified-anchor-m4-design.md   (this file)
└── verified-anchor-bridge.md         (MODIFY) seeds row; canonical-only boundary; instr_data sig change
```

---

## 3. Part 1 — `Ctx` becomes a structure

```lean
structure Ctx where
  accounts  : List AccountInfo
  instrData : ByteArray := ByteArray.empty

/-- Build a Ctx from just accounts (instrData empty). Keeps existing examples terse. -/
def Ctx.ofAccounts (l : List AccountInfo) : Ctx := { accounts := l }
```

Mechanical reroutes (account access goes through `.accounts`):
- `Ctx.lookup`, `Ctx.atField`, `WellFormed` ([`Constraints/Context.lean`](../../../lean/VerifiedAnchor/Constraints/Context.lean)): `c[idx]?` → `c.accounts[idx]?`, `c.length` → `c.accounts.length`.
- `genValidate` ([`Codegen/Generated.lean:35`](../../../lean/VerifiedAnchor/Codegen/Generated.lean#L35)): `c.length` → `c.accounts.length`.
- `Ctx.update` ([`Codegen/Lifecycle.lean`](../../../lean/VerifiedAnchor/Codegen/Lifecycle.lean)): update `.accounts`, **preserve `.instrData`**; `Ctx.getElem?_update` re-proved over `.accounts` (mechanical).
- Example `Ctx` literals (~10 sites in `ExampleGenerated.lean`, `Withdraw.lean`): `[a, b]` → `Ctx.ofAccounts [a, b]`.

All theorems are parameterized over `c`, so only the defs that destructure the list change; the proof structure is unaffected beyond the `.accounts`/`.ofAccounts` renames.

---

## 4. Part 2 — AST + contract

### 4.1 `SeedSpec.instrArg`
```lean
inductive SeedSpec where
  | literal  (bytes : ByteArray)        -- e.g. b"vault"
  | fieldKey (field : String)           -- another account's key bytes
  | instrArg (off : Nat) (len : Nat)    -- NEW: a concrete slice of the instruction data
  deriving Inhabited
```

### 4.2 `resolveSeeds` gains the instr-arg case
```lean
def resolveSeeds (s : AccountsStruct) (c : Ctx) : List SeedSpec → List ByteArray
  | [] => []
  | .literal bytes :: rest => bytes :: resolveSeeds s c rest
  | .fieldKey name :: rest => (… c.accounts lookup as today …) :: resolveSeeds s c rest
  | .instrArg off len :: rest => c.instrData.extract off (off + len) :: resolveSeeds s c rest
```
`satisfies .seeds` ([`Contract/Satisfies.lean:57-60`](../../../lean/VerifiedAnchor/Contract/Satisfies.lean#L57-L60)) is **unchanged** — it already calls `findProgramAddress (resolveSeeds …)` and checks `pr.1 = a.key ∧ bumpMatches b pr.2` (canonical-only). The only change reaching it is that `resolveSeeds` now also resolves instr-arg seeds, and `Ctx.atField`/`Ctx.lookup` read `.accounts`.

---

## 5. Part 3 — codegen model + soundness

### 5.1 `genSeeds` (Bool mirror of `satisfies .seeds`)
```lean
def bumpMatchesB : BumpSpec → UInt8 → Bool
  | .declared db, actual => actual == db
  | .canonical,  _       => true

def genSeeds (s : AccountsStruct) (c : Ctx) (idx : Nat)
    (ss : List SeedSpec) (b : BumpSpec) : Bool :=
  (Ctx.atField s c idx).allB (fun a =>
    (findProgramAddress (resolveSeeds s c ss) s.programId).allB (fun pr =>
      decide (pr.1 = a.key) && bumpMatchesB b pr.2))
```
Wire into `genConstraint` ([`Codegen/Generated.lean:29`](../../../lean/VerifiedAnchor/Codegen/Generated.lean#L29)), replacing the `_ => false` catch-all with `| .seeds ss b => genSeeds s c idx ss b` (init/close remain `false` — they are effects handled by the Hoare layer, not `genValidate`).

### 5.2 Soundness ([`Codegen/Soundness.lean`](../../../lean/VerifiedAnchor/Codegen/Soundness.lean))
```lean
theorem bumpMatchesB_iff (b : BumpSpec) (x : UInt8) :
    bumpMatchesB b x = true ↔ bumpMatches b x          -- by cases on b

theorem genConstraint_seeds_iff (s c idx f ss b) :
    genConstraint s c idx f (Constraint.seeds ss b) = true
      ↔ satisfies s c idx f (Constraint.seeds ss b)     -- allB_iff + decide + bumpMatchesB_iff

def isM4Constraint : Constraint → Bool
  | .signer | .mut | .owner _ | .hasOne _ | .discriminator _ | .seeds _ _ => true
  | _ => false

def M4Subset (s : AccountsStruct) : Prop :=
  ∀ f ∈ s.fields, ∀ k ∈ (f.ty.impliedConstraints ++ f.constraints), isM4Constraint k = true

theorem genValidate_sound (s c) (h : M4Subset s) :
    genValidate s c = true ↔ validates s c
```
The existing `genValidate_sound` proof body is generic over the per-field subset hypothesis, so it carries over once `genConstraint_iff_satisfies` gains the `.seeds` case (dispatching to `genConstraint_seeds_iff`). `#print axioms genValidate_sound` must remain `[propext, Quot.sound]`.

**Opaque-`sha256` wall.** `findProgramAddress` calls `sha256`, so `.seeds` is *decidable but does not reduce* under `decide`/`#eval` — exactly like `discriminator`. Therefore the Lean example demonstrates `.seeds` symbolically (the `genValidate_sound` instantiation, and `genConstraint_seeds_iff` on a concrete struct), **not** by computing the check to `true`. The empirical reduction is provided by the native + litesvm tests against the real `find_program_address`.

---

## 6. Part 4 — Rust macro + runtime tests

### 6.1 Codegen (`verified-anchor-macros`)
- Parse `#[account(seeds = [<expr>, ...], bump)]` and `bump = <expr>`. Supported seed exprs map to the three `SeedSpec` variants: byte literals (`b"..."`), `field.key().as_ref()` (`fieldKey`), and instruction-data slices (`instrArg`). Unsupported seed exprs → a clear compile error.
- Emit the PDA check inside `validate()`: assemble the seed slice, call `Pubkey::find_program_address(&seeds, program_id)` to get `(pda, canonical_bump)`, then `if accounts[i].key != &pda { return Err(VAError::WrongPda { .. }) }`; for `bump = db`, additionally `if canonical_bump != db { return Err(VAError::WrongBump { .. }) }`.
- `lean_spec()` emits `Constraint.seeds [<SeedSpec>...] <BumpSpec>`, including `SeedSpec.instrArg off len` for instruction-arg seeds.
- `VAError` gains `WrongPda` and `WrongBump`.

### 6.2 `validate` signature change (mirrors `Ctx.instrData`)
The generated `validate` gains an instruction-data parameter:
```rust
fn validate(accounts: &[AccountInfo], instr_data: &[u8]) -> Result<(), VAError>
```
Added **uniformly** (every generated `validate`), even when no instr-arg seeds appear, for a single consistent trait signature; unused `instr_data` is simply ignored in those cases. The `Validate` trait and all existing call sites/tests update to pass `instr_data` (empty slice where irrelevant).

### 6.3 Runtime tests
- `behavior.rs` (native): construct accounts whose key is the real `find_program_address` of declared seeds → `validate` returns `Ok`; a wrong key and a wrong declared bump → the respective `Err`. (This exercises the *real* Solana `find_program_address`, cross-checking the Lean crypto model without BPF.)
- `lean_spec.rs`: assert the emitted `Constraint.seeds` literal shape (incl. `instrArg`).
- `verified-anchor-program`: add a seeds-validated instruction that derives a PDA from a literal seed + an instruction-argument seed and runs the generated check.
- `runtime_seeds.rs` (litesvm, behind the existing litesvm + vendored-openssl dev-deps): build the `.so` (§ recipe in HANDOVER.md/M3 design), send a tx with the correctly-derived PDA → success; send one with a tampered/wrong PDA → on-chain custom error.

---

## 7. Part 5 — trust boundary (bridge doc addendum)

- **Proven:** `genValidate ≡ validates` extended to `.seeds` (`M4Subset`) — the modeled generated PDA check agrees with the M1 contract, canonical-only, parameterized over the declared seeds/bump. `[propext, Quot.sound]` only.
- **Modeled (already, not new):** `findProgramAddress` is concrete over opaque `sha256`/`isOnCurve`. No new axioms in M4.
- **Stricter than stock Anchor (documented boundary):** the verified subset accepts only canonical PDAs; a `declared` bump must equal the canonical bump. Anchor's `bump = <stored>` (cheap re-derivation via `create_program_address` with a possibly non-canonical bump) is intentionally outside the subset.
- **Transcription (documented + runtime-tested):** the generated Rust `validate` PDA check matches `genSeeds`; the macro's mapping from typed seed expressions to `SeedSpec` (esp. typed instruction args → byte offset/length) is transcription — backed by native real-`find_program_address` tests + litesvm execution, not proven across the language boundary.
- **Out of scope:** rustc/LLVM/sBPF codegen fidelity; the runtime's own `find_program_address` correctness (cross-checked empirically, not proven); non-canonical/stored-bump validation; Borsh deserialization of typed args (we model the resulting bytes as a concrete slice).

---

## 8. Scope / non-goals (M4)

**In.** `Ctx` → structure with `instrData`; `SeedSpec.instrArg`; `resolveSeeds` instr-arg case; `genSeeds` + `genConstraint` wiring; `bumpMatchesB_iff` / `genConstraint_seeds_iff`; `isM4Constraint` / `M4Subset`; `genValidate_sound` @ `M4Subset`; Rust seeds/bump codegen + `validate` instr_data param + `WrongPda`/`WrongBump`; native + `lean_spec` + litesvm seeds tests; a seeds closed-loop example; the bridge addendum.

**Out.** Non-canonical / stored-bump semantics; account-data-field seeds (`user.authority.as_ref()`); seeds derived from arbitrary expressions; full `anchor-lang` API + cargo plugin (M5); empirical historical-exploit study (M6); proving the runtime's `find_program_address`.

---

## 9. Done-bar for Milestone 4

1. `lake build` green, zero `sorry`/`admit`, including the edited `Constraints`/`Contract`/`Codegen` modules.
2. `Ctx` is a structure with `instrData`; all M1–M3 defs/examples build via `.accounts`/`Ctx.ofAccounts`; M1/M2/M3 theorems still green (no regressions).
3. `genConstraint_seeds_iff` and `bumpMatchesB_iff` proved; `genValidate_sound` re-proved at `M4Subset`; all with `#print axioms` = `[propext, Quot.sound]` (no `sorryAx`).
4. A seeds closed-loop example: `genConstraint_seeds_iff` / `genValidate_sound` instantiated on a concrete struct (symbolic, given the `sha256` wall).
5. `cargo build`/`cargo test -p verified-anchor` green: native seeds accept/reject (real `find_program_address`) + `lean_spec` shape.
6. `verified-anchor-program` builds to `.so`; `runtime_seeds.rs` (litesvm): correct PDA → Ok, wrong/tampered PDA → on-chain error.
7. `docs/verified-anchor-bridge.md` updated with the seeds correspondence row, the canonical-only boundary, and the `instr_data` signature change.
```
