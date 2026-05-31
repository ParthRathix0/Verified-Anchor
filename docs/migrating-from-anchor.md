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
| `#[account]` on type T                        | `#[derive(BorshSerialize, BorshDeserialize, AccountData)]`  |

Plus: `use verified_anchor::prelude::*;` brings in everything (wrappers, traits, Context, derives).

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
| `seeds = [..], bump` | yes (canonical-only) | validation |
| `init, payer = p, space = n` | yes | lifecycle (`lifecycle_sound`) |
| `close = d` | yes | lifecycle |
| `realloc`, `zero`, `constraint = expr`, `token::*`, `mint::*`, `associated_token::*`, `address`, `executable`, `rent_exempt` | **no** | rejected at compile time |

## Limitations

- `seeds` / `bump` is **canonical-only**. A declared `bump = n` must equal the canonical bump
  returned by `find_program_address`. This is stricter than stock Anchor's stored-bump form.
- The Rust↔Lean correspondence is **transcription**: the Lean and Rust sides interpret the same
  `AccountsStruct` literal, and the proof relates the Lean side to the contract. The
  correspondence is mechanically regenerated and runtime-tested, not proved as a cross-language
  property. See [`verified-anchor-bridge.md`](verified-anchor-bridge.md) for the full discussion.
- `init` / `close` model the documented effect on lamports, ownership, and the discriminator
  marker. The actual CPI dispatch path and the Rust-to-sBPF compilation are not modelled.
