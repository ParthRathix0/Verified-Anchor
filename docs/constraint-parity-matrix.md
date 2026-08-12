# Constraint parity matrix

Every Anchor `#[account(...)]` constraint and its status in Verified Anchor as of v0.4.0.

**Status key:**

| Status | Meaning |
|---|---|
| **Proven** | Modelled in Lean, proven equal to the formal contract via `genValidate_sound` at `M10Subset` (or `lifecycle_sound` for lifecycle effects). |
| **Proven + honesty boundary** | Proven correct against an opaque Lean constant; the constant's correspondence to the Solana runtime value is cross-checked empirically by litesvm, not axiomatically. |
| **Proven + escape hatch** | Compiled into the proven sublanguage when the expression and every data field it touches fit it; otherwise runs as unproven-but-enforced verbatim Rust in `try_accounts`, reported by `cargo verified-anchor check`. Never a silent gap and never a `compile_error!`. See [`verified-anchor-bridge.md`](verified-anchor-bridge.md) for the guarantee this gives (soundness unconditional; only completeness is affected). |
| **Unsupported (compile_error)** | The macro emits a `compile_error!` pointing to the migration guide. The constraint is not silently ignored. |
| **Planned (Mn)** | Will be added in milestone Mn. Currently emits a `compile_error!`. |

---

## Field-level validation constraints

| Anchor `#[account(...)]` | Lean constraint | Status | Notes |
|---|---|---|---|
| `signer` | `Constraint.signer` | **Proven** | `genValidate_sound` |
| `mut` | `Constraint.mut` | **Proven** | `genValidate_sound` |
| `owner = <expr>` | `Constraint.owner (expected : Pubkey)` | **Proven** | Placeholder pubkey `Pubkey.zero` in `lean_spec`; theorem is ∀ over the pubkey. |
| `has_one = <field>` | `Constraint.hasOne (field : String)` | **Proven** | Relational; locates `field` at its REAL Borsh offset via `T::LAYOUT`/`locate` (as of v0.4.0 — before v0.4.0 the codegen hardcoded offset 8, which mis-checked any target that was not the struct's first field; see the migration guide's "behaviour change" note). A target unlocatable in `T::LAYOUT` (behind a non-literal-length array, a nested struct, or an enum) is a build error, not a runtime brick — `has_one` is declarative, so there is no developer fallback expression. |
| `seeds = [...], bump` | `Constraint.seeds ss (BumpSpec.canonical)` | **Proven** | Canonical-only PDA via `findProgramAddress`; `genValidate_sound`. |
| `seeds = [...], bump = n` | `Constraint.seeds ss (BumpSpec.literal n)` | **Proven** | Declared bump must equal the canonical bump; proven at `M10Subset`. |
| `seeds = [...], bump = arg(off)` | `Constraint.seeds ss (BumpSpec.stored off)` | **Proven** | Non-canonical opt-in; re-derives via `createProgramAddress`. Explicit less-safe opt-in. |
| `seeds::program = <expr>` | `Constraint.seeds ss bump (program := some pid)` | **Proven** | PDA against a foreign program id; `program : Option Pubkey` on `Constraint.seeds`. |
| `address = <pubkey>` | `Constraint.address (expected : Pubkey)` | **Proven** | `VAError::WrongAddress` (code 12). Placeholder pubkey in `lean_spec`. |
| `executable` | `Constraint.executable` | **Proven** | `VAError::NotExecutable` (code 13). Also implied by `Program<'info, P>`. |
| `rent_exempt = enforce` | `Constraint.rentExempt` | **Proven + honesty boundary** | `rentExemptMinimum : Nat → Lamports` is opaque (like `sha256`/`isOnCurve`). Correspondence to `Rent::is_exempt` cross-checked by litesvm. `VAError::NotRentExempt` (code 15). |
| `rent_exempt = skip` | (no constraint emitted) | **Proven** | Explicit opt-out; emits no check. Safe-by-default consistent. |
| `discriminator = "Name"` | `Constraint.discriminator (d : ByteArray)` | **Proven** | Computes `sha256("account:Name")[..8]`; opaque under `sha256` but proven symbolically. |
| `allow_duplicate = <field>` | suppresses `distinctMutKeys` for the named pair | **Proven** | Per-pair opt-out for the automatic distinct-mut-key check. |
| `constraint = <expr>` | `Constraint.expr (e : Expr)` | **Proven + escape hatch** | `Operand`/`Cmp`/`Expr` sublanguage, `evalExpr` (fail-closed, strict `and`/`or`); `genValidate_sound` at `M10Subset` when the expression and every data field it reads compile into it. `nat`/`int` compare numerically widened. Falls to the escape hatch on a function call, macro, multi-segment path, a field behind/of a float or Borsh enum (unlocatable in `T::LAYOUT`), or a field `read_val` cannot decode even though `T::LAYOUT` names it — every aggregate: `[T; N]`, `String`, `Vec<T>`, `Option<T>`, a nested struct (as of v0.4.0). `VAError::ConstraintViolated { field, expr }`. |
| `#[instruction(name: T, ...)]` argument binding | `AccountsStruct.instrArgs : List (String × Ty)` | **Proven** | Named-argument infrastructure for `seeds` and `constraint`; decoded from `instr_data` (discriminator already stripped by the caller) via `locate`/`readVal` over `Ty.struct s.instrArgs`. An argument of an unmappable type (or any argument declared after one) cannot reach `seeds`/the proven sublanguage; still usable in `constraint = <expr>` via the escape hatch. |

