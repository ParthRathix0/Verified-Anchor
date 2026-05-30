# Verified Anchor — Milestone 7a Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Lift the `#[derive(VerifiedAccounts)]` macro from `u8` spec-carrier fields to real Anchor-style typed wrappers (`Account<'info, T>`, `Signer<'info>`, `Program<'info, P>`, `SystemAccount<'info>`, `UncheckedAccount<'info>`, `AccountInfo<'info>`) with Borsh deserialization and a `Context<'a, 'b, 'c, 'info, T>` shape mirroring stock Anchor — so verified-anchor handlers are signature-identical to Anchor ones.

**Architecture:** Two trait layers — keep the proven `Validate` (unchanged, M1–M6 theorems still apply) and add a new `Accounts<'info>` developer surface whose `try_accounts` calls `validate` then Borsh-deserializes typed fields. The macro recognises wrapper types via `syn::Type` pattern matching and emits per-wrapper auto-implied checks that stack with explicit `#[account(...)]` attributes. A clean break: a bare `u8` field becomes `compile_error!` and all ~22 existing derived structs migrate to typed wrappers in this milestone.

**Tech Stack:** Rust 1.93.1, `syn 2`/`quote`/`proc-macro2`, **`borsh = "1"` (new)**, `sha2 = "0.10"` (already in macro since D1); Lean unchanged.

---

## Conventions

- **Lean:** no source changes in M7a. Confirm `lake build` stays green + zero-sorry as a regression check at the end.
- **Rust:** mandatory full gate (per HANDOVER) — rebuild every `.so` + `cargo test --workspace` with SBF tools + elan on PATH after each migration task.
- **SBF recipe** (unchanged):
  ```bash
  export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
  cd rust/<program-crate> && cargo-build-sbf --no-rustup-override
  ```
- **Inventory stays BPF-gated** (M5 sanity fix invariant). Nothing in M7a changes the gating.
- **The transitional window:** Tasks M1a → MIG5 keep the macro accepting BOTH bare `u8` and typed wrappers, so existing tests stay green during the migration. Task M1b removes the transitional u8 acceptance at the end.
- Commit after each task; `.gitignore` already covers `target/`, `lean/.lake/`.

---

## File structure

| File | Responsibility |
|------|----------------|
| `rust/verified-anchor/Cargo.toml` | (MODIFY) + `borsh = "1"` |
| `rust/verified-anchor/src/account_data.rs` | (NEW) `AccountData` trait, `ProgramId` trait, `System` marker |
| `rust/verified-anchor/src/account.rs` | (NEW) the 6 wrapper structs + `Deref`/`DerefMut` for `Account<T>` |
| `rust/verified-anchor/src/context.rs` | (NEW) `Context<'a, 'b, 'c, 'info, T>` |
| `rust/verified-anchor/src/lib.rs` | (MODIFY) add `mod`s + `Accounts<'info>` trait + new `VAError::BorshFailed` variant |
| `rust/verified-anchor/src/prelude.rs` | (NEW) re-exports (`Account, Signer, Program, SystemAccount, UncheckedAccount, AccountInfo, Context, VerifiedAccounts, AccountData (trait+derive), ProgramId, System`) |
| `rust/verified-anchor-macros/src/lib.rs` | (MAJOR REWRITE) `WrapperKind` recognition from `syn::Type`; per-wrapper validate codegen; per-wrapper Accounts codegen; `lean_spec` real type names + `crate::ID` |
| `rust/verified-anchor-macros/src/account_data_derive.rs` | (NEW) `#[derive(AccountData)]` proc-macro — derive `BorshDeserialize`+`BorshSerialize`, compute `DISCRIMINATOR` via sha256 |
| `rust/verified-anchor-macros/tests/ui/bare_u8.rs` (+ `.stderr`) | (NEW) `trybuild` fixture asserting bare-`u8` rejected (added in M1b) |
| Migration: `rust/verified-anchor-program/src/lib.rs` | (MODIFY) 3 structs |
| Migration: `rust/verified-anchor-example/src/lib.rs` | (MODIFY) 3 structs |
| Migration: `rust/verified-anchor-exploits/src/lib.rs` | (MODIFY) 4 structs + NEW `#[derive(AccountData)]` on `Collateral`/`Vault`/`Config` |
| Migration: `rust/verified-anchor/tests/behavior.rs` | (MODIFY) ~7 structs |
| Migration: `rust/verified-anchor/tests/lean_spec.rs` | (MODIFY) ~4 structs |
| Migration: `rust/verified-anchor-macros/tests/ui/unsupported_constraint.rs` (+ `.stderr`) | (MODIFY) `Bad` to typed wrapper form |
| `docs/migrating-from-anchor.md` | (MODIFY) update to typed-wrapper syntax (near-1:1 with stock Anchor) |
| `docs/verified-anchor-bridge.md` | (MODIFY) one-paragraph addendum: `Accounts` sits on top of `Validate`; proven layer unchanged |
| `docs/superpowers/m3-followups.md` | (MODIFY) check off "lean_spec_string hardcodes Vault" |

---

# PART L — Library types (set up the public API surface first)

## Task L1: `borsh` dep + `account_data.rs` (`AccountData`/`ProgramId` traits + `System` marker)

**Files:** Modify `rust/verified-anchor/Cargo.toml`; create `rust/verified-anchor/src/account_data.rs`.

- [ ] **Step 1: Add the `borsh` dependency**

In `rust/verified-anchor/Cargo.toml`, under `[dependencies]`, add:
```toml
borsh = "1"
```

- [ ] **Step 2: Create `account_data.rs`**

Create `rust/verified-anchor/src/account_data.rs`:
```rust
//! Traits that user types carry into typed-account wrappers.

use solana_program::pubkey::Pubkey;

/// Anchor-compatible account-data trait. The derive `#[derive(AccountData)]`
/// implements this and the underlying Borsh traits; the `DISCRIMINATOR` is
/// `sha256(b"account:" ++ <TypeName>)[0..8]` — the real Anchor wire format.
pub trait AccountData: borsh::BorshDeserialize + borsh::BorshSerialize {
    const DISCRIMINATOR: [u8; 8];
}

/// A marker for a Solana program, providing its on-chain id. Carried by
/// `Program<'info, P>` so the wrapper can check `accounts[i].key == &P::ID`.
pub trait ProgramId {
    const ID: Pubkey;
}

/// Marker for the System Program. Used as `Program<'info, System>` so the
/// wrapper auto-checks `accounts[i].key == solana_program::system_program::ID`.
pub struct System;
impl ProgramId for System {
    const ID: Pubkey = solana_program::system_program::ID;
}
```

- [ ] **Step 3: Wire `account_data` module into `lib.rs`**

In `rust/verified-anchor/src/lib.rs`, near the top (after the `use` lines), add:
```rust
pub mod account_data;
pub use account_data::{AccountData, ProgramId, System};
```

- [ ] **Step 4: Build**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo build -p verified-anchor 2>&1 | tail -3` — expect success.

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor/Cargo.toml rust/verified-anchor/src/
git commit -m "feat(verified-anchor): AccountData + ProgramId traits + System marker; + borsh dep"
```

---

## Task L2: `account.rs` — the 6 wrapper structs

**Files:** Create `rust/verified-anchor/src/account.rs`; modify `rust/verified-anchor/src/lib.rs`.

- [ ] **Step 1: Create `account.rs`**

Create `rust/verified-anchor/src/account.rs`:
```rust
//! The six typed wrappers the M7a macro recognises. Each is a thin marker over
//! `&'info AccountInfo<'info>`; `Account<'info, T>` additionally carries the
//! Borsh-deserialised T (the macro fills it in `try_accounts`).

