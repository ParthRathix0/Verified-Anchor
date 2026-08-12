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
| `if accounts.len() < n { Err(NotEnoughAccounts) }` | `decide (s.fields.length ≤ c.length)` | `WellFormed` |
| `let (off, ty) = locate(T::LAYOUT, &["field"], data, 0)?; if read_val(&ty, data, off) != Some(Value::Key(target.key)) { Err(WrongHasOne) }` | `genHasOne` (locate the named field via `f.ty.locateField field a.data` at its REAL Borsh offset, `readVal`, compare to the looked-up key) | `satisfies … (.hasOne field)` |
| `if a.field.locate(…) is unlocatable { compile_error! }` (macro-time, via `has_top_level_field`) | `AccountType.locateField` returning `none` for the target | — (build error, not a runtime check; see "Borsh field model" below) |
| `if !condA \|\| !condB { Err(ConstraintViolated) }` (`constraint = <expr>`, compiled) | `evalExpr` over the `Operand`/`Cmp`/`Expr` sublanguage, strict `and`/`or`, fail-closed | `satisfies … (.expr e)` |
| verbatim developer Rust, run in `try_accounts` after deserialisation (escape hatch) | not modelled — outside `genValidate`/`evalExpr` entirely | — (unproven; see "The constraint expression sublanguage" below) |
| `let (pda,_) = find_program_address(seeds, program_id); if accounts[i].key != pda { Err(WrongPda) }` | `genSeeds` (canonical PDA equals the account key; bump matches) | `satisfies … (.seeds ss bump)` |
| `let pda = create_program_address(seeds_with_stored_bump, pid); if accounts[i].key != pda { Err(WrongPda) }` | `genSeeds` with `BumpSpec.stored` (re-derive via `createProgramAddress` at the bump byte from `instr_data[off]`, no canonical requirement) | `satisfies … (.seeds ss (BumpSpec.stored off))` |
| `let pid = <expr>; let (pda,_) = find_program_address(seeds, pid); if key != pda { Err(WrongPda) }` | `genSeeds` with a `program : some pid` field on `Constraint.seeds` (derive PDA against a foreign program id) | `satisfies … (.seeds ss bump)` with the resolved `pid` |
| `if accounts[i].key != &expected { Err(WrongAddress) }` | `genConstraint … (.address e) := decide (a.key = e)` | `satisfies … (.address e)` |
| `if !accounts[i].executable { Err(NotExecutable) }` | `genConstraint … .executable := a.executable` | `satisfies … .executable` |
| iterate over all `(i,j)` pairs of `mut` fields; `if accounts[i].key == accounts[j].key { Err(DuplicateAccount) }` (unless `allow_duplicate` opts out per pair) | `distinctMutKeys` folded into `genValidate` (struct-level check, all pairwise-distinct-key obligations) | `satisfies … (struct-level distinct-mut-keys)` |
| `let min = rent_exempt_minimum(accounts[i].data.len()); if accounts[i].lamports < min { Err(NotRentExempt) }` | `genConstraint … .rentExempt` via opaque `rentExemptMinimum : Nat → Lamports` (see Honesty boundary below) | `satisfies … .rentExempt` |
| `invoke(create_account(...)) + write disc` | `applyInit` (state transformer) | `init_establishes_post`: post-state has owner set and at least `space + 8` bytes |
| `dest.lamports += t.lamports; t.lamports = 0; mark` | `applyClose` (state transformer) | `close_establishes_post`: post-state has lamports zero and a closed-account marker |
| `let min = rent.minimum_balance(newLen); if min > cur { transfer(delta) }; account.realloc(newLen, zero)` | `applyRealloc` (state transformer) | `realloc_establishes_post`: size=newLen ∧ rent-exempt ∧ never-debited |
| `if data[..8] != [0u8;8] { Err(NotZeroed) }` | `genConstraint … .zero` | `satisfies … .zero`: the all-zero-discriminator reinit guard |
| uninitialized branch: `invoke(create_account(...)) + write T::DISCRIMINATOR`; existing branch: `if owner≠program ∥ data_len<space+8 { Err(InitFailed) }` | `applyInitIfNeeded` (state transformer) | `initIfNeeded_establishes_post`: both branches leave owner=program ∧ data≥space+8 |
| `encoded_width(Ty::Vec(e), ..)`: `count.checked_mul(w)?` yields `None` when the u32 length prefix times the element width overflows `usize` (`layout.rs`) | `encodedWidth`/`vecWidthFrom` accumulate in unbounded `Nat`, so the same input yields `some <huge>` (`Solana/Borsh/Locate.lean`) | — (**known divergence, safe direction**: both end in rejection. Rust's `None` propagates out of `locate` and the constraint fails closed; Lean's huge offset then fails the `off + w ≤ data.size` bounds test in `readVal`, which is also `none`, also fail-closed.) |
| an `argField` seed whose name `argBytes` cannot resolve in the instruction data: the generated Rust returns `Err(WrongPda)` for the field | `resolveSeeds` substitutes `ByteArray.empty` for the unresolved seed and derives a PDA from it (`Contract/Satisfies.lean`) | — (**known divergence, safe direction**: Rust rejects outright where Lean derives from an empty seed and *could* in principle match, so Rust rejects strictly more — never less.) |

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
    (s : AccountsStruct) (c : Ctx) (h : M10Subset s) :
  genValidate s c = true ↔ validates s c
