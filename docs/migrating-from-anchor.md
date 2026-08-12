# Migrating a stock-Anchor program to verified-anchor

verified-anchor verifies a **subset** of Anchor's `#[derive(Accounts)]` account validation.
Programs in the subset get a machine-checked guarantee that the generated validation and
lifecycle code implements the formal contract.

## Syntax mapping

verified-anchor is signature-identical to stock Anchor at the account-validation surface.
A typical struct migrates field-for-field:

| Stock Anchor                                  | verified-anchor                                              |
|-----------------------------------------------|-------------------------------------------------------------|
| `pub vault: Account<'info, Vault>`            | `pub vault: Account<'info, Vault>`                          |
| `pub authority: Signer<'info>`                | `pub authority: Signer<'info>`                              |
| `pub system_program: Program<'info, System>`  | `pub system_program: Program<'info, System>`                |
| `#[account(init, payer = p, space = n)]`      | same                                                        |
| `#[account(init_if_needed, payer = p, space = n)]` | same — typed `Account<'info, T>` required (see below)  |
| `#[account(realloc = N, realloc::payer = p, realloc::zero = true)]` | same — requires `mut` (see below) |
| `#[account(zero)]`                            | same                                                        |
| `#[account(has_one = bank)]`                  | same                                                        |
| `#[account(seeds = [..], bump)]`              | same (canonical-only — see bridge)                          |
| `#[account(seeds = [..], bump = arg(off))]`   | same — stored/non-canonical opt-in (see Opt-outs below)    |
| `#[account(seeds = [..], seeds::program = e, bump)]` | same                                               |
| `#[account(address = <pubkey>)]`              | same                                                        |
| `#[account(executable)]`                      | same                                                        |
| `#[account(rent_exempt = enforce)]`           | same (proven with opaque boundary — see bridge)             |
| `#[account(constraint = <expr>)]`             | same — proven sublanguage where possible, honest escape hatch otherwise (see below) |
| `#[instruction(amount: u64, name: String)]`   | same — binds named arguments for `seeds`/`constraint` (see below) |
| `#[account]` on type T                        | `#[derive(BorshSerialize, BorshDeserialize, AccountData)]`  |

Plus: `use verified_anchor::prelude::*;` brings in everything (wrappers, traits, Context, derives).

The wrapper types' base checks are part of the proven subset, not just runtime conveniences:
`Account<'info, T>` implies `owner` + `discriminator`, `Signer<'info>` implies `signer`,
`SystemAccount<'info>` implies `owner == system_program`, and `Program<'info, P>` implies
`executable` + `key == P::ID`. Each maps to a constraint in the Lean contract that
`genValidate_sound` discharges (see [`verified-anchor-bridge.md`](verified-anchor-bridge.md)).

**Bare `u8` field types are not supported.** The macro requires one of the typed wrappers in
the table above. Declaring an account field as `u8` is a compile error; the
`#[derive(VerifiedAccounts)]` macro emits a `compile_error!` pointing back to this guide.

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
| `seeds = [..], bump` | yes (canonical-only, safe default) | validation |
| `seeds = [..], bump = arg(off)` | yes (stored/non-canonical opt-in) | validation |
| `seeds::program = <expr>` | yes | validation |
| `address = <pubkey>` | yes | validation |
| `executable` | yes | validation |
| `rent_exempt = enforce` | yes (proven with opaque `rentExemptMinimum` wall — see bridge) | validation |
| `rent_exempt = skip` | yes (explicit no-check opt-out) | — |
| distinct-mut-key check | yes, automatic (struct-level safety value-add — see bridge) | validation |
| `allow_duplicate = <field>` | yes (per-pair opt-out for distinct-mut check) | — |
| `init, payer = p, space = n` | yes | lifecycle (`init_establishes_post`) |
| `close = d` | yes | lifecycle (`close_establishes_post`) |
| `realloc = N, realloc::payer = p` | yes | lifecycle (`realloc_establishes_post`) |
| `realloc::zero = <bool>` | yes | runtime zero-fill flag; no separate proof obligation |
| `zero` | yes (validation constraint) | `genValidate_sound` / `genConstraint_zero_iff` |
| `init_if_needed, payer = p, space = n` | yes — typed `Account<'info, T>` required | lifecycle (`initIfNeeded_establishes_post`) |
| `constraint = <expr>` | yes — compiled into a proven relational sublanguage when possible, an honest reported escape hatch otherwise | validation (`genValidate_sound` at `M10Subset`) or unproven-but-enforced (see below) |
| `#[instruction(...)]` argument binding | yes | supporting infrastructure for `seeds`/`constraint` (see below) |
| `token::*`, `mint::*`, `associated_token::*` | **no** | rejected at compile time (planned M11) |