use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use solana_program::account_info::AccountInfo;

use crate::account_data::{AccountData, ProgramId};

/// `Account<'info, T>` — Anchor-style typed account. The macro auto-implies
/// owner=crate::ID + discriminator=T::DISCRIMINATOR in validate, and
/// Borsh-deserialises T in try_accounts (skipping the 8-byte discriminator).
pub struct Account<'info, T: AccountData> {
    pub info: &'info AccountInfo<'info>,
    pub data: T,
}

impl<'info, T: AccountData> Deref for Account<'info, T> {
    type Target = T;
    fn deref(&self) -> &T { &self.data }
}
impl<'info, T: AccountData> DerefMut for Account<'info, T> {
    fn deref_mut(&mut self) -> &mut T { &mut self.data }
}

/// `Signer<'info>` — auto-implies `is_signer == true`.
pub struct Signer<'info> {
    pub info: &'info AccountInfo<'info>,
}

/// `Program<'info, P>` — auto-implies `executable == true` AND `info.key == P::ID`.
pub struct Program<'info, P: ProgramId> {
    pub info: &'info AccountInfo<'info>,
    _phantom: PhantomData<P>,
}
impl<'info, P: ProgramId> Program<'info, P> {
    /// Constructed by the macro after the wrapper checks pass.
    pub fn new(info: &'info AccountInfo<'info>) -> Self {
        Self { info, _phantom: PhantomData }
    }
}

/// `SystemAccount<'info>` — auto-implies `info.owner == system_program::ID`.
pub struct SystemAccount<'info> {
    pub info: &'info AccountInfo<'info>,
}

/// `UncheckedAccount<'info>` — escape hatch; no implied checks (explicit
/// `#[account(...)]` attributes still apply).
pub struct UncheckedAccount<'info> {
    pub info: &'info AccountInfo<'info>,
}

// `AccountInfo<'info>` is the raw Solana type — re-exported from prelude as-is;
// no wrapper struct here.
```

- [ ] **Step 2: Wire `account` module into `lib.rs`**

In `rust/verified-anchor/src/lib.rs`, near the other `pub mod` line you added in L1, add:
```rust
pub mod account;
pub use account::{Account, Signer, Program, SystemAccount, UncheckedAccount};
```

- [ ] **Step 3: Build**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo build -p verified-anchor 2>&1 | tail -3` — expect success.

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor/src/account.rs rust/verified-anchor/src/lib.rs
git commit -m "feat(verified-anchor): 6 typed wrappers (Account, Signer, Program, SystemAccount, UncheckedAccount, AccountInfo)"
```

---

## Task L3: `context.rs` + `Accounts<'info>` trait + `prelude.rs` + `VAError::BorshFailed`

**Files:** Create `rust/verified-anchor/src/context.rs`, `rust/verified-anchor/src/prelude.rs`; modify `rust/verified-anchor/src/lib.rs`.

- [ ] **Step 1: Create `context.rs`**

Create `rust/verified-anchor/src/context.rs`:
```rust
//! `Context<'a, 'b, 'c, 'info, T>` — mirrors stock Anchor's signature so a
//! verified-anchor instruction handler is type-identical to a stock-Anchor one.

use core::marker::PhantomData;
use solana_program::{account_info::AccountInfo, pubkey::Pubkey};

use crate::Accounts;

pub struct Context<'a, 'b, 'c, 'info, T: Accounts<'info>> {
    pub accounts: T,
    pub program_id: &'a Pubkey,
    pub remaining_accounts: &'c [AccountInfo<'info>],
    pub bumps: T::Bumps,
    _phantom: PhantomData<&'b ()>,
}

impl<'a, 'b, 'c, 'info, T: Accounts<'info>> Context<'a, 'b, 'c, 'info, T> {
    pub fn new(
        program_id: &'a Pubkey,
        accounts: T,
        remaining_accounts: &'c [AccountInfo<'info>],
        bumps: T::Bumps,
    ) -> Self {
        Self { accounts, program_id, remaining_accounts, bumps, _phantom: PhantomData }
    }
}
```

- [ ] **Step 2: Add `Accounts<'info>` trait + `BorshFailed` to `lib.rs`**

In `rust/verified-anchor/src/lib.rs`, add the new trait (place it BELOW the existing `Validate` trait) and the new `VAError` variant:
```rust
/// THE DEVELOPER SURFACE (M7a). `try_accounts` calls `Self::validate`
/// (the proven layer) and Borsh-deserialises each `Account<T>` field.
pub trait Accounts<'info>: Sized {
    type Bumps;
    fn try_accounts(
        program_id: &Pubkey,
        accounts: &'info [AccountInfo<'info>],
        instr_data: &[u8],
    ) -> Result<Self, VAError>;
}
```
Add to `enum VAError` + the `Display` match:
```rust
    BorshFailed { field: &'static str },
```
```rust
            VAError::BorshFailed { field } => write!(f, "Borsh deserialization failed for `{field}`"),
```

- [ ] **Step 3: Wire `context` module + add `prelude.rs`**

In `rust/verified-anchor/src/lib.rs`, add:
```rust
pub mod context;
pub use context::Context;

pub mod prelude;
```
Create `rust/verified-anchor/src/prelude.rs`:
```rust
//! Recommended `use verified_anchor::prelude::*;` — gives users the typed
//! wrappers, traits, `Context`, and the derive macros in one import.
pub use crate::{
    Account, Accounts, AccountData, AccountInfo, Context, Program, ProgramId,
    Signer, System, SystemAccount, UncheckedAccount, VAError, Validate, VerifiedAccounts,
};
// The AccountData derive proc-macro is re-exported once it exists (Task D1).
// pub use verified_anchor_macros::AccountData;
```
The `AccountInfo` re-export needs a `use` line at the TOP of `prelude.rs`:
```rust
use solana_program::account_info::AccountInfo;
```
Actually — since `AccountInfo` is from `solana_program`, write it as:
```rust
pub use solana_program::account_info::AccountInfo;
```
in the `pub use` block.

- [ ] **Step 4: Build**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo build -p verified-anchor 2>&1 | tail -3` — expect success.

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor/src/
git commit -m "feat(verified-anchor): Accounts<'info> trait + Context<T> + prelude + BorshFailed"
```

---

# PART D — `AccountData` derive proc-macro

## Task D1: `#[derive(AccountData)]` — sha256 discriminator + Borsh derive

**Files:** Create `rust/verified-anchor-macros/src/account_data_derive.rs`; modify `rust/verified-anchor-macros/src/lib.rs`, `rust/verified-anchor/tests/behavior.rs`.

- [ ] **Step 1: Write the failing test (TDD)**