```

The Lean model of the generated validator agrees with the declarative contract for every
struct in the supported subset (named `M10Subset` in Lean). The theorem is proved once,
parameterised over the user's struct. `#print axioms` reports `[propext, Quot.sound]` only —
no `sorryAx`, no `Classical.choice`, no `native_decide`. Per-constraint lemmas
(`genConstraint_{signer,mut,owner,discriminator,hasOne,seeds,executable,address,expr}_iff`, plus
`bumpMatchesB_iff`) connect each `gen*` to the corresponding `satisfies` case in the contract.

`M10Subset s` (now covering all supported features, including the `constraint = <expr>`
sublanguage) characterises structs in scope: every field's combined implied-and-declared
constraint list contains only
`{signer, mut, owner, hasOne, discriminator, seeds, executable, address, rentExempt, zero, expr}`
and struct-level `distinctMutKeys` is discharged. `M4Subset` remains as a reducible `abbrev` for
`M10Subset` — a proof obligation naming either compiles identically — so a published `cargo
verified-anchor` binary from before this release keeps working against the current Lean tree.

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

**Account count: a prefix condition on both sides (was a divergence before v0.4.0).** The
generated Rust guards with `accounts.len() < n`, so it accepts surplus accounts — only the
declared prefix `0..n` is inspected. Anchor passes the surplus through to
`ctx.remaining_accounts`, so rejecting it would break drop-in compatibility and the guard must
stay as it is.

