# Verified Anchor — the Rust↔Lean bridge (through Milestone 3)

How the generated Rust validator relates to the machine-checked Lean proof, and exactly
what is and isn't proven.

## Clause-by-clause correspondence

| Generated Rust (`validate`) | Lean model (`genConstraint`) | Discharges M1 contract case |
|---|---|---|
| `if !accounts[i].is_signer { Err(MissingSigner) }` | `genSigner a := a.isSigner` | `satisfies … .signer` |
| `if !accounts[i].is_writable { Err(NotWritable) }` | `genMut a := a.isWritable` | `satisfies … .mut` |
| `if accounts[i].owner != &expected { Err(WrongOwner) }` | `genOwner e a := decide (a.owner = e)` | `satisfies … (.owner e)` |
| `if accounts.len() < n { Err(NotEnoughAccounts) }` | `decide (c.length = s.fields.length)` | `WellFormed` |
| `if data[8..40] != target.key { Err(WrongHasOne) }` (M3) | `genHasOne` (read 32B @ offset 8, compare to looked-up key) | `satisfies … (.hasOne field)` |
| `let (pda,_)=find_program_address(seeds,program_id); if accounts[i].key != pda { Err(WrongPda) }` (M4) | `genSeeds` (canonical PDA = key, bump matches) | `satisfies … (.seeds ss bump)` |
| `invoke(create_account(...)) + write disc` (M3) | `applyInit` (state transformer) | `init_establishes_post`: post-state has owner set + ≥ `space+8` bytes |
| `dest.lamports += t.lamports; t.lamports = 0; mark` (M3) | `applyClose` (state transformer) | `close_establishes_post`: post-state has lamports 0 + closed marker |

The generated `validate` has signature `fn validate(accounts: &[AccountInfo], instr_data: &[u8], program_id: &Pubkey) -> Result<(), VAError>`
(an associated method of the `Validate` trait — no `&self`; the struct is a compile-time
spec carrier and validation is positional over the runtime account slice, index = field
declaration order, matching the Lean `Ctx`). Per field it checks every constraint in order
and short-circuits on the first failure; `genFieldValidate` folds `genConstraint` with `&&`
over `impliedConstraints ++ constraints`. `genValidate` conjoins well-formedness with all
fields.

## What is proven

`theorem genValidate_sound (s c) (h : M4Subset s) : genValidate s c = true ↔ validates s c`
— the Lean model of the generated validator agrees with the Milestone-1 contract for every
struct in the **M4 subset**. Proved once, parameterized over the user's annotation.
`#print axioms` reports `[propext, Quot.sound]` only — no `sorryAx`, no `Classical.choice`,
no `native_decide`. Per-constraint lemmas (`genConstraint_{signer,mut,owner,discriminator,hasOne,seeds}_iff`,
plus `bumpMatchesB_iff`) connect each `gen*` to the corresponding M1 `satisfies` case.

The subset grew with each milestone (the theorem was re-proved at each, never weakened):
- **M2** (`mut`/`signer`/`owner`): only unchecked-style account types; excluded the typed
  `Account<T>` because it implies a `discriminator` constraint.
- **M3** adds `hasOne` + `discriminator`, so `M3Subset` admits the typed `Account<T>`.
- **M4** adds `.seeds`, so `M4Subset` (= M3 + `.seeds`) is the current predicate. `M4Subset s`:
  every field's `(impliedConstraints ++ constraints)` is one of {signer, mut, owner, hasOne,
  discriminator, seeds}.

## What is transcription (documented + tested, not proven)

The Rust `validate` body is a clause-by-clause transcription of `genValidate` per the table
above. This correspondence is NOT machine-checked across the language boundary; it is backed
by shared accept/reject test vectors run in both `rust/verified-anchor/tests/behavior.rs` and
the Lean `#guard`s in `Codegen/ExampleGenerated.lean`.

**Known permissiveness difference (account count).** The generated Rust guards with
`accounts.len() < n`, so it *accepts surplus accounts* (it only inspects the declared prefix
`0..n`). The Lean model `genValidate` — and the M1 contract `WellFormed` it is proven equal
to — instead require an *exact* count (`c.length = s.fields.length`). So on a slice with more
accounts than the struct declares, Rust returns `Ok` while the contract/model would reject.
This is a transcription difference, not a soundness defect: `genValidate_sound` relates the
model to the contract (both exact), and the Rust is strictly *more* permissive only on the
surplus dimension. The Rust behavior is pinned by the `accepts_surplus_accounts` test; a
future milestone can tighten the generated guard to `!= n` if exact-count parity is desired.

## Lifecycle / Hoare framework (M3)

`has_one` is a pure validation check, so it extends the `genValidate` framework directly
(generalized relational `genConstraint`; `genValidate_sound` now holds at `M3Subset`, which
admits typed `Account<T>` — bringing the implied `discriminator`, opaque under `sha256`, so
`genValidate` stays symbolic for typed structs while the *proof* still holds).

`init`/`close` are **effects**, not checks, so they get a separate Hoare-style treatment in
`Codegen/Lifecycle.lean`: `applyInit`/`applyClose : Ctx → Option Ctx` model the state
transition, and `init_establishes_post`/`close_establishes_post` prove the post-state has the
**core** M1 post-condition properties — for `init`, the target's owner is set and its data is
≥ `space+8` bytes; for `close`, the target's lamports are 0 and its data carries the
closed-account marker (`[propext, Quot.sound]` only). The remaining clauses bundled into the
M1 `satisfies (.init/.close)` proposition (payer is signer+writable; the close destination
resolves) are *guarded preconditions* of the transformer that it preserves rather than
post-effects; proving the literal `satisfies` proposition as a corollary is a tracked
follow-up (`docs/superpowers/m3-followups.md`). The full `satisfies (.close …)` was verified
to hold on a concrete post-state during review. The generated
effectful Rust (`execute_lifecycle`) is **executed under litesvm** (`tests/runtime_lifecycle.rs`):
`init` is asserted to create a program-owned, funded, 8-byte account; `close` to move all
lamports to the destination and drain the target — i.e. the model is empirically
cross-checked against a real Solana VM, not just documented.

**New trusted modeling assumption (M3):** that `solana_program::system_instruction::create_account`'s
on-chain effect on account state matches `applyInit` (its documented effect — owner assigned,
space allocated, lamports moved). We model the *effect*, not the CPI dispatch. The litesvm
runtime tests reduce the risk that this model diverges from reality.

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

## Developer surface (M7a)

The Rust→Lean proof chain is unchanged: the macro emits an `impl Validate` whose body is the
same per-constraint check sequence (signer/mut/owner/has_one/seeds/discriminator) that
`genValidate` models in Lean, with `M4Subset s → (genValidate s c = true ↔ validates s c)`
proved generically. M7a adds an `Accounts<'info>` trait alongside `Validate`: its
`try_accounts` calls `Self::validate` first (the proven gate), then Borsh-deserialises each
`Account<'info, T>` field's data into the typed struct. Borsh deserialisation is outside the
proven surface (a transcription concern, like the M3 CPI-effect modelling) — a `BorshFailed`
error is honest runtime feedback, not a verification hole. The `lean_spec` emission now uses
the real type name from `Account<'info, T>` (closing the M3 "Vault hardcode" follow-up). No
Lean source changes; M1–M5 headline theorems' `#print axioms` unchanged.

## What is out of scope

rustc/LLVM/sBPF code generation fidelity — that the compiled binary faithfully executes the
Rust source. This is the standard boundary of source-level verification (cf. CompCert), and
is not addressed by Verified Anchor at any milestone.

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