In `rust/verified-anchor/tests/behavior.rs`, append:
```rust
use verified_anchor::AccountData;

#[derive(verified_anchor_macros::AccountData)]
struct Vault2 {
    pub authority: solana_program::pubkey::Pubkey,
    pub amount: u64,
}

#[test]
fn account_data_derive_computes_anchor_discriminator() {
    let expected = disc("Vault2"); // disc() from the M6 D1 helper already in behavior.rs
    assert_eq!(<Vault2 as AccountData>::DISCRIMINATOR, expected);
    // also confirm Borsh is derived (round-trips the bytes)
    let v = Vault2 { authority: solana_program::pubkey::Pubkey::new_from_array([7u8; 32]), amount: 42 };
    let bytes = borsh::to_vec(&v).unwrap();
    let v2: Vault2 = borsh::from_slice(&bytes).unwrap();
    assert_eq!(v2.amount, 42);
}
```
NOTE: `borsh` is a dev-dep of verified-anchor only after L1's edit; if the test build complains, add `borsh = "1"` under `[dev-dependencies]` in `verified-anchor/Cargo.toml` (in addition to the L1 normal dep).

Run `cd rust && cargo test -p verified-anchor --test behavior account_data 2>&1 | tail` — expect FAIL (`AccountData` derive doesn't exist).

- [ ] **Step 2: Create the derive proc-macro**

Create `rust/verified-anchor-macros/src/account_data_derive.rs`:
```rust
//! `#[derive(AccountData)]`: derive Borsh + compute the Anchor-wire DISCRIMINATOR.

use proc_macro::TokenStream;
use quote::quote;
use sha2::{Digest, Sha256};
use syn::{parse_macro_input, DeriveInput};

pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let name_str = name.to_string();
    let mut h = Sha256::new();
    h.update(b"account:");
    h.update(name_str.as_bytes());
    let out = h.finalize();
    let bs: Vec<u8> = out[..8].to_vec();
    let expanded = quote! {
        impl ::borsh::BorshSerialize for #name where #name: ::borsh::BorshSerialize {}
        impl ::borsh::BorshDeserialize for #name where #name: ::borsh::BorshDeserialize {}
        impl ::verified_anchor::AccountData for #name {
            const DISCRIMINATOR: [u8; 8] = [#(#bs),*];
        }
    };
    expanded.into()
}
```
WAIT — that `impl BorshSerialize for T where T: BorshSerialize` is a no-op (and is logically wrong as a derive). The user's `#[derive(AccountData)]` must ALSO derive Borsh. The cleanest way: have our derive RE-EMIT `#[derive(BorshSerialize, BorshDeserialize)]` as part of the expansion. But you can't derive on an item from within a derive proc-macro (proc-macros expand and the result is attached; further derives on the same item have already run or not). So the practical approach is: tell users to ALSO derive Borsh themselves, OR our derive does its job (DISCRIMINATOR const) and Borsh derives are written by the user.

Adjust: the derive emits ONLY `impl AccountData for Name { const DISCRIMINATOR ... }`. The user writes:
```rust
#[derive(borsh::BorshSerialize, borsh::BorshDeserialize, verified_anchor_macros::AccountData)]
struct Vault { ... }
```
That's 3 derives but matches how Anchor itself works (Anchor's `#[account]` attribute macro adds Borsh derives implicitly — we'd need an attribute macro for the same brevity, M7b polish). For M7a's first cut, require all three derives explicitly. Document in migration guide.

REVISED `account_data_derive.rs`:
```rust
//! `#[derive(AccountData)]`: compute Anchor-wire DISCRIMINATOR. The user also
//! derives `BorshSerialize`/`BorshDeserialize` separately (Anchor's `#[account]`
//! attribute macro that bundles these is an M7b polish item).
use proc_macro::TokenStream;
use quote::quote;
use sha2::{Digest, Sha256};
use syn::{parse_macro_input, DeriveInput};

pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let mut h = Sha256::new();
    h.update(b"account:");
    h.update(name.to_string().as_bytes());
    let out = h.finalize();
    let bs: Vec<u8> = out[..8].to_vec();
    quote! {
        impl ::verified_anchor::AccountData for #name {
            const DISCRIMINATOR: [u8; 8] = [#(#bs),*];
        }
    }.into()
}
```

Wire it in `rust/verified-anchor-macros/src/lib.rs` — at the top add:
```rust
mod account_data_derive;

#[proc_macro_derive(AccountData)]
pub fn derive_account_data(input: TokenStream) -> TokenStream {
    account_data_derive::derive(input)
}
```

Update the failing test in behavior.rs to derive Borsh explicitly:
```rust
#[derive(borsh::BorshSerialize, borsh::BorshDeserialize, verified_anchor_macros::AccountData)]
struct Vault2 {
    pub authority: solana_program::pubkey::Pubkey,
    pub amount: u64,
}
```

- [ ] **Step 3: Run the test**

Run `cd rust && cargo test -p verified-anchor --test behavior account_data 2>&1 | tail` — expect PASS.

- [ ] **Step 4: Uncomment the prelude re-export**

In `rust/verified-anchor/src/prelude.rs`, uncomment the line added in L3:
```rust
pub use verified_anchor_macros::AccountData;
```
Build: `cd rust && cargo build -p verified-anchor 2>&1 | tail -3` — expect success.

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-macros/ rust/verified-anchor/tests/behavior.rs rust/verified-anchor/src/prelude.rs
git commit -m "feat(macros): #[derive(AccountData)] - sha256 DISCRIMINATOR matching real Anchor"
```

---

# PART M — Main macro rewrite (transitional → wrapper-aware → strict)

## Task M1a: Field-type parsing (transitional — both u8 and wrappers accepted)

**Files:** Modify `rust/verified-anchor-macros/src/lib.rs`.

The current macro reads each field's `#[account(...)]` attributes and ignores the field type (always `u8`). M1a adds **field-type recognition** so the macro can tell `Account<'info, Vault>` from `u8`. Bare `u8` still works (transitional). M1b removes that at the end.

- [ ] **Step 1: Add the `WrapperKind` enum**

In `rust/verified-anchor-macros/src/lib.rs`, near the top (after `use` and `enum SeedElem`), add:
```rust
/// Recognised field-type wrapper categories.
#[derive(Clone)]
enum WrapperKind {
    /// `Account<'info, T>` — type name is the inner T's ident.
    Account(syn::Ident),
    /// `Signer<'info>`.
    Signer,
    /// `Program<'info, P>` — type name is P.
    Program(syn::Ident),
    /// `SystemAccount<'info>`.
    SystemAccount,
    /// `UncheckedAccount<'info>` or `AccountInfo<'info>`.
    Unchecked,
    /// Bare `u8` (transitional, removed in M1b).
    BareU8,
}

