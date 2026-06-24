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
| `#[account(has_one = bank)]`                  | same                                                        |
| `#[account(seeds = [..], bump)]`              | same (canonical-only — see bridge)                          |
| `#[account(seeds = [..], bump = arg(off))]`   | same — stored/non-canonical opt-in (see Opt-outs below)    |
| `#[account(seeds = [..], seeds::program = e, bump)]` | same                                               |
| `#[account(address = <pubkey>)]`              | same                                                        |
| `#[account(executable)]`                      | same                                                        |
| `#[account(rent_exempt = enforce)]`           | same (proven with opaque boundary — see bridge)             |
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
| `init, payer = p, space = n` | yes | lifecycle (`lifecycle_sound`) |
| `close = d` | yes | lifecycle |
| `realloc`, `zero`, `constraint = expr`, `token::*`, `mint::*`, `associated_token::*` | **no** | rejected at compile time |

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
