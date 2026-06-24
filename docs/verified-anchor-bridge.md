# The Rust↔Lean bridge

How the generated Rust validator relates to the machine-checked Lean proof, and exactly
what is and is not proven.

## Clause-by-clause correspondence

| Generated Rust (`validate`) | Lean model (`genConstraint`) | Discharges contract case |
|---|---|---|
| `if !accounts[i].is_signer { Err(MissingSigner) }` | `genSigner a := a.isSigner` | `satisfies … .signer` |
| `if !accounts[i].is_writable { Err(NotWritable) }` | `genMut a := a.isWritable` | `satisfies … .mut` |
| `if accounts[i].owner != &expected { Err(WrongOwner) }` | `genOwner e a := decide (a.owner = e)` | `satisfies … (.owner e)` |
| `if !accounts[i].executable { Err(WrongOwner) }` (`Program<P>` base check) | `genConstraint … .executable := a.executable` | `satisfies … .executable` |
| `if accounts[i].key != &P::ID { Err(WrongOwner) }` (`Program<P>` base check) | `genConstraint … (.address e) := decide (a.key = e)` | `satisfies … (.address e)` |
| `if accounts.len() < n { Err(NotEnoughAccounts) }` | `decide (c.length = s.fields.length)` | `WellFormed` |
| `if data[8..40] != target.key { Err(WrongHasOne) }` | `genHasOne` (read 32 bytes at offset 8, compare to the looked-up key) | `satisfies … (.hasOne field)` |
| `let (pda,_) = find_program_address(seeds, program_id); if accounts[i].key != pda { Err(WrongPda) }` | `genSeeds` (canonical PDA equals the account key; bump matches) | `satisfies … (.seeds ss bump)` |
| `let pda = create_program_address(seeds_with_stored_bump, pid); if accounts[i].key != pda { Err(WrongPda) }` | `genSeeds` with `BumpSpec.stored` (re-derive via `createProgramAddress` at the bump byte from `instr_data[off]`, no canonical requirement) | `satisfies … (.seeds ss (BumpSpec.stored off))` |
| `let pid = <expr>; let (pda,_) = find_program_address(seeds, pid); if key != pda { Err(WrongPda) }` | `genSeeds` with a `program : some pid` field on `Constraint.seeds` (derive PDA against a foreign program id) | `satisfies … (.seeds ss bump)` with the resolved `pid` |
| `if accounts[i].key != &expected { Err(WrongAddress) }` | `genConstraint … (.address e) := decide (a.key = e)` | `satisfies … (.address e)` |
| `if !accounts[i].executable { Err(NotExecutable) }` | `genConstraint … .executable := a.executable` | `satisfies … .executable` |
| iterate over all `(i,j)` pairs of `mut` fields; `if accounts[i].key == accounts[j].key { Err(DuplicateAccount) }` (unless `allow_duplicate` opts out per pair) | `distinctMutKeys` folded into `genValidate` (struct-level check, all pairwise-distinct-key obligations) | `satisfies … (struct-level distinct-mut-keys)` |
| `let min = rent_exempt_minimum(accounts[i].data.len()); if accounts[i].lamports < min { Err(NotRentExempt) }` | `genConstraint … .rentExempt` via opaque `rentExemptMinimum : Nat → Lamports` (see Honesty boundary below) | `satisfies … .rentExempt` |
| `invoke(create_account(...)) + write disc` | `applyInit` (state transformer) | `init_establishes_post`: post-state has owner set and at least `space + 8` bytes |
| `dest.lamports += t.lamports; t.lamports = 0; mark` | `applyClose` (state transformer) | `close_establishes_post`: post-state has lamports zero and a closed-account marker |

The generated `validate` has signature
`fn validate(accounts: &[AccountInfo], instr_data: &[u8], program_id: &Pubkey) -> Result<(), VAError>`
(an associated method of the `Validate` trait; no `&self`). The derived struct is a compile-time
spec carrier. Validation is positional over the runtime account slice — field index equals slice
index, matching the Lean `Ctx`. Per field the macro emits the declared constraints in order and
short-circuits on the first failure. The Lean side mirrors this: `genFieldValidate` folds
`genConstraint` with `&&` over the field's implied and declared constraints; `genValidate`
conjoins well-formedness with all fields.

## What is proven

```
theorem genValidate_sound
    (s : AccountsStruct) (c : Ctx) (h : M4Subset s) :
  genValidate s c = true ↔ validates s c
```

The Lean model of the generated validator agrees with the declarative contract for every
struct in the supported subset (named `M4Subset` in Lean). The theorem is proved once,
parameterised over the user's struct. `#print axioms` reports `[propext, Quot.sound]` only —
no `sorryAx`, no `Classical.choice`, no `native_decide`. Per-constraint lemmas
(`genConstraint_{signer,mut,owner,discriminator,hasOne,seeds,executable,address}_iff`, plus
`bumpMatchesB_iff`) connect each `gen*` to the corresponding `satisfies` case in the contract.