/// Recognise a field's type as a wrapper. Returns `BareU8` for `u8` (transitional)
/// and an error span otherwise.
fn classify_field_type(ty: &syn::Type) -> syn::Result<WrapperKind> {
    use syn::{PathArguments, Type, TypePath};
    // Bare `u8`
    if let Type::Path(TypePath { qself: None, path }) = ty {
        if path.is_ident("u8") {
            return Ok(WrapperKind::BareU8);
        }
        // Walk a path like `Account<'info, T>` or `solana_program::account_info::AccountInfo<'info>`.
        let last = path.segments.last().ok_or_else(||
            syn::Error::new_spanned(ty, "verified-anchor: unrecognised field type"))?;
        let ident_str = last.ident.to_string();
        match ident_str.as_str() {
            "Account" => {
                if let PathArguments::AngleBracketed(args) = &last.arguments {
                    for ga in &args.args {
                        if let syn::GenericArgument::Type(Type::Path(TypePath { qself: None, path: p })) = ga {
                            if let Some(seg) = p.segments.last() {
                                return Ok(WrapperKind::Account(seg.ident.clone()));
                            }
                        }
                    }
                }
                Err(syn::Error::new_spanned(ty, "Account<'info, T> requires a type argument"))
            }
            "Signer" => Ok(WrapperKind::Signer),
            "SystemAccount" => Ok(WrapperKind::SystemAccount),
            "UncheckedAccount" | "AccountInfo" => Ok(WrapperKind::Unchecked),
            "Program" => {
                if let PathArguments::AngleBracketed(args) = &last.arguments {
                    for ga in &args.args {
                        if let syn::GenericArgument::Type(Type::Path(TypePath { qself: None, path: p })) = ga {
                            if let Some(seg) = p.segments.last() {
                                return Ok(WrapperKind::Program(seg.ident.clone()));
                            }
                        }
                    }
                }
                Err(syn::Error::new_spanned(ty, "Program<'info, P> requires a type argument"))
            }
            _ => Err(syn::Error::new_spanned(ty,
                format!("verified-anchor: unrecognised field wrapper `{ident_str}`; use one of Account<'info, T>, Signer<'info>, Program<'info, P>, SystemAccount<'info>, UncheckedAccount<'info>, AccountInfo<'info>"))),
        }
    } else {
        Err(syn::Error::new_spanned(ty, "verified-anchor: unrecognised field type"))
    }
}
```

- [ ] **Step 2: Extend `FieldSpec` with the recognised kind**

Replace the existing `struct FieldSpec` block:
```rust
struct FieldSpec {
    name: String,
    constraints: Vec<Constraint>,
}
```
with:
```rust
struct FieldSpec {
    name: String,
    constraints: Vec<Constraint>,
    kind: WrapperKind,
}
```
In `collect_fields`, parse the type too. Inside the `for field in &named.named` loop, after computing `name`/`constraints`, add:
```rust
        let kind = classify_field_type(&field.ty)?;
```
And construct: `specs.push(FieldSpec { name, constraints, kind });`.

- [ ] **Step 3: Build**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo build -p verified-anchor-macros 2>&1 | tail -3` — expect success. Existing tests still pass (all current structs use bare `u8`, classified as `BareU8`; downstream codegen unchanged for bare-u8 paths).

Run also: `cargo test -p verified-anchor --test behavior --test lean_spec 2>&1 | grep "test result"` — expect existing tests still pass.

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-macros/src/lib.rs
git commit -m "feat(macros): field-type recognition (transitional - bare u8 still accepted)"
```

---

## Task M2: validate_body — per-wrapper auto-implied checks + lean_spec real type names

**Files:** Modify `rust/verified-anchor-macros/src/lib.rs`.

For each non-`BareU8` field kind, prepend the wrapper's auto-implied constraints to the per-field constraint list BEFORE emitting the runtime checks. The codegen for each constraint kind already exists (D1's discriminator, M2's owner, M3's signer, M4's seeds/bump, M2's mut, etc.). All we do here is synthesize the right `Constraint::...` values from `WrapperKind`.

- [ ] **Step 1: Synthesize wrapper-implied constraints in `validate_body`**

In `rust/verified-anchor-macros/src/lib.rs`, in `validate_body`, the per-field loop currently iterates `spec.constraints` directly:
```rust
        for c in &spec.constraints {
            ...
        }