## Explicit opt-outs

The safe-by-default tenet means that Verified Anchor's defaults are always the safer choice.
Some Anchor patterns deliberately trade safety for flexibility; these require explicit opt-in:

- **Stored / non-canonical bump.** Write `bump = arg(off)` to read the bump from instruction
  data at byte offset `off`. This uses `createProgramAddress` (no canonical requirement).
  The canonical `bump` / `bump = n` remains the default and the safer choice.
- **Distinct-mut-key opt-out.** Add `#[account(allow_duplicate = <field>)]` to suppress the
  automatic same-key check for one specific `mut`-account pair. Use only when two `mut` fields
  intentionally point to the same account.
- **Skip rent-exempt check.** Write `rent_exempt = skip` to omit the rent check entirely.
  The default is to enforce it when `rent_exempt = enforce` is written; no annotation means
  no check (consistent with stock Anchor behaviour for existing constraints).

## Realloc, zero, and init_if_needed

**`realloc`.** Syntax is identical to stock Anchor:

```rust
#[account(mut, realloc = 128, realloc::payer = payer, realloc::zero = true)]
pub my_account: Account<'info, MyData>,
```

`realloc` requires `mut` — the macro emits a `compile_error!` if the field is not also
annotated `mut`. The funding model is top-up-only and surplus-preserving:
`lamports' = max(current, minimum_balance(newLen))`. The account is never debited; shrinks
and already-exempt accounts succeed without moving any lamports. This matches stock Anchor.
See [`verified-anchor-bridge.md`](verified-anchor-bridge.md) — Realloc section.

**`zero`.** Identical to stock Anchor. It is a **validation** constraint (not lifecycle):

```rust
#[account(zero)]
pub fresh_account: Account<'info, MyData>,
```

It checks that the first 8 bytes are all zero and rejects with `VAError::NotZeroed`. It is
proven via `genValidate_sound`, crypto-free, and reduces under `cargo verified-anchor check`.

**`init_if_needed`.** Syntax is identical to stock Anchor, with one additional requirement:
the field **must** be a typed `Account<'info, T>`. A bare `UncheckedAccount` or other
wrapper is rejected at compile time.

```rust
#[account(init_if_needed, payer = payer, space = 8 + 64)]
pub my_account: Account<'info, MyData>,
```

`init_if_needed` is a genuine drop-in: `try_accounts`/`validate` skips the wrapper-implied
owner and discriminator checks for this field (a fresh, system-owned, all-zero-discriminator
account cannot pass them), but keeps any explicitly declared `seeds` or `address`. The
`execute_lifecycle` step then provides the two-branch guard:

* **Fresh account** (discriminator bytes are all zero): creates it, funds it, and writes
  `T`'s real discriminator.
* **Existing account** (non-zero discriminator): accepts it only if `owner == program_id`
  and `data_len >= space + 8`; otherwise rejects with `VAError::InitFailed`. This is the
  reinit guard.

**Recommended: pair with `seeds` (PDA).** Without `seeds`, the existing-account branch
checks only owner + size, which is the same level of safety stock Anchor provides without
seeds. Adding `seeds` ties account identity to the PDA derivation on both branches and
closes the reinit-attack surface. See the exploit case studies for a worked example.

## `constraint = <expr>`

Syntax is identical to stock Anchor — verified-anchor never rejects a `constraint = <expr>`
attribute at compile time, because real Anchor accepts arbitrary Rust there and the drop-in
guarantee means verified-anchor must too:

```rust
#[account(constraint = vault.amount >= 1000 && vault.authority == authority.key())]
pub vault: Account<'info, Vault>,
```

Each expression is compiled one of two ways:

* **Proven sublanguage.** If the expression is a comparison (or `&&`/`||`/`!` combination of
  comparisons) over account metadata (`a.key()`, `a.owner`, `a.lamports()`, `a.data_len()`,
  `a.is_signer`, `a.is_writable`, `a.executable`), a literal, a `#[instruction(...)]` argument,
  or a Borsh-located field of a typed `Account<'info, T>`, it compiles into the proven
  relational sublanguage: a byte-level check in `validate`, covered by `genValidate_sound` at
  `M10Subset`, exactly like `signer`/`owner`/`has_one`. `nat`/`int` fields compare numerically
  (widened), so a signed and unsigned field can be compared directly.
* **Honest escape hatch.** Anything else — a function call, a macro, a module-qualified path, a
  multi-segment data path (`vault.inner.amount`), or a data field the sublanguage cannot locate
  (behind a float, a Borsh enum, or a fixed-size array whose length is a named const rather than
  an integer literal) — compiles to the developer's expression run **verbatim as Rust**, inside
  `try_accounts`, after the account has been Borsh-deserialised (so `vault.amount` means what it
  means in stock Anchor). This still runs and still rejects on failure
  (`VAError::ConstraintViolated { field, expr }`) — it is simply not part of the Lean proof.

**How it is reported.** `cargo verified-anchor check` lists every escape-hatch expression per
struct (the developer's exact source text) in its human report and in `--json` output; pass
`--deny-unproven` to make any unproven check a CI failure. See
[`verified-anchor-bridge.md`](verified-anchor-bridge.md) for the exact guarantee this gives you:
soundness (never accepting what the contract rejects) holds unconditionally, including for
structs that use the escape hatch — only completeness (whether a legitimate account set is
accepted) is affected by an unproven check.

## `#[instruction(...)]` argument binding

Syntax is identical to stock Anchor:

```rust
#[instruction(amount: u64, name: String)]
#[derive(VerifiedAccounts)]
pub struct Deposit<'info> {
    #[account(seeds = [b"vault", name.as_bytes()], bump)]
    pub vault: Account<'info, Vault>,
    #[account(constraint = amount > 0)]
    pub source: Account<'info, Source>,
}
```

Supported argument types are the ones `Ty` can express: `u8`..`u128`/`i8`..`i128`, `bool`,
`Pubkey`, `String`, `Vec<T>` (recursively, for a mappable `T`), and `Option<T>`. An argument of
an unmappable type (a float, an enum, a user struct `Ty` cannot describe) — or any argument
declared after one — cannot be used in a `seeds` expression or reach the proven sublanguage;
using it in `constraint = <expr>` still compiles, via the escape hatch.

**`instr_data` convention — read this before writing a handler.** `instr_data`, as
`#[instruction(...)]` decodes it and as `validate`/`try_accounts` receive it, is the
instruction's **argument buffer with any instruction discriminator (sighash) already stripped
by the caller** — exactly what stock Anchor hands its own `try_accounts`. Decoding starts at
byte offset 0. **This is a real footgun**: a handler that forwards the raw instruction data
Solana delivers to the program entrypoint — which, under Anchor's own wire format, begins with
an 8-byte discriminator — without stripping it first will misdecode every `#[instruction(...)]`
argument (and every `arg(off, len)` / `bump = arg(off)` offset), silently reading the wrong
bytes rather than failing loudly. Strip the discriminator (or use the entrypoint macro that
already does) before calling `try_accounts`.

## `arg(off, len)` (deprecated)

The pre-M10 raw-slice seed form still works and is not going away — removing it would break
existing verified-anchor users — but new code should prefer `#[instruction(...)]` +
`name.as_bytes()` / `amount.to_le_bytes()`, which is what real Anchor source actually writes
and what `cargo verified-anchor check` can additionally cross-check against the argument's
declared type:

```rust
// Deprecated, still supported:
#[account(seeds = [b"vault", arg(0, 8)], bump)]
// Preferred:
#[instruction(amount: u64)]
#[account(seeds = [b"vault", amount.to_le_bytes()], bump)]
```

## `has_one` — behaviour change if upgrading from < v0.4.0

**Before v0.4.0**, the generated `has_one` check read the target field unconditionally at byte
offset 8 (immediately after the 8-byte discriminator) — correct only when the `has_one` target
happened to be the struct's FIRST field. A program whose `has_one` target was declared second,
third, or later — a perfectly ordinary Anchor struct — was **mis-checked**: the comparison ran
against the wrong bytes.

**As of v0.4.0**, `#[derive(AccountData)]` emits the struct's real Borsh field layout
(`T::LAYOUT`), and `has_one = field` locates `field` at its actual offset via `locate`
(walking past any preceding fixed- or variable-width fields), matching stock Anchor exactly
regardless of field order. If you were relying on the old (incorrect) offset-8 behaviour for a
non-first-field target — which was never a documented or intended semantics — re-run your test
suite; the check now examines different bytes and may reject something it previously accepted
by accident, or (correctly) reject something it previously passed by accident.

## The seed-spelling boundary

`seeds = [...]` accepts the spellings a real, unmodified Anchor program actually writes.
Determined empirically by compiling each form against the macro:

**Parses (accepted):**

* `b"vault"`, `b"vault".as_ref()`, `&b"vault"` — byte-string literal
* `"vault".as_bytes()` — str-literal spelling of the same literal seed
* `user.key()`, `user.key().as_ref()` — an account field's pubkey
* `name.as_bytes()` — a `#[instruction(...)]` `String`/`Vec<u8>` argument
* `amount.to_le_bytes()`, `amount.to_le_bytes().as_ref()`, `&amount.to_le_bytes()` — a
  `#[instruction(...)]` numeric argument, little-endian (matching Borsh)
* `authority.as_ref()` — a `#[instruction(...)]` `Pubkey`/`Vec` argument
* `&blob`, `blob.as_slice()` — a bare byte-slice-shaped argument reached via `&`/`.as_slice()`
* `arg(off, len)` — the deprecated raw-slice form (still works; see above)

**Does NOT parse — every one of these is a BUILD ERROR, never a silently different PDA:**

* `SEED_CONST`, `SEED_CONST.as_ref()` — a module-level constant; not resolvable to a specific
  account field or instruction argument at macro time
* `crate::ID.as_ref()`, or any multi-segment path — same reason
* `ctx.accounts.user.key().as_ref()` — the macro resolves seeds against the struct's OWN
  fields, not through a `ctx.accounts.` prefix (real Anchor seed expressions inside the derive
  don't have a `ctx` in scope either)
* `user.key.as_ref()` — the `key` FIELD (not the `.key()` METHOD) of an `AccountInfo`; not
  recognised
* `&user.key().to_bytes()` — an extra conversion the macro does not peel
* `vault.owner.as_ref()` — a field of a DESERIALISED account struct; not resolvable at
  seed-derivation time (`validate` runs before deserialisation)
* `&name.as_bytes()[..4]` — indexing/slicing a seed source is not supported
* `flag.as_ref()` on a `bool` argument — no `AsRef<[u8]>` seed spelling for `bool`
* an `Option<_>` argument used directly as a seed
* any computed expression (arithmetic, a function call, a ternary-like `if`)

**Why this boundary is safe to publish as-is: every unsupported spelling above is a compile-time
error, never a silently different PDA.** There is no case where an unrecognised seed expression
compiles and derives an address other than the one Anchor would derive from the same source —
it either compiles to the same bytes Anchor would use, or it fails to compile at all.

**`to_be_bytes()` / `to_ne_bytes()` are refused deliberately, not merely unsupported.** Borsh —
and therefore the PDA Anchor itself derives — is little-endian. `to_be_bytes()` would silently
derive a *different* address than the same source compiled under real Anchor: a security bug
that looks like "wrong account", not "won't compile". `to_ne_bytes()` is refused for the same
reason even though it happens to equal `to_le_bytes()` on-chain (BPF is little-endian): code
using it would pass every on-chain test and then be silently wrong the moment the same `lean_spec()`
or any off-chain / big-endian-host tooling evaluates it. Both are compile errors pointing at
`to_le_bytes()`.

## Limitations

- `seeds` / `bump` **canonical is the safe default**. A declared `bump = n` must equal the
  canonical bump returned by `find_program_address`. Stored bumps are available as an
  explicit opt-in (`bump = arg(off)`) — see Explicit opt-outs above.
- **`rentExemptMinimum` is an opaque boundary.** `rent_exempt = enforce` is proven correct
  against an opaque Lean constant; its correspondence to Solana's `Rent::is_exempt` is
  cross-checked empirically by litesvm, not axiomatically. See
  [`verified-anchor-bridge.md`](verified-anchor-bridge.md) — Honesty boundary section.
- The Rust↔Lean correspondence is **transcription**: the Lean and Rust sides interpret the same
  `AccountsStruct` literal, and the proof relates the Lean side to the contract. The
  correspondence is mechanically regenerated and runtime-tested, not proved as a cross-language
  property. See [`verified-anchor-bridge.md`](verified-anchor-bridge.md) for the full discussion.
- `init` / `close` model the documented effect on lamports, ownership, and the discriminator
  marker. The actual CPI dispatch path and the Rust-to-sBPF compilation are not modelled.
- `realloc` uses the same `rentExemptMinimum` opaque wall as `rent_exempt = enforce`. The
  lamport model (top-up-only, surplus-preserving) matches stock Anchor; the proof is against
  the opaque constant, empirically cross-checked by litesvm.
- `init_if_needed` without `seeds` checks only owner + size on the existing-account branch
  (same limitation as stock Anchor without seeds). Pair with `seeds` for full identity
  enforcement on both branches.
- `init_if_needed` with `arg(..)` seeds is not supported in `execute_lifecycle` (the
  lifecycle executor has no instruction data to derive them from); use literal or
  field-key seeds instead.
- **An unlocatable `has_one` target is a build error.** `has_one` needs the named field's real
  Borsh offset from `T::LAYOUT`. If the target sits behind — or is itself — a non-literal-length
  array (`[u8; N]` with `N` a named const), a nested struct, or an enum, `T::LAYOUT` does not
  record it and the macro rejects the struct at compile time. Unlike `constraint = <expr>`,
  `has_one` is declarative: there is no developer expression to fall back to, so an unlocatable
  target genuinely cannot be checked rather than being routed to an escape hatch.
- **`constraint = <expr>` over a multi-segment data path** (e.g. `vault.inner.amount`) falls out
  of the proven sublanguage to the escape hatch: `map_ty` cannot yet produce a nested
  `Ty::Struct`, so no descriptor can locate a field two levels deep. The expression still
  compiles and still runs — just unproven. See "The escape hatch" above.
- **`constraint = <expr>` over a NON-SCALAR data field** (`[T; N]`, `String`, `Vec<T>`,
  `Option<T>`, a nested struct) also falls to the escape hatch, as of v0.4.0. The byte-level
  reader `read_val` decodes only scalars — integers, `bool`, `Pubkey` — mirroring Lean's
  `readVal`, so a comparison like `vault.root == root` over a `[u8; 32]` cannot be part of the
  proven core. It is reported in `UNPROVEN_CHECKS` and enforced as verbatim Rust in
  `try_accounts`, exactly as real Anchor would run it. (Before v0.4.0 an array field was
  *reported as proven* and then rejected every account — see the v0.4.0 fix notes.)
- **`nat`/`int` comparisons in `constraint = <expr>` ARE supported**, numerically widened
  (a signed and an unsigned field, or a signed field and an unsigned literal, compare correctly
  against each other). **Floats and Borsh enums are not modelled** in the `Ty` descriptor at
  all — any field of, or expression touching, either type falls to the escape hatch.
- **`AccountData` gained required `LAYOUT` and `LAYOUT_LEAN` associated consts in v0.4.0.**
  This breaks any HAND-WRITTEN `impl AccountData for T`, which will no longer compile until the
  two consts are added. The fix is not to add them by hand: **`AccountData` should only ever be
  derived**, via `#[derive(AccountData)]` or the `#[verified_anchor::account]` attribute. The
  derive computes `LAYOUT` from the struct's real field declarations and emits `LAYOUT_LEAN` as
  the matching Lean term, so the proven spec and the bytes the runtime actually walks describe
  the same layout by construction. A hand-written impl can silently disagree with the struct's
  Borsh encoding, and every offset the proof reasons about — `has_one`, `constraint = <expr>` —
  is taken from `LAYOUT`. That makes a wrong `LAYOUT` a way to obtain a *valid proof of the
  wrong statement*, which no other input to this system can do.
- **`lean_spec()` is now `#[cfg(not(target_os = "solana"))]`, as of v0.4.0.** It was always
  host-only in intent (the Lean source string is development-time metadata, spliced from the
  typed field's real Borsh layout), but was not previously gated. If any code in your crate
  called `<YourStruct>::lean_spec()` directly from a path that gets compiled into the on-chain
  `.so` (rather than only from `cargo verified-anchor check`'s own tooling / a host-only test),
  that call is now a compile error under a BPF build. This is a genuine public-API removal in
  BPF builds, not a bug fix — `UNPROVEN_CHECKS` is gated the same way, for the same reason.
