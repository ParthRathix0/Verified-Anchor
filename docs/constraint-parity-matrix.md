# Constraint parity matrix

Every Anchor `#[account(...)]` constraint and its status in Verified Anchor as of v0.2.0.

**Status key:**

| Status | Meaning |
|---|---|
| **Proven** | Modelled in Lean, proven equal to the formal contract via `genValidate_sound` at `M4Subset` (or `lifecycle_sound` for lifecycle effects). |
| **Proven + honesty boundary** | Proven correct against an opaque Lean constant; the constant's correspondence to the Solana runtime value is cross-checked empirically by litesvm, not axiomatically. |
| **Unsupported (compile_error)** | The macro emits a `compile_error!` pointing to the migration guide. The constraint is not silently ignored. |
| **Planned (Mn)** | Will be added in milestone Mn. Currently emits a `compile_error!`. |

---

## Field-level validation constraints

| Anchor `#[account(...)]` | Lean constraint | Status | Notes |
|---|---|---|---|
| `signer` | `Constraint.signer` | **Proven** | `genValidate_sound` |
| `mut` | `Constraint.mut` | **Proven** | `genValidate_sound` |
| `owner = <expr>` | `Constraint.owner (expected : Pubkey)` | **Proven** | Placeholder pubkey `Pubkey.zero` in `lean_spec`; theorem is ∀ over the pubkey. |
| `has_one = <field>` | `Constraint.hasOne (field : String)` | **Proven** | Relational; reads 32 bytes at offset 8 of account data. |
| `seeds = [...], bump` | `Constraint.seeds ss (BumpSpec.canonical)` | **Proven** | Canonical-only PDA via `findProgramAddress`; `genValidate_sound`. |
| `seeds = [...], bump = n` | `Constraint.seeds ss (BumpSpec.literal n)` | **Proven** | Declared bump must equal the canonical bump; proven at `M4Subset`. |
| `seeds = [...], bump = arg(off)` | `Constraint.seeds ss (BumpSpec.stored off)` | **Proven** | Non-canonical opt-in; re-derives via `createProgramAddress`. Explicit less-safe opt-in. |
| `seeds::program = <expr>` | `Constraint.seeds ss bump (program := some pid)` | **Proven** | PDA against a foreign program id; `program : Option Pubkey` on `Constraint.seeds`. |
| `address = <pubkey>` | `Constraint.address (expected : Pubkey)` | **Proven** | `VAError::WrongAddress` (code 12). Placeholder pubkey in `lean_spec`. |
| `executable` | `Constraint.executable` | **Proven** | `VAError::NotExecutable` (code 13). Also implied by `Program<'info, P>`. |
| `rent_exempt = enforce` | `Constraint.rentExempt` | **Proven + honesty boundary** | `rentExemptMinimum : Nat → Lamports` is opaque (like `sha256`/`isOnCurve`). Correspondence to `Rent::is_exempt` cross-checked by litesvm. `VAError::NotRentExempt` (code 15). |
| `rent_exempt = skip` | (no constraint emitted) | **Proven** | Explicit opt-out; emits no check. Safe-by-default consistent. |
| `discriminator = "Name"` | `Constraint.discriminator (d : ByteArray)` | **Proven** | Computes `sha256("account:Name")[..8]`; opaque under `sha256` but proven symbolically. |
| `allow_duplicate = <field>` | suppresses `distinctMutKeys` for the named pair | **Proven** | Per-pair opt-out for the automatic distinct-mut-key check. |

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
| `init, payer = p, space = n` | `applyInit` state transformer | **Proven** | `lifecycle_sound` / `init_establishes_post`. |
| `close = dest` | `applyClose` state transformer | **Proven** | `lifecycle_sound` / `close_establishes_post`. |
| `realloc, realloc::payer, realloc::zero` | — | **Planned (M9)** | Lifecycle parity milestone. |
| `zero` | — | **Planned (M9)** | Reinit guard. |
| `init_if_needed` | — | **Planned (M9)** | Guarded conditional init. |

## Unsupported / planned constraints

| Anchor `#[account(...)]` | Status | Notes |
|---|---|---|
| `constraint = <expr>` | **Planned (M10)** | Restricted relational sublanguage + honest escape hatch for out-of-sublanguage expressions. |
| `token::mint`, `token::authority`, `token::token_program` | **Planned (M11)** | SPL Token `Account` layout modelling. |
| `mint::authority`, `mint::decimals`, `mint::freeze_authority` | **Planned (M11)** | SPL `Mint` layout. |
| `associated_token::mint`, `associated_token::authority`, `associated_token::token_program` | **Planned (M11)** | ATA derivation. |

---

*This matrix reflects the constraint surface as accepted or rejected by the macro today.
Every unsupported entry emits a `compile_error!` pointing to [`migrating-from-anchor.md`](migrating-from-anchor.md).
No constraint is silently ignored.*