## Struct-level validation (automatic)

| Check | Lean predicate | Status | Notes |
|---|---|---|---|
| All `mut` accounts have pairwise-distinct keys | `distinctMutKeys` folded into `genValidate` | **Proven** | Safety value-add beyond stock Anchor. Covers the "duplicate mutable accounts" bug class. Per-pair opt-out via `allow_duplicate`. `VAError::DuplicateAccount` (code 14). |

## Typed-wrapper implied constraints

| Wrapper type | Implied constraints | Status |
|---|---|---|
| `Account<'info, T>` | `owner`, `discriminator` | **Proven** |
| `Signer<'info>` | `signer` | **Proven** |
| `SystemAccount<'info>` | `owner == system_program` | **Proven** |
| `Program<'info, P>` | `executable`, `address == P::ID` | **Proven** |
| `UncheckedAccount<'info>` / `AccountInfo<'info>` | (none) | **Proven** |

## Lifecycle constraints

| Anchor `#[account(...)]` | Lean model | Status | Notes |
|---|---|---|---|
| `init, payer = p, space = n` | `applyInit` state transformer | **Proven** | `init_establishes_post`: post-state has owner set and data ≥ space+8. |
| `close = dest` | `applyClose` state transformer | **Proven** | `close_establishes_post`: post-state has lamports zero and closed-account marker. |
| `realloc = N` | `applyRealloc` state transformer | **Proven** | `realloc_establishes_post`: size=N, rent-exempt, never-debited. Requires `mut`. |
| `realloc::payer = p` | `applyRealloc` (payerIdx parameter) | **Proven** | Funds the top-up; payer must be writable signer. |
| `realloc::zero = <bool>` | `applyRealloc` (`zero : Bool` parameter) | **Proven** | Runtime zero-fill flag; schematic in the proof. |
| `zero` | `Constraint.zero` (validation) | **Proven** | `genValidate_sound` / `genConstraint_zero_iff`. Checks first 8 bytes all-zero. Crypto-free. `VAError::NotZeroed`. |
| `init_if_needed, payer = p, space = n` | `applyInitIfNeeded` state transformer | **Proven** | `initIfNeeded_establishes_post`: both branches leave owner=program ∧ data≥space+8. Requires typed `Account<'info, T>`. |

## Unsupported / planned constraints

| Anchor `#[account(...)]` | Status | Notes |
|---|---|---|
| `token::mint`, `token::authority`, `token::token_program` | **Planned (M11)** | SPL Token `Account` layout modelling. |
| `mint::authority`, `mint::decimals`, `mint::freeze_authority` | **Planned (M11)** | SPL `Mint` layout. |
| `associated_token::mint`, `associated_token::authority`, `associated_token::token_program` | **Planned (M11)** | ATA derivation. |

**Not modelled at all (falls to the `constraint = <expr>` escape hatch, not this table):**
floats and Borsh enums have no `Ty` variant, so a `constraint = <expr>` field of, or reading
behind, either type is unlocatable and compiles to the escape hatch rather than the proven
sublanguage. This is a `Ty`-modelling gap, not an unsupported *constraint* — there is no
`token::*`/`mint::*`-style `compile_error!` for it.

---

*This matrix reflects the constraint surface as accepted or rejected by the macro today.
Every **Unsupported (compile_error)** / **Planned** entry emits a `compile_error!` pointing to
[`migrating-from-anchor.md`](migrating-from-anchor.md). Every **Proven + escape hatch** entry
compiles and runs unconditionally — nothing in that category is ever rejected at compile time.
No constraint is silently ignored.*