```
Before that loop, build an `effective_constraints` list = wrapper-implied + explicit. Replace:
```rust
        for c in &spec.constraints {
```
with:
```rust
        let implied: Vec<Constraint> = wrapper_implied(&spec.kind);
        let effective: Vec<Constraint> = implied.into_iter().chain(spec.constraints.iter().cloned()).collect();
        for c in &effective {
```
where the new helper is:
```rust
/// The per-constraint implications of the field's wrapper kind.
fn wrapper_implied(kind: &WrapperKind) -> Vec<Constraint> {
    match kind {
        WrapperKind::Account(t) => {
            // owner = crate::ID + discriminator = sha256("account:" + t)
            let mut h = sha2::Sha256::new();
            h.update(b"account:");
            h.update(t.to_string().as_bytes());
            let out = h.finalize();
            let mut d = [0u8; 8];
            d.copy_from_slice(&out[..8]);
            vec![
                Constraint::Owner(syn::parse_quote! { crate::ID }),
                Constraint::Discriminator(d),
            ]
        }
        WrapperKind::Signer => vec![Constraint::Signer],
        WrapperKind::SystemAccount =>
            vec![Constraint::Owner(syn::parse_quote! { ::solana_program::system_program::ID })],
        WrapperKind::Program(p) => {
            // executable + key == P::ID — emit as a new compound constraint OR
            // two simple ones. The existing macro doesn't have an Executable
            // constraint, so we'll add it as part of the runtime check below.
            // For now, synthesise an Owner-like check via a marker constraint:
            vec![Constraint::ProgramMarker(p.clone())]
        }
        WrapperKind::Unchecked | WrapperKind::BareU8 => vec![],
    }
}
```
And add the new `Constraint::ProgramMarker(syn::Ident)` variant + its `validate_body` arm + a `lean_constraint` arm (treat as unchecked in lean_spec, since the Lean model has no `program` AccountType variant beyond `uncheckedAccount`):

In `enum Constraint`:
```rust
    ProgramMarker(syn::Ident),
```
In `validate_body`'s per-constraint match arm:
```rust
                Constraint::ProgramMarker(p) => {
                    let fname = name;
                    let pid_ty = p;
                    quote! {
                        if !accounts[#i].executable {
                            return Err(::verified_anchor::VAError::WrongOwner { field: #fname });
                        }
                        if accounts[#i].key != &<#pid_ty as ::verified_anchor::ProgramId>::ID {
                            return Err(::verified_anchor::VAError::WrongOwner { field: #fname });
                        }
                    }
                },
```
(Using `WrongOwner` as a reasonable best-fit existing error; a dedicated `WrongProgram` could be added later.)

In `lean_constraint`:
```rust
        Constraint::ProgramMarker(_) => String::new(),  // not a Lean constraint kind
```

- [ ] **Step 2: lean_spec real type name + crate::ID**

In `lean_spec_string`, the `ty` chooser currently hardcodes `"Vault"`. Replace it to use the real type name from the wrapper:
```rust
        let ty = match &spec.kind {
            WrapperKind::Account(t) => {
                // Layout: if has_one is present, use the existing layout-with-offset-8 form.
                let layout = spec.constraints.iter().find_map(|c| {
                    if let Constraint::HasOne(target) = c { Some(target.to_string()) } else { None }
                });
                let lay = match layout {
                    Some(target) => format!("[(\"{}\", 8)]", target),
                    None => "[]".to_string(),
                };
                format!("AccountType.account \"{}\" {} crate::ID", t, lay)
            }
            WrapperKind::Signer => "AccountType.signer".to_string(),
            WrapperKind::SystemAccount => "AccountType.systemAccount".to_string(),
            WrapperKind::Program(_) | WrapperKind::Unchecked | WrapperKind::BareU8 =>
                "AccountType.uncheckedAccount".to_string(),
        };
```
NOTE: emitting `crate::ID` works inside the user's crate context when the spec is interpolated into Lean — but wait, the lean_spec is a Lean string written to a file and consumed by `lake env lean`; Lean has no `crate::ID`. Use a Lean-valid placeholder: `Pubkey.zero` (matching the existing convention; `M4Subset` decide doesn't evaluate the program id):
```rust
                format!("AccountType.account \"{}\" {} Pubkey.zero", t, lay)
```
(The "real programId" idea in the spec is honest but doesn't translate to a Lean literal without a way to emit the program's id as bytes. Stick with `Pubkey.zero` — same as current — but the type name is now the REAL one. That's the M3 follow-up closure.)

For bare `u8` (transitional), keep the existing has_one fallback "Vault" hardcode so existing tests still pass during the transition window. Add at the top of the `ty` chooser:
```rust
        let ty = match &spec.kind {
            WrapperKind::BareU8 => {
                // transitional: preserve the M3 "Vault" hardcode for bare-u8 fields
                let ty_fallback = spec.constraints.iter().find_map(|c| {
                    if let Constraint::HasOne(t) = c { Some(t.to_string()) } else { None }
                }).map(|t| format!("AccountType.account \"Vault\" [(\"{}\", 8)] Pubkey.zero", t))
                  .unwrap_or_else(|| "AccountType.uncheckedAccount".to_string());
                ty_fallback
            }
            WrapperKind::Account(t) => { /* … as above … */ }
            // …
        };
```

- [ ] **Step 3: Run all existing tests**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test behavior --test lean_spec 2>&1 | grep "test result"` — all existing tests still pass (bare-u8 paths unchanged; new wrappers not yet exercised by tests but compile).

Also `cargo build -p verified-anchor-program -p verified-anchor-example -p verified-anchor-exploits 2>&1 | tail -3` — existing programs build (still bare-u8).

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-macros/src/lib.rs
git commit -m "feat(macros): per-wrapper auto-implied checks in validate; lean_spec real type names (closes M3 followup)"
```

---

## Task M3: `Accounts<'info>` impl codegen — try_accounts + Bumps

**Files:** Modify `rust/verified-anchor-macros/src/lib.rs`.

Emit a `impl<'info> Accounts<'info> for #name<'info>` block alongside the existing `Validate` impl. The `try_accounts` body calls `validate` first (the proven gate), then constructs `Self` by populating each field from `accounts[i]` according to its wrapper kind.

- [ ] **Step 1: Emit the Accounts impl**

In `rust/verified-anchor-macros/src/lib.rs`, in `derive_verified_accounts`, after the existing `impl ::verified_anchor::Validate for #name` block in the `expanded = quote! { ... }`, add an `impl Accounts<'info>` block. The field initialisers depend on the wrapper:

Replace the existing `expanded = quote! { ... }` with:
```rust
    let n = specs.len();
    let bumps_struct_name = syn::Ident::new(&format!("{}Bumps", name), name.span());
    let field_inits: Vec<TokenStream2> = specs.iter().enumerate().map(|(i, spec)| {
        let fname = syn::Ident::new(&spec.name, name.span());
        match &spec.kind {
            WrapperKind::Account(t) => quote! {
                #fname: ::verified_anchor::Account {
                    info: &accounts[#i],
                    data: <#t as ::borsh::BorshDeserialize>::try_from_slice(
                        &accounts[#i].data.borrow()[8..]
                    ).map_err(|_| ::verified_anchor::VAError::BorshFailed { field: stringify!(#fname) })?,
                }
            },
            WrapperKind::Signer => quote! {
                #fname: ::verified_anchor::Signer { info: &accounts[#i] }
            },
            WrapperKind::Program(p) => quote! {
                #fname: ::verified_anchor::Program::<'info, #p>::new(&accounts[#i])
            },
            WrapperKind::SystemAccount => quote! {
                #fname: ::verified_anchor::SystemAccount { info: &accounts[#i] }
            },
            WrapperKind::Unchecked => quote! {
                #fname: ::verified_anchor::UncheckedAccount { info: &accounts[#i] }
            },
            WrapperKind::BareU8 => quote! {
                #fname: 0u8   // transitional; will be removed in M1b
            },
        }
    }).collect();
    let expanded = quote! {
        impl<'info> ::verified_anchor::Validate for #name<'info> {
            #body
        }
        impl #name<'_> {
            pub fn lean_spec() -> ::std::string::String {
                #lean.to_string()
            }
            #lifecycle
        }
        pub struct #bumps_struct_name;
        impl<'info> ::verified_anchor::Accounts<'info> for #name<'info> {
            type Bumps = #bumps_struct_name;
            fn try_accounts(
                program_id: &::solana_program::pubkey::Pubkey,
                accounts: &'info [::solana_program::account_info::AccountInfo<'info>],
                instr_data: &[u8],
            ) -> ::core::result::Result<Self, ::verified_anchor::VAError> {
                <Self as ::verified_anchor::Validate>::validate(accounts, instr_data, program_id)?;
                ::core::result::Result::Ok(Self { #(#field_inits),* })
            }
        }
        #[cfg(not(target_os = "solana"))]
        ::verified_anchor::inventory::submit! {
            ::verified_anchor::SpecEntry {
                name: #name_str,
                lean_spec: #name::lean_spec,
                has_lifecycle: #has_lifecycle,
            }
        }
    };
```
NOTE: the struct's `#name` now uses `#name<'info>` in trait impls; bare `#name` in `lean_spec`/lifecycle uses `<'_>` (anonymous lifetime). This requires the struct DEFINITION to carry `<'info>` — which the migration tasks will add to each struct.

For BareU8 (transitional), the field-init produces `0u8` so `Self { vault: 0u8, … }` still constructs. NOT correct semantically (we're returning a Self whose u8 fields are 0), but it compiles, and existing tests only call `Validate::validate` (not `try_accounts`), so the dummy is never observed. M1b removes BareU8.

- [ ] **Step 2: Build (existing programs should still compile pre-migration)**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo build --workspace 2>&1 | tail -5` — expect success.
NOTE: many derived structs currently have NO `'info` lifetime parameter (they have `u8` fields). The macro emits `impl<'info> Validate for #name<'info>` — this REQUIRES the struct to have a lifetime parameter. For existing bare-u8 structs the emitted `#name<'info>` will fail to compile.

Solution: the macro detects whether ANY field is a non-BareU8 wrapper; if NOT, it emits `impl Validate for #name` (no lifetime) and `impl<'info> Accounts<'info> for #name` (lifetime only on the trait). 

Update the codegen logic:
```rust
    let has_info = specs.iter().any(|s| !matches!(s.kind, WrapperKind::BareU8));
    let validate_impl = if has_info {
        quote! { impl<'info> ::verified_anchor::Validate for #name<'info> { #body } }
    } else {
        quote! { impl ::verified_anchor::Validate for #name { #body } }
    };
    let accounts_impl_target = if has_info { quote! { #name<'info> } } else { quote! { #name } };
    let lean_spec_impl_target = if has_info { quote! { #name<'_> } } else { quote! { #name } };
    let expanded = quote! {
        #validate_impl
        impl #lean_spec_impl_target {
            pub fn lean_spec() -> ::std::string::String { #lean.to_string() }
            #lifecycle
        }
        pub struct #bumps_struct_name;
        impl<'info> ::verified_anchor::Accounts<'info> for #accounts_impl_target {
            type Bumps = #bumps_struct_name;
            fn try_accounts(
                program_id: &::solana_program::pubkey::Pubkey,
                accounts: &'info [::solana_program::account_info::AccountInfo<'info>],
                instr_data: &[u8],
            ) -> ::core::result::Result<Self, ::verified_anchor::VAError> {
                <Self as ::verified_anchor::Validate>::validate(accounts, instr_data, program_id)?;
                ::core::result::Result::Ok(Self { #(#field_inits),* })
            }
        }
        #[cfg(not(target_os = "solana"))]
        ::verified_anchor::inventory::submit! {
            ::verified_anchor::SpecEntry {
                name: #name_str,
                lean_spec: <#lean_spec_impl_target>::lean_spec,
                has_lifecycle: #has_lifecycle,
            }
        }
    };
```

- [ ] **Step 3: Confirm the workspace builds + existing tests pass**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test behavior --test lean_spec 2>&1 | grep "test result"` — all existing tests still pass.

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-macros/src/lib.rs
git commit -m "feat(macros): Accounts<'info> impl codegen with try_accounts + Bumps assoc-type"
```

---

# PART MIG — migrate the ~22 existing structs to typed wrappers

> Per-migration approach: change each `#[derive(VerifiedAccounts)]` struct's field types to wrappers; drop the now-implied constraints; add `'info` lifetime parameter to the struct. Tests must still pass after each migration.

## Task MIG1: `verified-anchor-program` (3 structs)

**Files:** Modify `rust/verified-anchor-program/src/lib.rs`.

- [ ] **Step 1: Migrate `InitOne`, `CloseOne`, `CheckPda`**

Replace the existing struct definitions in `rust/verified-anchor-program/src/lib.rs`:
```rust
#[derive(VerifiedAccounts)]
struct InitOne<'info> {
    #[account(init, payer = payer, space = 0)]
    new: verified_anchor::UncheckedAccount<'info>,
    #[account(mut)]
    payer: verified_anchor::Signer<'info>,
    system_program: verified_anchor::Program<'info, verified_anchor::System>,
}

#[derive(VerifiedAccounts)]
struct CloseOne<'info> {
    #[account(close = dest)]
    target: verified_anchor::UncheckedAccount<'info>,
    #[account(mut)]
    dest: verified_anchor::UncheckedAccount<'info>,
}

#[derive(VerifiedAccounts)]
struct CheckPda<'info> {
    #[account(seeds = [b"vault", arg(0, 4)], bump)]
    pda: verified_anchor::UncheckedAccount<'info>,
}
```

- [ ] **Step 2: Native + SBF builds**

Run:
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo build -p verified-anchor-program 2>&1 | tail -3
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd rust/verified-anchor-program && cargo-build-sbf --no-rustup-override 2>&1 | grep -iE "PT_DYNAMIC|Finished" | tail
```
Expected: success; no PT_DYNAMIC warning.

- [ ] **Step 3: Re-run litesvm runtime tests (must pass — same on-chain behaviour)**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test runtime_lifecycle --test runtime_seeds 2>&1 | tail -10` — expect 2+2 = 4 pass.

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-program/src/lib.rs
git commit -m "refactor(program): migrate to typed wrappers (Signer, Program<System>, UncheckedAccount)"
```

---

## Task MIG2: `verified-anchor-example` (3 structs)

**Files:** Modify `rust/verified-anchor-example/src/lib.rs`.

- [ ] **Step 1: Migrate `CheckPda`, `Transfer`, `Lifecycle`**

Replace the existing struct definitions:
```rust
#[derive(VerifiedAccounts)]
pub struct CheckPda<'info> {
    #[account(seeds = [b"vault", arg(0, 4)], bump)]
    pub pda: verified_anchor::UncheckedAccount<'info>,
}

#[derive(VerifiedAccounts)]
pub struct Transfer<'info> {
    #[account(mut)]
    pub vault: verified_anchor::UncheckedAccount<'info>,
    pub authority: verified_anchor::Signer<'info>,
}

#[derive(VerifiedAccounts)]
pub struct Lifecycle<'info> {
    #[account(init, payer = payer, space = 0)]
    pub new_acct: verified_anchor::UncheckedAccount<'info>,
    #[account(mut)]
    pub payer: verified_anchor::Signer<'info>,
    #[account(close = payer)]
    pub old_acct: verified_anchor::UncheckedAccount<'info>,
}
```

- [ ] **Step 2: Native build + M5 check**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo build -p verified-anchor-example 2>&1 | tail -3
export PATH="$HOME/.elan/bin:$PATH"
cargo run -q -p cargo-verified-anchor -- verified-anchor check -p verified-anchor-example --lean-dir ../lean ; echo "EXIT $?"
```
Expected: build green; check exits 0 with 3 ✓ lines.

- [ ] **Step 3: Run the CLI e2e test**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p cargo-verified-anchor --test cli 2>&1 | tail` — expect PASS.

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-example/src/lib.rs
git commit -m "refactor(example): migrate to typed wrappers; CLI e2e still passes"
```

---

## Task MIG3: `verified-anchor-exploits` (4 verified structs + 3 AccountData user types)

**Files:** Modify `rust/verified-anchor-exploits/src/lib.rs`.

- [ ] **Step 1: Add user-type declarations + migrate verified structs**

In `rust/verified-anchor-exploits/src/lib.rs`, near the top (after `use` and `declare_id!`), add the typed account types:
```rust
use borsh::{BorshDeserialize, BorshSerialize};

#[derive(BorshSerialize, BorshDeserialize, verified_anchor::AccountData)]
pub struct Collateral {
    pub bank: solana_program::pubkey::Pubkey,
    pub amount: u64,
}

#[derive(BorshSerialize, BorshDeserialize, verified_anchor::AccountData)]
pub struct Vault {
    pub authority: solana_program::pubkey::Pubkey,
}

#[derive(BorshSerialize, BorshDeserialize, verified_anchor::AccountData)]
pub struct Config {
    pub field: solana_program::pubkey::Pubkey,
}
```
Add `borsh = "1"` to `rust/verified-anchor-exploits/Cargo.toml` `[dependencies]`.

Then replace the four `Verified*` structs:
```rust
#[derive(VerifiedAccounts)]
pub struct VerifiedCashio<'info> {
    #[account(has_one = bank)]
    pub collateral: verified_anchor::Account<'info, Collateral>,
    pub bank: verified_anchor::UncheckedAccount<'info>,
    #[account(mut)]
    pub out: verified_anchor::UncheckedAccount<'info>,
}

#[derive(VerifiedAccounts)]
pub struct VerifiedConfusion<'info> {
    pub vault: verified_anchor::Account<'info, Vault>,
    pub authority: verified_anchor::Signer<'info>,
    #[account(mut)]
    pub out: verified_anchor::UncheckedAccount<'info>,
}

#[derive(VerifiedAccounts)]
pub struct VerifiedCrema<'info> {
    #[account(owner = AMM_PROG)]
    pub price: verified_anchor::UncheckedAccount<'info>,
    #[account(mut)]
    pub out: verified_anchor::UncheckedAccount<'info>,
}

#[derive(VerifiedAccounts)]
pub struct VerifiedSeeds<'info> {
    pub user: verified_anchor::Signer<'info>,
    #[account(seeds = [b"vault", user.key()], bump)]
    pub vault: verified_anchor::UncheckedAccount<'info>,
    #[account(mut)]
    pub out: verified_anchor::UncheckedAccount<'info>,
}
```
NOTES:
- `VerifiedCashio.collateral` was `#[account(owner = crate::ID, discriminator = "Collateral", has_one = bank)]` — `Account<'info, Collateral>` auto-implies owner+disc, so only `has_one = bank` is explicit. Much cleaner.
- `VerifiedConfusion.vault` was `#[account(discriminator = "Vault")]` — `Account<'info, Vault>` auto-implies. Cleaner.
- `VerifiedCrema.price` uses `UncheckedAccount` + explicit `owner = AMM_PROG` because the owner is NOT `crate::ID` (so `Account<T>` doesn't fit).
- `VerifiedSeeds` doesn't use `Account<T>` because the PDA carries no typed data.

- [ ] **Step 2: SBF rebuild + runtime_exploits + M5 check**
```bash
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-exploits && cargo-build-sbf --no-rustup-override 2>&1 | grep -iE "PT_DYNAMIC|Finished" | tail
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test runtime_exploits 2>&1 | tail -10
export PATH="$HOME/.elan/bin:$PATH"
cargo run -q -p cargo-verified-anchor -- verified-anchor check -p verified-anchor-exploits --lean-dir ../lean ; echo "EXIT $?"
```
Expected: SBF clean (no PT_DYNAMIC); runtime_exploits 4 pass; M5 check exits 0 with 4 ✓ lines.

- [ ] **Step 3: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-exploits/
git commit -m "refactor(exploits): migrate to typed wrappers; add AccountData on Collateral/Vault/Config"
```

---

## Task MIG4: `tests/behavior.rs` + `tests/lean_spec.rs` migrations

**Files:** Modify `rust/verified-anchor/tests/behavior.rs`, `rust/verified-anchor/tests/lean_spec.rs`.

These are the largest single-file migrations (~11 derived structs across both). All structs migrate to typed wrappers; the `Acct` helper still produces `AccountInfo`, which the wrappers take by reference.

- [ ] **Step 1: Migrate `behavior.rs` structs**

In `rust/verified-anchor/tests/behavior.rs`, replace each `#[derive(VerifiedAccounts)]` struct:
- `Transfer { vault: u8 (mut), authority: u8 (signer) }` → `Transfer<'info> { vault: UncheckedAccount<'info> #[account(mut)], authority: Signer<'info> }`
- `OwnedVault { vault: u8 (owner = PROG_OWNER) }` → `OwnedVault<'info> { vault: UncheckedAccount<'info> #[account(owner = PROG_OWNER)] }`
- `CheckOwner { vault: u8 (has_one = authority), authority: u8 }` → `CheckOwner<'info> { vault: UncheckedAccount<'info> #[account(has_one = authority)], authority: UncheckedAccount<'info> }`
- `PdaAccount { pda: u8 (seeds, bump) }` → `PdaAccount<'info> { pda: UncheckedAccount<'info> #[account(seeds = [...], bump)] }`
- `PdaDeclaredBump { pda: u8 (seeds, bump = 0) }` → `PdaDeclaredBump<'info> { pda: UncheckedAccount<'info> #[account(seeds = [b"vault"], bump = 0)] }`
- `DiscOnly { vault: u8 (discriminator="Vault") }` → `DiscOnly<'info> { vault: UncheckedAccount<'info> #[account(discriminator = "Vault")] }` (keep the explicit `discriminator` since the wrapper is Unchecked — Account<Vault> would also work and is more Anchor-ish, but the existing tests want the explicit override path to keep working)
- `InitClose { new (init, payer=payer, space=0), payer (mut), old (close=payer) }` → `InitClose<'info> { new: UncheckedAccount<'info> #[account(init, payer = payer, space = 0)], payer: Signer<'info> #[account(mut)], old: UncheckedAccount<'info> #[account(close = payer)] }`
- `Vault2` (from D1, the AccountData derive test) is NOT a VerifiedAccounts struct — leave as-is.

Add `use verified_anchor::{Signer, UncheckedAccount};` near the top of `behavior.rs`.

The test bodies (calling `Type::validate(...)`) stay the same — the validate trait method doesn't change signature.

- [ ] **Step 2: Migrate `lean_spec.rs` structs**

Same pattern for `lean_spec.rs`:
- `Transfer` → typed version
- `PdaSpec` → typed UncheckedAccount with seeds
- `InitClose` → typed version
- `DiscSpec` → typed UncheckedAccount with explicit discriminator

The expected `lean_spec()` strings need updating because the AccountType emitted is now `AccountType.uncheckedAccount` (instead of e.g. `AccountType.account "Vault" [...] Pubkey.zero` for has_one structs). After migration, the `lean_spec_matches` test's `expected` will be wrong. Run the test, copy the actual emitted string from the failure diff, paste as new `expected` (the M5/M6 pattern — round-trip fidelity is the goal, not guessing whitespace).

- [ ] **Step 3: Run native suite**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test behavior --test lean_spec 2>&1 | grep "test result"` — expect all pass after the snapshot updates.

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor/tests/
git commit -m "refactor(tests): migrate behavior + lean_spec structs to typed wrappers"
```

---

## Task MIG5: `compile_fail` ui fixture migration

**Files:** Modify `rust/verified-anchor-macros/tests/ui/unsupported_constraint.rs`, `rust/verified-anchor-macros/tests/ui/unsupported_constraint.stderr`.

The existing fixture has `Bad { vault: u8 }`. After M1b will reject bare u8, but during MIG5 (transitional) the bare-u8 still works. We migrate the fixture to a typed wrapper anyway so it tests JUST the unsupported-constraint code path, not the bare-u8 one:

- [ ] **Step 1: Update the fixture**

Replace `rust/verified-anchor-macros/tests/ui/unsupported_constraint.rs`:
```rust
use verified_anchor::VerifiedAccounts;

#[derive(VerifiedAccounts)]
struct Bad<'info> {
    #[account(realloc = 8)]
    vault: verified_anchor::UncheckedAccount<'info>,
}

fn main() {}
```

- [ ] **Step 2: Refresh the stderr snapshot**

Run:
```bash
cd /home/parth/Desktop/PARTH/Verification/rust
TRYBUILD=overwrite cargo test -p verified-anchor-macros --test compile_fail 2>&1 | tail
cargo test -p verified-anchor-macros --test compile_fail 2>&1 | tail
```
Expected: first run rewrites `.stderr`; second run passes. The new stderr should still show the "realloc is a stock-Anchor constraint" message with `discriminator` in the supported list (unchanged).

- [ ] **Step 3: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-macros/tests/
git commit -m "refactor(macros/ui): migrate compile_fail Bad fixture to UncheckedAccount; refresh stderr"
```

---

# PART M (continued) — lock the migration

## Task M1b: Bare-`u8` becomes `compile_error!` (drop transitional path)

**Files:** Modify `rust/verified-anchor-macros/src/lib.rs`; create `rust/verified-anchor-macros/tests/ui/bare_u8.rs` (+ generated `.stderr`).

- [ ] **Step 1: Drop the transitional `BareU8` arm in `classify_field_type`**

In `rust/verified-anchor-macros/src/lib.rs`, in `classify_field_type`, replace the `if path.is_ident("u8") { return Ok(WrapperKind::BareU8); }` early-return with an error:
```rust
        if path.is_ident("u8") {
            return Err(syn::Error::new_spanned(ty,
                "verified-anchor: bare `u8` field types are not supported; use a typed wrapper like `Account<'info, T>`, `Signer<'info>`, `UncheckedAccount<'info>`, etc. See docs/migrating-from-anchor.md"));
        }
```

Drop the `WrapperKind::BareU8` arm from `wrapper_implied` (it produced `vec![]` — equivalent to removing the variant), and from `lean_spec_string`'s `ty` chooser (no longer needed), and from `field_inits` in `derive_verified_accounts`. Then DELETE the `BareU8` variant from `enum WrapperKind`.

Update `has_info` calculation — now ALL fields are non-BareU8, so `has_info = !specs.is_empty()`. Simplify:
```rust
    let has_info = !specs.is_empty();
```

- [ ] **Step 2: Add the trybuild fixture**

Create `rust/verified-anchor-macros/tests/ui/bare_u8.rs`:
```rust
use verified_anchor::VerifiedAccounts;

#[derive(VerifiedAccounts)]
struct OldStyle {
    vault: u8,
}

fn main() {}
```

- [ ] **Step 3: Generate the stderr snapshot**

Run:
```bash
cd /home/parth/Desktop/PARTH/Verification/rust
TRYBUILD=overwrite cargo test -p verified-anchor-macros --test compile_fail 2>&1 | tail
cargo test -p verified-anchor-macros --test compile_fail 2>&1 | tail
```
Open `rust/verified-anchor-macros/tests/ui/bare_u8.stderr` and confirm the message is the helpful one mentioning the migration guide.

- [ ] **Step 4: Full workspace test (everything still green after migration)**

Run:
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test --workspace 2>&1 | grep -E "Running |test result:"
```
Expected: every suite green; no failures (since MIG1-5 already migrated everything).

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-macros/
git commit -m "feat(macros): bare u8 is now a compile_error (transitional acceptance removed)"
```

---

# PART F — docs + final gate

## Task F1: docs update + mandatory full gate

**Files:** Modify `docs/migrating-from-anchor.md`, `docs/verified-anchor-bridge.md`, `docs/superpowers/m3-followups.md`.

- [ ] **Step 1: Update `docs/migrating-from-anchor.md`**

Add a new top section showing the now-near-1:1 syntax mapping:
```markdown
## Syntax mapping (M7a)

verified-anchor is now signature-identical to stock Anchor at the account-validation surface. A typical struct migrates field-for-field:

| Stock Anchor                              | verified-anchor (M7a)                                       |
|-------------------------------------------|-------------------------------------------------------------|
| `pub vault: Account<'info, Vault>`        | `pub vault: Account<'info, Vault>`                          |
| `pub authority: Signer<'info>`            | `pub authority: Signer<'info>`                              |
| `pub system_program: Program<'info, System>` | `pub system_program: Program<'info, System>`             |
| `#[account(init, payer = p, space = n)]`  | same                                                        |
| `#[account(has_one = bank)]`              | same                                                        |
| `#[account(seeds = [..], bump)]`          | same (canonical-only — see bridge)                          |
| `#[account]` on type T                    | `#[derive(BorshSerialize, BorshDeserialize, AccountData)]`  |

Plus: `use verified_anchor::prelude::*;` brings in everything (wrappers, traits, Context, derives).
```
Keep the existing "supported constraints" table and "boundaries" sections; add a one-paragraph note that bare `u8` field types are no longer supported (M7a).

- [ ] **Step 2: Update `docs/verified-anchor-bridge.md`**

Append a one-paragraph section (after the M5 section):
```markdown
## Developer surface (M7a)

The Rust→Lean proof chain is unchanged: the macro emits an `impl Validate` whose body is the same per-constraint check sequence (signer/mut/owner/has_one/seeds/discriminator) that `genValidate` models in Lean, with `M4Subset s → (genValidate s c = true ↔ validates s c)` proved generically. M7a adds an `Accounts<'info>` trait alongside `Validate`: its `try_accounts` calls `Self::validate` first (the proven gate), then Borsh-deserialises each `Account<'info, T>` field's data into the typed struct. Borsh deserialisation is outside the proven surface (a transcription concern, like the M3 CPI-effect modelling) — a `BorshFailed` error is honest runtime feedback, not a verification hole. The `lean_spec` emission now uses the real type name from `Account<'info, T>` (closing the M3 "Vault hardcode" follow-up). No Lean source changes; M1–M5 headline theorems' `#print axioms` unchanged.
```

- [ ] **Step 3: Check off the M3 follow-up**

In `docs/superpowers/m3-followups.md`, find the item:
```markdown
3. **`lean_spec_string` hardcodes the Lean type name `"Vault"`** for any `has_one` field
```
and prepend `✅ CLOSED in M7a — ` to it (or move it to a "Closed" sub-section if that's the file's convention).

- [ ] **Step 4: Run the MANDATORY full gate**

Run:
```bash
export PATH="$HOME/.elan/bin:$PATH"; cd /home/parth/Desktop/PARTH/Verification/lean && lake build 2>&1 | tail -1
grep -rn "sorry\|admit" VerifiedAnchor/ || echo "PASS lean zero-sorry"
printf 'import VerifiedAnchor\nimport VerifiedAnchor.Codegen.StructLifecycle\n#print axioms VerifiedAnchor.genValidate_sound\n#print axioms VerifiedAnchor.lifecycle_sound\n' > VerifiedAnchor/M7aAuditAx.lean
lake env lean VerifiedAnchor/M7aAuditAx.lean; rm -f VerifiedAnchor/M7aAuditAx.lean
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$HOME/.elan/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-program && cargo-build-sbf --no-rustup-override 2>&1 | grep -iE "PT_DYNAMIC|Finished" | tail -1
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-exploits && cargo-build-sbf --no-rustup-override 2>&1 | grep -iE "PT_DYNAMIC|Finished" | tail -1
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test --workspace 2>&1 | grep -E "Running |test result:"
cargo run -q -p cargo-verified-anchor -- verified-anchor check -p verified-anchor-example --lean-dir ../lean ; echo "EXIT $?"
cargo run -q -p cargo-verified-anchor -- verified-anchor check -p verified-anchor-exploits --lean-dir ../lean ; echo "EXIT $?"
```
Expected:
- Lean: `Build completed successfully (20 jobs).` + `PASS lean zero-sorry`; `genValidate_sound` + `lifecycle_sound` axioms `[propext, Quot.sound]`.
- Both SBF `.so`s `Finished release` with NO `PT_DYNAMIC`.
- Full workspace test suite green.
- Both M5 checks exit 0.

- [ ] **Step 5: Commit docs**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add docs/
git commit -m "docs(m7a): migration guide + bridge addendum; M3 'Vault hardcode' follow-up closed"
```

---

## Done-bar verification (after F1)

1. Macro accepts the 6 typed wrappers; bare `u8` is a `compile_error!` with the migration-guide message. ✅ (M1a, M3, M1b — last-mile in M1b)
2. `#[derive(AccountData)]` computes `DISCRIMINATOR = sha256("account:" + Name)[..8]` matching real Anchor; behavior test cross-checks. ✅ (D1)
3. Each derived struct gets BOTH `impl Validate` (proven, unchanged) AND `impl Accounts<'info>` (developer surface). ✅ (M3)
4. `Context<'a, 'b, 'c, 'info, T>` exists matching stock Anchor's signature. ✅ (L3)
5. All ~22 existing derived structs migrated; all M1–M6 tests pass (native + litesvm + M5 check + compile_fail); SBF `.so`s build clean. ✅ (MIG1–5, F1)
6. `lean_spec` emits real type names (no more "Vault" hardcode); M5 check discharges. ✅ (M2, F1)
7. `docs/migrating-from-anchor.md` updated; bridge addendum; m3-followups checkoff. ✅ (F1)
8. `lake build` green, zero `sorry`; M1–M5 headline theorems' `#print axioms` unchanged. ✅ (F1)