Through v0.3 the Lean side did not match: `WellFormed` and `genValidate` both required an exact
count (`c.length = s.fields.length`), so on a slice with more accounts than the struct declares
Rust returned `Ok` while the contract and model rejected. That was a real gap between the
headline soundness sentence ("verified-anchor never accepts an account set the contract
rejects") and the contract as written, and it is closed in v0.4.0 by relaxing the Lean side to
`s.fields.length ≤ c.length` rather than by tightening the Rust.

Nothing is weakened by the relaxation, because nothing ever inspected a surplus account under
either formulation: every per-field check and the struct-level distinct-mut-key check range over
`s.fields.zipIdx`, i.e. the declared prefix only, so accounts at index ≥ `s.fields.length` were
unconstrained by the exact-count contract too. The change is to what the contract *claims*, not
to what it *checks*, and `genValidate_sound` re-proves unchanged. The Rust behaviour is pinned
by the `accepts_surplus_accounts` test.

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

## Realloc

`realloc = N`, `realloc::payer`, and `realloc::zero` are lifecycle effects modelled by the
`applyRealloc` state transformer and proven by `realloc_establishes_post`.

**Lamport model — top-up-only and surplus-preserving.** The modelled effect on lamports is

```
lamports' = max(lamports, rentExemptMinimum(newLen))
```

The account is **never debited**: if it already holds at least the rent-exempt minimum for
the new size (every shrink, every over-funded account), `delta = 0` and its lamports are
unchanged. This matches stock Anchor's behaviour exactly — no shrink refund and no surplus
drain. The Rust codegen computes `delta = minimum_balance(newLen).saturating_sub(current)`
and transfers only when positive; the Lean model formulates the same using `max` and
subtraction from the larger side so no `UInt64` operation underflows.

`realloc` requires `mut` — the macro emits a `compile_error!` if omitted.

`realloc::zero` is the standard `bool` flag passed to `AccountInfo::realloc`; it controls
whether the grown region is zeroed by the runtime. It has no Lean model of its own —
zeroing is a data-content concern, not a state-safety concern — and `realloc_establishes_post`
is schematic over it.

**Opaque wall.** `realloc` uses the existing `rentExemptMinimum : Nat → Lamports` opaque
constant introduced for `rent_exempt = enforce`. No new opaque wall is added. The
correspondence with Solana's `Rent::minimum_balance` is cross-checked empirically by the
litesvm runtime tests.

**Proven.** `realloc_establishes_post` is at `[propext, Quot.sound]`. It establishes three
post-conditions: `data.size = newLen`, `rentExemptMinimum newLen ≤ lamports'`, and
`lamports ≤ lamports'` (never-debited). The companion theorem
`applyRealloc_noTopUp_succeeds` is a regression witness proving that a already-exempt
account succeeds and its lamports are preserved unchanged.

## Zero

`#[account(zero)]` is a **validation** constraint (not a lifecycle effect). It guards that
the first 8 bytes of the account's data are all zero — the condition a freshly-allocated
system account or a just-created-but-not-yet-written program account satisfies, and the
condition a legitimately reused account does not.

`genConstraint … .zero` checks `data[0..8] == [0u8; 8]` and rejects with `VAError::NotZeroed`
on mismatch. The constraint is crypto-free and reduces under `decide`. It is folded into
`genValidate_sound` at the grown `M10Subset` — specifically through
`genConstraint_zero_iff` and `genConstraint_iff_satisfies_M4` — so it carries the same
`[propext, Quot.sound]` soundness guarantee as every other supported validation constraint.
No new opaque walls are introduced.

## `init_if_needed`

`init_if_needed` is a lifecycle effect modelled by the `applyInitIfNeeded` state transformer
and proven by `initIfNeeded_establishes_post`.

**Required field type.** The macro enforces that an `init_if_needed` field is a typed
`Account<'info, T>`. A compile error is emitted otherwise. The requirement is structural:
`execute_lifecycle` needs the concrete account type `T` to stamp its real discriminator on
the fresh account and to enforce owner + size on an existing one.

**How the two-phase design works.** `try_accounts` calls `validate` first, then
Borsh-deserialises each field. For an `init_if_needed` field, `validate` **skips** the
wrapper-implied `owner` and `discriminator` checks — they would block a fresh account
(owned by the System Program, all-zero discriminator) from passing. Any explicitly declared
`seeds` or `address` constraints are kept; they identify the account and are equally valid
on a fresh or existing account. After validation passes, `execute_lifecycle` provides the
actual two-branch guard:

* **Fresh branch** (`data[0..8] == [0u8; 8]` or `data.len() < 8`): creates the account via
  `system_instruction::create_account` (or `invoke_signed` for a PDA), then writes `T`'s
  real discriminator into the first 8 bytes. This corresponds to `applyInit` in the Lean
  model.
* **Existing branch** (non-zero discriminator): accepts the account **only** if
  `account.owner == program_id && account.data_len() >= space + 8`. If either condition
  fails, `VAError::InitFailed` is returned. This is the reinit guard — the SELF-GUARD that
  replaces the skipped wrapper checks — corresponding to the `else if a.owner = owner ∧
  space + 8 ≤ a.data.size then some c else none` branch of `applyInitIfNeeded`.

**Proven.** `initIfNeeded_establishes_post` holds at `[propext, Quot.sound]`. It shows that
both branches establish the same post-condition as `init`: the target account exists, is
owned by the program, and has at least `space + 8` bytes of data. The uninitialized branch
delegates wholesale to `init_establishes_post`; the existing-account branch succeeds only
because the guard (`owner = owner ∧ space + 8 ≤ a.data.size`) is a direct precondition of
the `some c` outcome.

**Recommended usage: pair with `seeds`.** An `init_if_needed` field without a `seeds`
constraint relies only on owner + size to accept an existing account. A legitimate-looking,
program-owned account of the correct size at the wrong identity would be accepted. Pairing
with `seeds` (a PDA) binds the account's identity to the seeds on both branches: the fresh
branch derives the canonical PDA and creates it at that address; the existing branch also
checks the key against the PDA derivation (through the `validate`-side `seeds` check, which
is kept). Seeds + `init_if_needed` is the safe default pattern; without seeds, the residual
limitation is the same as in stock Anchor.

**Trusted modelling assumption.** The same CPI-effect assumption as for `init` applies: that
`system_instruction::create_account` produces an account owned by the program and funded to
the declared size.

## PDA derivation

`seeds` and `bump` are pure validation checks. `genSeeds` mirrors `satisfies (.seeds ss bump)`,
and `genValidate_sound` holds at `M10Subset`. PDA derivation runs through the concrete
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
and included in `genValidate_sound` at `M10Subset`. Stock Anchor does not perform this check
automatically; verified-anchor's default is therefore **stricter than stock Anchor** here —
the "duplicate mutable accounts" bug class is closed by construction.

Per-pair opt-out: add `#[account(allow_duplicate = <field>)]` to suppress the check for one
specific pair. This is the explicit, user-visible escape hatch. The opt-out is never silent.
`VAError::DuplicateAccount` (code 14) is the rejection code.

## Borsh field model

`has_one` and the `constraint = <expr>` sublanguage both need to read a NAMED field out of an
account's raw Borsh-encoded bytes — not just its metadata (key, owner, lamports). Both sides
model this with the same three pieces:

* **`Ty`** — a Borsh type descriptor: the ten integer widths, `bool`, `pubkey`, fixed-size
  `array`, `string`, `vec`, `option`, and `struct` (a named field list). Lean:
  `VerifiedAnchor.Ty` (`lean/VerifiedAnchor/Solana/Borsh/Ty.lean`). Rust: `verified_anchor::
  layout::Ty` (`rust/verified-anchor/src/layout.rs`). Floats and Borsh enums are deliberately
  absent from `Ty` — see "What is not modelled" below.
* **`locate`** — walks a dotted field path through a `Ty::Struct` from a byte offset, stepping
  over each non-matching field by its *encoded* width (`encodedWidth`/`encoded_width`, which
  reads a `string`/`vec`'s length prefix or an `option`'s tag from the bytes, since those widths
  are not statically known from the type alone). Returns the target's byte offset and `Ty`, or
  `none`/`None` if the path does not exist in the descriptor. Lean:
  `lean/VerifiedAnchor/Solana/Borsh/Locate.lean`. Rust: `layout::locate`.
* **`readVal`/`read_val`** — decodes one scalar `Value` (`nat`/`int`/`bool`/`key`/`bytes`) at a
  given offset and `Ty`. Aggregates (`struct`, `vec`, `string`, `option`, `array`) are not
  values and yield `none`/`None` — a constraint can only compare *scalars*.

`#[derive(AccountData)]` emits each `#[account]` struct's real field layout as a `Ty::Struct`
associated const (`T::LAYOUT`), computed from the struct definition at macro-expansion time.
`has_one = field` and a `constraint = <expr>` data-field operand both compile to
`locate(T::LAYOUT, &["field"], data, 8)` (offset 8 to skip the discriminator) followed by
`read_val`, in both Rust and Lean — the two `Ty` trees, produced independently by the derive
macro and by `map_ty`, are asserted equal for representative structs in
`rust/verified-anchor/tests/lean_spec.rs` and cross-checked byte-for-byte against the real
`borsh` crate in `rust/verified-anchor/tests/borsh_differential.rs` (see the trust-boundary
entry below).

**Correspondence table (`Constraint.expr` and the rebuilt `hasOne`):**

| Generated Rust | Lean model | Discharges |
|---|---|---|
| `layout::locate(T::LAYOUT, path, data, 8)` then `layout::read_val` | `f.ty.locateField' path a.data` then `readVal` (via `evalOperand .field`) | operand resolution inside `evalExpr` |
| per-operand codegen (`Operand::{Field,Key,Owner,Lamports,DataLen,IsSigner,IsWritable,Executable,InstrArg}`) computed as `Option<...>`, combined STRICTLY (never Rust's short-circuiting `&&`/`\|\|`) | `evalOperand` / `evalCmp` / `evalExpr`, strict `and`/`or` in the `Option` monad | `satisfies … (.expr e)` |
| `nat`/`int` operands compared via a widened numeric comparison (so `delta < 0` on an `i64` field and `count > 0` on a `u64` field both type-check against a signed/unsigned literal) | `Value.toInt?` widens `nat`/`int` to `Int` before `evalCmp` orders them | same |
| target unlocatable in `T::LAYOUT` → `compile_error!` at macro expansion (`has_one`), or the field falls out of the sublanguage into the escape hatch (`constraint = <expr>`) | `AccountType.locateField`/`locateField'` returning `none` | — (build-time / escape-hatch routing, not a runtime check) |
| `constraint = <expr>` data field present in `T::LAYOUT` but NOT decodable by `read_val` (an aggregate: `Ty::{Array,String,Vec,Option,Struct}`) → escape hatch, via the const `layout::has_top_level_scalar_field` | `readVal` has no aggregate arm — every one of them is `none` | — (escape-hatch routing; see "readability, not merely presence" below) |
| an ORDERING (`<` `<=` `>` `>=`) whose operands are readable but NOT numeric (`Pubkey`, `bool`) → escape hatch, via the const `layout::has_top_level_orderable_field` | `Value.toInt?` is `none` outside `.nat`/`.int`, so `evalCmp`'s four ordering arms are `none` | — (escape-hatch routing; see "orderability, not merely readability" below) |

**What is not modelled.** `Ty` has no float and no Borsh-enum variant. A `has_one` target (or
a `constraint = <expr>` data-field operand) behind — or itself — a float, an enum, a nested
struct, or a fixed-size array whose length is a named const rather than an integer literal is
unlocatable in `T::LAYOUT`. For `has_one` this is a **build error** (see "Known limitations"
in the migration guide): `has_one` is declarative, so there is no developer expression to fall
back to. For `constraint = <expr>` it is not an error — the expression falls to the escape
hatch and still runs, just outside the proof.

**Readability, not merely presence (v0.4.0).** The const that routes a `constraint = <expr>`
data field between the proven core and the escape hatch asks whether `read_val` can DECODE the
field, not whether `T::LAYOUT` merely names it: `layout::has_top_level_scalar_field`, whose
truth table is exactly `read_val`'s arm split — `true` for the integers, `bool` and `Pubkey`;
`false` for `Array`, `String`, `Vec`, `Option` and `Struct`.

Presence was a sufficient test only while the descriptor TRUNCATED at the first unmappable
field, because "named" then implied "scalar". Once the derive learned to map `[T; N]`, an array
field became named while `read_val` kept refusing it, and a presence test would report
`constraint = vault.root == root` as *proven*: the check went into the Lean spec, the obligation
discharged honestly — Lean's `readVal .array` is `none`, so the contract faithfully says "reject
everything" — and the generated validator then rejected every account, matching root included.
A build error had silently become an always-reject, which is strictly worse.

The false branch routes to the **verbatim-Rust** hatch rather than to the recompiled
`layout::FieldValue` form, because `FieldValue` refuses the same aggregate set and would brick
identically. The recompiled form is retained only for expressions whose verbatim Rust would not
type-check — a negative literal against an unsigned field, or an ordering between two runtime
operands of possibly-different signedness — where the sublanguage is deliberately more
permissive than Rust. Both arms of the selection are type-checked whether or not the const picks
them, which is why that choice is made at macro-expansion time rather than in the user's crate.

**Orderability, not merely readability (v0.4.0).** Readability is necessary but not sufficient
for the four ORDERING comparisons. `Pubkey` and `bool` decode fine, so `read_val` answers for
them — but `Value.toInt?` does not, and `evalCmp`'s `lt`/`le`/`gt`/`ge` arms are therefore
`none`. A readability-only gate reported `constraint = pool.mint_a < pool.mint_b` — the
canonical AMM mint ordering — as *proven*, wrote it into the spec, discharged the obligation
honestly, and then rejected every pool, including correctly ordered ones: C1's exact shape one
question deeper. The gate an ordering must pass is therefore
`layout::has_top_level_orderable_field` (`true` for the ten integer types only), conjoined with
the readability terms; operands whose type is fixed at macro time (`key()`, `owner`,
`is_signer`, a `bool` literal, a non-numeric `#[instruction(..)]` argument) fail it statically.

`eq`/`ne` are deliberately EXEMPT: their `evalCmp` arms are total over `Value`, so
`constraint = vault.authority == authority.key()` — among the most common constraints in
Anchor — stays fully proven, as does `bool` equality. Only orderings narrow.

**Declared divergence: ordering inside the escape hatch.** The recompiled `layout::FieldValue`
fallback (`ExprCtx::deser`, one call site) orders same-constructor `Key`/`Bool` values where
Lean's `evalCmp` yields `none`. It is confined to constraints the gate has just REMOVED from
`lean_spec` and listed in `UNPROVEN_CHECKS`, so the contract makes no claim about them and
there is no `evalExpr` to disagree with; the hatch's obligation is parity with real Anchor's
verbatim Rust, which does order `Pubkey`s. It exists because the two fallback forms are chosen
at macro-expansion time, before field types are known, and the one syntactic shape "ordering
between two runtime operands" has to serve both `pool.mint_a < pool.mint_b` (needs `Key`
ordering) and `vault.delta < vault.amount` (needs the sublanguage's cross-signedness widening,
which Rust's type checker refuses). The proven path never sets `deser`.

## The constraint expression sublanguage (`constraint = <expr>`)

`constraint = <expr>` compiles into a small relational sublanguage when the expression fits it,
and into an honest, reported escape hatch otherwise. The macro NEVER rejects a `constraint =`
expression at compile time on the grounds that it is unsupported — real Anchor accepts
arbitrary Rust there, and verified-anchor's drop-in guarantee means it must too.

**The sublanguage.** `Operand` (a literal, `a.key()`/`a.owner`/`a.lamports()`/`a.data_len()`/
`a.is_signer`/`a.is_writable`/`a.executable`, a Borsh-located data field, or a named
`#[instruction(...)]` argument), `Cmp` (`==`, `!=`, `<`, `<=`, `>`, `>=`), and `Expr` (a
comparison, `&&`, `||`, `!`, or a bare boolean operand) are defined in
`lean/VerifiedAnchor/Constraints/Ast.lean`; `evalExpr` (`lean/VerifiedAnchor/Constraints/
Expr.lean`) gives them fail-closed semantics — any unevaluable operand makes the whole
expression `none`, which the contract reads as "not satisfied". The Rust codegen
(`rust/verified-anchor-macros/src/expr.rs`) parses a `syn::Expr` into the same `VExpr` shape
and emits Rust that computes an `Option<bool>` per operand and combines them exactly as
strictly as `evalExpr` does — **not** with Rust's native `&&`/`\|\|`, which would
short-circuit `true \|\| <unevaluable>` to `true` (ACCEPT) where the contract says `none`
(REJECT). This is the concrete mechanism behind the guarantee statement below.

**What compiles into the sublanguage.** Field-access and method-call operands on a struct
field or a typed account (see the table above); comparisons and boolean combinators over them;
`nat`/`int` comparisons (numerically widened, so a signed and unsigned field can be compared).

**What falls to the escape hatch.** A function call, a macro invocation, a module-qualified
path, a multi-segment data path (`vault.inner.amount` — `map_ty` cannot yet produce a nested
`Ty::Struct`, so the descriptor never contains one), a data field behind or of a float or Borsh
enum, and any `#[instruction(...)]` argument type `map_ty` cannot map. Falling out of the
sublanguage is silent to the developer at compile time (no error, no warning) — it is reported
at `cargo verified-anchor check` time instead (see below), which is where a proof-coverage
question belongs.

**How it is reported.** Every field whose `constraint = <expr>` fell to the escape hatch is
listed in that struct's `UNPROVEN_CHECKS: &'static [&'static str]` — the developer's exact
source text — and surfaced by `cargo verified-anchor check`'s human and `--json` reports (see
the CLI's own docs / `--deny-unproven`). `VAError::ConstraintViolated { field, expr }` is the
runtime rejection.

**The guarantee.**

> Unproven checks are additional conjuncts layered on the proven core, so they can only reject
> more. Verified Anchor never accepts an account set the contract rejects: soundness holds
> unconditionally, including for structs that use the escape hatch. Only completeness — whether
> a legitimate account set is accepted — is affected by unproven checks.

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

## Trust boundary: the generated Borsh locator vs the `borsh` crate

`sha256`, `isOnCurve`, and `rentExemptMinimum` are OPAQUE Lean constants: axioms whose
real-world correspondence (to a real hash function, curve check, and rent formula) is trusted
and cross-checked empirically rather than proved. The Borsh field locator is a different kind
of boundary — `locate`/`readVal` are fully concrete, computable Lean functions, not opaque
constants — but the same question applies to them: the generated Rust validation code emits
**our own locator** (`rust/verified-anchor/src/layout.rs`), mirroring the Lean model
byte-for-byte, and does **not** call the `borsh` crate at all. So whether our reimplementation
of Borsh's encoding actually agrees with what the `borsh` crate emits on the wire is not
assumed — it is established empirically, the same way `rentExemptMinimum`'s correspondence to
`Rent::is_exempt` is: `rust/verified-anchor/tests/borsh_differential.rs` serializes real structs
with `borsh::to_vec`, deserializes them with `borsh`'s own `try_from_slice`, and asserts our
`locate`/`read_val` agree with both, across fixed layouts, variable-length prefixes (`String`,
`Vec`, `Option`), nested structs, signed-integer boundary values, and truncated buffers (which
must fail closed on both paths). Grouped with `sha256` / `isOnCurve` / `rentExemptMinimum` as
a named, honestly-scoped boundary — **cross-checked, not axiomatized**.

## Developer surface

The Rust-to-Lean proof chain is unchanged from the macro's introduction. The derive emits an
`impl Validate` whose body is the per-constraint check sequence
(`signer` / `mut` / `owner` / `has_one` / `seeds` / `discriminator` / `expr`) that `genValidate`
models in Lean, with `M10Subset s → (genValidate s c = true ↔ validates s c)` proved
generically. **`Validate::validate` runs the proven core ONLY** — every check it performs is
one `genValidate`/`evalExpr` agrees with.

Alongside `Validate` the derive also emits `impl<'info> Accounts<'info>`, whose `try_accounts`
calls `Self::validate` first (the proven gate), then Borsh-deserialises each
`Account<'info, T>` field's data into the typed struct, then — and only then — runs any
`UNPROVEN_CHECKS` (the escape hatch: `constraint = <expr>` expressions that fell outside the
sublanguage, or whose sublanguage form reads a field the user's descriptor cannot locate) as
verbatim Rust against the deserialised bindings, where `vault.amount` means what Anchor means.
**Unproven checks run in `Accounts::try_accounts`, never in `Validate::validate`** — the split
is what keeps "the proof covers `validate`" a true statement even for a struct that uses the
escape hatch. Borsh deserialisation itself is outside the proven surface (a transcription
concern, like the CPI-effect modelling for `init`/`close`). A `BorshFailed` error is honest
runtime feedback, not a verification hole.

## What is out of scope

The fidelity of rustc, LLVM, and the sBPF code generator — i.e. that the compiled binary
faithfully executes the Rust source. This is the standard boundary of source-level
verification (see CompCert for context) and is not addressed by this project.

## Automated checking

The Rust-to-Lean flow is mechanical. `#[derive(VerifiedAccounts)]` auto-registers each struct
through the `inventory` crate; `verified_anchor::emit_specs!()` writes each struct's
`lean_spec()`; and `cargo verified-anchor check` generates a `check.lean` file containing
per-struct obligations and runs `lake env lean`. Each obligation is a single `decide`:

* validation structs → `M10Subset spec` (the generic `genValidate_sound` applies);
* lifecycle structs → `StructLifecycleWF spec` (the generic `lifecycle_sound` applies).

This automates the generation and checking of obligations that were always implied by the
specification; it does not widen the proven surface. The correspondence remains
transcription, now regenerated each run. No new modelling axioms are introduced.