`M4Subset s` (now covering all M8 features) characterises structs in scope: every field's
combined implied-and-declared constraint list contains only
`{signer, mut, owner, hasOne, discriminator, seeds, executable, address, rentExempt}` and
struct-level `distinctMutKeys` is discharged.

**Wrapper base checks are modelled, not just transcribed.** The macro's `wrapper_implied`
emits base checks for two typed wrappers beyond what the explicit `#[account(...)]`
annotations request: a `SystemAccount<'info>` is checked to be owned by the System Program,
and a `Program<'info, P>` is checked to be `executable` with `key == P::ID`. These are
carried in the Lean contract through `AccountType.impliedConstraints` (`systemAccount`
implies `owner`; `program` implies `executable` + `address`), so `genValidate_sound` covers
them — the generated validator does no validation work outside the proven contract. The
modelled pubkeys (the System-Program id, `P::ID`) are schematic placeholders (`Pubkey.zero`),
exactly like the explicit `owner = EXPR` placeholder; the theorem is universally quantified
over the pubkey value. `lean/VerifiedAnchor/Codegen/ExampleGenerated.lean` carries closed-loop
`#guard`s (`sysAcct_*`, `prog_*`) demonstrating accept/reject of the modelled checks.

## What is transcription

The Rust `validate` body is a clause-by-clause transcription of `genValidate` per the table
above. This correspondence is not machine-checked across the language boundary; it is backed
by shared accept/reject test vectors run in both `rust/verified-anchor/tests/behavior.rs`
and the Lean `#guard`s in `lean/VerifiedAnchor/Codegen/ExampleGenerated.lean`.

**Known permissiveness difference (account count).** The generated Rust guards with
`accounts.len() < n`, so it accepts surplus accounts (only the declared prefix `0..n` is
inspected). The Lean model `genValidate` and the contract's `WellFormed` predicate require
an exact count (`c.length = s.fields.length`). On a slice with more accounts than the struct
declares, Rust returns `Ok` while the contract and model would reject. This is a transcription
difference, not a soundness defect: `genValidate_sound` relates the model to the contract
(both exact), and the Rust is strictly *more* permissive only along the surplus dimension.
The Rust behaviour is pinned by the `accepts_surplus_accounts` test. A future revision can
tighten the generated guard to `!= n` if exact-count parity is desired.

## Lifecycle: `init` and `close`

`has_one` is a pure validation check and extends the `genValidate` framework directly through
relational `genConstraint`. With it, `genValidate_sound` admits typed `Account<T>` (which
implies a discriminator). The discriminator constraint is opaque under `sha256`, so
`genValidate` stays symbolic for typed structs while the *proof* still holds.

`init` and `close` are effects, not checks, and receive a separate Hoare-style treatment
under `lean/VerifiedAnchor/Codegen/Lifecycle.lean`. `applyInit` and `applyClose` are state
transformers `Ctx → Option Ctx`. The theorems `init_establishes_post` and
`close_establishes_post` show the post-state satisfies the core contract post-conditions:

* `init` — the target's owner is set, and its data is at least `space + 8` bytes.
* `close` — the target's lamports are zero and its data carries the closed-account marker.

Both theorems' axioms are `[propext, Quot.sound]`. Remaining clauses bundled into the
declarative `satisfies (.init/.close)` proposition (payer is signer and writable; the close
destination resolves) are guarded preconditions of the transformer that it preserves rather
than post-effects; proving the literal `satisfies` proposition as a corollary is a tracked
follow-up. The full `satisfies (.close …)` was verified to hold on a concrete post-state
during review.

The generated effectful Rust (`execute_lifecycle`) is executed under litesvm
(`rust/verified-anchor/tests/runtime_lifecycle.rs`): `init` is asserted to create a
program-owned, funded, 8-byte account; `close` is asserted to move all lamports to the
destination and drain the target. The model is empirically cross-checked against a real
Solana VM.

**Trusted modelling assumption.** That
`solana_program::system_instruction::create_account`'s on-chain effect on account state
matches `applyInit` (its documented effect — owner assigned, space allocated, lamports
moved). The library models the effect, not the CPI dispatch. The litesvm runtime tests
reduce the risk that the model diverges from reality.

## PDA derivation

`seeds` and `bump` are pure validation checks. `genSeeds` mirrors `satisfies (.seeds ss bump)`,
and `genValidate_sound` holds at `M4Subset`. PDA derivation runs through the concrete
`findProgramAddress` over opaque `sha256` and `isOnCurve`. **No new axioms are introduced.**
The `.seeds` clause is decidable but does not reduce under `decide` (the same wall as
`discriminator`); the Lean example shows the crypto-free `resolveSeeds` slicing concretely
and the soundness arrow symbolically.

**Canonical-only (default).** The verified subset derives via `find_program_address` (the
canonical bump). A declared `bump = n` must equal the canonical bump. This is the safe
default. Stock Anchor's `bump = <stored>` form — re-derive via `create_program_address` with
a possibly non-canonical bump — is an explicit opt-in: write `bump = arg(off)` to read the
bump byte from instruction-data offset `off`. This uses `createProgramAddress` (`BumpSpec.stored`
on the Lean side) and is modelled and proven; the canonical requirement is intentionally
absent. The opt-in is deliberate: stored bumps are less safe than canonical bumps, so they are
never the silent default.

**`seeds::program`.** A PDA may be derived against a foreign program id by writing
`seeds::program = <expr>`. The Lean model adds a `program : Option Pubkey` field to
`Constraint.seeds`; `none` means own program id (the default), `some pid` means the foreign
id. `lean_spec` emits the schematic `(some Pubkey.zero)` placeholder (∀-over-pubkey, exactly
like `owner` and `address`). The proof holds uniformly.

**Instruction-arg seeds.** A seed may be a concrete slice of the instruction data
(`SeedSpec.instrArg off len` on the Lean side; `arg(off, len)` on the Rust side). Offsets into
fixed-size leading Borsh fields are deterministic, so this adds no new trusted assumption. The
generated slice clamps both bounds to `instr_data.len()`
(`&instr_data[off.min(len)..(off+len).min(len)]`), which both prevents an out-of-bounds panic
on a short `instr_data` and mirrors the Lean model's `ByteArray.extract off (off+len)` (which
likewise clamps); a too-short `instr_data` therefore yields a clean `WrongPda` rejection, not a
panic.

**Transcription.** The generated PDA check matches `genSeeds`. The macro's seed-element
mapping (`arg(off, len)` to offset and length) is transcription, backed by native tests
against the real `find_program_address` and a litesvm on-chain accept/reject
(`rust/verified-anchor/tests/runtime_seeds.rs`), not proved across the language boundary.

## Distinct-mutable-account checking (safety value-add)

`genValidate` now folds a struct-level `distinctMutKeys` predicate that checks every pair of
`mut`-annotated accounts has a distinct key. This is proven correct (`distinctMutKeysB_iff`)
and included in `genValidate_sound` at `M4Subset`. Stock Anchor does not perform this check
automatically; verified-anchor's default is therefore **stricter than stock Anchor** here —
the "duplicate mutable accounts" bug class is closed by construction.

Per-pair opt-out: add `#[account(allow_duplicate = <field>)]` to suppress the check for one
specific pair. This is the explicit, user-visible escape hatch. The opt-out is never silent.
`VAError::DuplicateAccount` (code 14) is the rejection code.

## Honesty boundary: `rentExemptMinimum`

`rent_exempt = enforce` checks that an account holds at least the runtime rent-exempt
minimum for its data size. The runtime function `Rent::is_exempt` is not modelled
axiomatically — that would require trusting an external spec. Instead, the Lean model
introduces an opaque function `rentExemptMinimum : Nat → Lamports` (a new honesty wall,
analogous to the existing `sha256`/`isOnCurve` walls). The theorem `genValidate_sound` holds
over this opaque constant; the proof does not depend on its numeric value.

The correspondence between `rentExemptMinimum` and Solana's actual `Rent::is_exempt` is
verified **empirically**: the litesvm runtime tests exercise `rent_exempt = enforce` against
a real Solana VM and confirm accept/reject behaviour matches. This is not a proof gap — it is
an honest statement of what is and is not provable across the Rust/Solana boundary. The
`rent_exempt = skip` annotation omits the check entirely (emits no constraint), as an explicit
opt-out consistent with the safe-by-default tenet.

## Developer surface

The Rust-to-Lean proof chain is unchanged from the macro's introduction. The derive emits an
`impl Validate` whose body is the per-constraint check sequence
(`signer` / `mut` / `owner` / `has_one` / `seeds` / `discriminator`) that `genValidate`
models in Lean, with `M4Subset s → (genValidate s c = true ↔ validates s c)` proved
generically.

Alongside `Validate` the derive also emits `impl<'info> Accounts<'info>`, whose `try_accounts`
calls `Self::validate` first (the proven gate), then Borsh-deserialises each
`Account<'info, T>` field's data into the typed struct. Borsh deserialisation is outside the
proven surface (a transcription concern, like the CPI-effect modelling for `init`/`close`).
A `BorshFailed` error is honest runtime feedback, not a verification hole.

## What is out of scope

The fidelity of rustc, LLVM, and the sBPF code generator — i.e. that the compiled binary
faithfully executes the Rust source. This is the standard boundary of source-level
verification (see CompCert for context) and is not addressed by this project.

## Automated checking

The Rust-to-Lean flow is mechanical. `#[derive(VerifiedAccounts)]` auto-registers each struct
through the `inventory` crate; `verified_anchor::emit_specs!()` writes each struct's
`lean_spec()`; and `cargo verified-anchor check` generates a `check.lean` file containing
per-struct obligations and runs `lake env lean`. Each obligation is a single `decide`:

* validation structs → `M4Subset spec` (the generic `genValidate_sound` applies);
* lifecycle structs → `StructLifecycleWF spec` (the generic `lifecycle_sound` applies).

This automates the generation and checking of obligations that were always implied by the
specification; it does not widen the proven surface. The correspondence remains
transcription, now regenerated each run. No new modelling axioms are introduced.
