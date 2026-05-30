# Verified Anchor — Milestone 7b Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make verified-anchor publishable on crates.io with a first-class developer surface — Anchor-style `#[account]` attribute macro, per-seed `Bumps`, dual-license + crates.io metadata + crates.io-ready README.

**Architecture:** Three independent additions in `rust/`: (A) a new `#[proc_macro_attribute]` bundling Borsh+AccountData derives; (B) extend the existing `#[derive(VerifiedAccounts)]` to emit a real `<Name>Bumps` struct with one `pub <f>: u8` per seeded field, and change `Accounts::try_accounts` to return `(Self, Self::Bumps)`; (C) workspace metadata + LICENSE files + README + publish checklist. No Lean changes. Mandatory full gate at the end (lake build + axioms + SBF + workspace tests + both M5 checks).

**Tech Stack:** Rust 1.93.1, `syn 2` / `quote` / `proc-macro2` / `sha2 0.10` (already deps). No new deps.

---

## Conventions

- Each `feat`/`refactor`/`docs` task ends in a single commit. Always rebuild + test before committing.
- The M7a mandatory full gate is restated in Task F1 — run after every task that touches `rust/`.
- The branch is `m7b-release` (create at the start; merge `--no-ff` to master at the end per HANDOVER convention).
- Lean is unchanged in M7b; **do not edit any file under `lean/`**.
- `.gitignore` already covers `target/`.

## File structure

| File | Action | Component |
|------|--------|-----------|
| `rust/verified-anchor-macros/src/account_attr.rs` | NEW | A |
| `rust/verified-anchor-macros/src/lib.rs` | MODIFY (wire attribute macro entry + Bumps codegen + tuple return) | A, B |
| `rust/verified-anchor/src/lib.rs` | MODIFY (Accounts trait return type, re-export `account` attr) | A, B |
| `rust/verified-anchor/src/prelude.rs` | MODIFY (re-export `account` attr) | A |
| `rust/verified-anchor/tests/behavior.rs` | MODIFY (3 new tests; unpack tuple in M7a try_accounts tests) | A, B |
| `rust/verified-anchor-macros/tests/ui/account_with_args.rs` (+`.stderr`) | NEW | A |
| `rust/verified-anchor/Cargo.toml` | MODIFY (publish metadata) | C |
| `rust/verified-anchor-macros/Cargo.toml` | MODIFY (publish metadata) | C |
| `rust/cargo-verified-anchor/Cargo.toml` | MODIFY (publish metadata) | C |
| `rust/verified-anchor-program/Cargo.toml` | MODIFY (`publish = false`) | C |
| `rust/verified-anchor-example/Cargo.toml` | MODIFY (`publish = false`) | C |
| `rust/verified-anchor-exploits/Cargo.toml` | MODIFY (`publish = false`) | C |
| `LICENSE-MIT` | NEW | C |
| `LICENSE-APACHE` | NEW | C |
| `README.md` (repo root) | NEW | C |
| `docs/publish-checklist.md` | NEW | C |

---

## Task 0: Create the feature branch

**Files:** none.

- [ ] **Step 1: Create + switch to the branch**
```bash
cd /home/parth/Desktop/PARTH/Verification
git checkout -b m7b-release
git log -1 --oneline  # should show the latest master commit
```
Expected: branch created, head matches master.

---

# PART A — `#[account]` attribute macro

## Task A1: Write the failing behavior test

**Files:** Modify `rust/verified-anchor/tests/behavior.rs`.

- [ ] **Step 1: Append the test**

At the bottom of `rust/verified-anchor/tests/behavior.rs`, append:
```rust
#[verified_anchor::account]
pub struct VaultAttr { pub authority: solana_program::pubkey::Pubkey, pub amount: u64 }

#[test]
fn account_attribute_implies_borsh_and_discriminator() {
    let d = <VaultAttr as verified_anchor::AccountData>::DISCRIMINATOR;
    assert_eq!(d, disc("VaultAttr"));
    let v = VaultAttr { authority: solana_program::pubkey::Pubkey::new_from_array([7u8; 32]), amount: 42 };
    let bytes = borsh::to_vec(&v).unwrap();
    let v2: VaultAttr = borsh::from_slice(&bytes).unwrap();
    assert_eq!(v2.amount, 42);
    assert_eq!(v2.authority, v.authority);
}
```

- [ ] **Step 2: Run + confirm failure**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test behavior account_attribute 2>&1 | tail -10
```
Expected: compile error — `cannot find attribute 'account' in 'verified_anchor'`.

---

## Task A2: Implement the `#[account]` attribute macro

**Files:** Create `rust/verified-anchor-macros/src/account_attr.rs`; modify `rust/verified-anchor-macros/src/lib.rs`, `rust/verified-anchor/src/lib.rs`, `rust/verified-anchor/src/prelude.rs`.

- [ ] **Step 1: Create the proc-macro module**

Create `rust/verified-anchor-macros/src/account_attr.rs`:
```rust
//! `#[account]` attribute macro — bundles `BorshSerialize + BorshDeserialize + AccountData`
//! so users write `#[account]` instead of three derives. Mirrors stock Anchor's sugar.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, AttributeArgs, Item};

pub fn account(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as AttributeArgs);
    if !args.is_empty() {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "verified-anchor: `#[account]` takes no arguments in M7b; the bundled derives are fixed (BorshSerialize, BorshDeserialize, AccountData). Use the explicit 3-derive form if you need different derive flags."
        ).to_compile_error().into();
    }
    let item = parse_macro_input!(input as Item);
    let item_struct = match item {
        Item::Struct(s) => s,
        _ => return syn::Error::new(
            proc_macro2::Span::call_site(),
            "verified-anchor: `#[account]` may only be applied to a named-fields struct"
        ).to_compile_error().into(),
    };
    let expanded = quote! {
        #[derive(::borsh::BorshSerialize, ::borsh::BorshDeserialize, ::verified_anchor::AccountData)]
        #item_struct
    };
    expanded.into()
}
```

Note: `syn 2` removed `AttributeArgs` in favour of `Punctuated<Meta, Token![,]>`. Check the `syn` version actually in use by reading `rust/verified-anchor-macros/Cargo.toml` (already loaded into memory in the spec) — it's `syn = { version = "2", features = ["full"] }`. So `AttributeArgs` is NOT in `syn 2`. Instead parse args as a `proc_macro2::TokenStream` and check that it's empty:
```rust
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Item};

pub fn account(args: TokenStream, input: TokenStream) -> TokenStream {
    let args_tokens: proc_macro2::TokenStream = args.into();
    if !args_tokens.is_empty() {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "verified-anchor: `#[account]` takes no arguments in M7b; the bundled derives are fixed (BorshSerialize, BorshDeserialize, AccountData). Use the explicit 3-derive form if you need different derive flags."
        ).to_compile_error().into();
    }
    let item = parse_macro_input!(input as Item);
    let item_struct = match item {
        Item::Struct(s) => s,
        _ => return syn::Error::new(
            proc_macro2::Span::call_site(),
            "verified-anchor: `#[account]` may only be applied to a named-fields struct"
        ).to_compile_error().into(),
    };
    let expanded = quote! {
        #[derive(::borsh::BorshSerialize, ::borsh::BorshDeserialize, ::verified_anchor::AccountData)]
        #item_struct
    };
    expanded.into()
}
```

- [ ] **Step 2: Wire the macro entry point**

In `rust/verified-anchor-macros/src/lib.rs`, add at the top near the existing `account_data_derive` module declaration:
```rust
mod account_attr;

#[proc_macro_attribute]
pub fn account(args: TokenStream, input: TokenStream) -> TokenStream {
    account_attr::account(args, input)
}
```

- [ ] **Step 3: Re-export from `verified-anchor`**

In `rust/verified-anchor/src/lib.rs`, after the existing `pub use verified_anchor_macros::AccountData as AccountData;` line, add:
```rust
pub use verified_anchor_macros::account;
```

In `rust/verified-anchor/src/prelude.rs`, add `account` to the `pub use crate::{...};` list. (Attribute macros and traits live in different namespaces — no collision.)

- [ ] **Step 4: Build + run the behavior test**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test behavior account_attribute 2>&1 | tail -10
```
Expected: `test result: ok. 1 passed`.

Also run the full behavior suite:
```bash
cargo test -p verified-anchor --test behavior 2>&1 | grep "test result"
```
Expected: `ok. 25 passed` (24 from M7a + 1 new).

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-macros/ rust/verified-anchor/src/ rust/verified-anchor/tests/behavior.rs
git commit -m "feat(macros): #[account] attribute macro - bundles Borsh + AccountData derives"
```

---

## Task A3: trybuild fixture for `#[account(args)]` rejection

**Files:** Create `rust/verified-anchor-macros/tests/ui/account_with_args.rs` (+ generated `.stderr`).

- [ ] **Step 1: Create the negative fixture**

Create `rust/verified-anchor-macros/tests/ui/account_with_args.rs`:
```rust
use verified_anchor::account;

#[account(some_flag)]
pub struct BadAttr {
    pub field: u64,
}

fn main() {}
```

- [ ] **Step 2: Generate the stderr snapshot**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust
TRYBUILD=overwrite cargo test -p verified-anchor-macros --test compile_fail 2>&1 | tail -10
cargo test -p verified-anchor-macros --test compile_fail 2>&1 | tail -5
```
Expected: first run writes `.stderr`; second run passes.

Open `rust/verified-anchor-macros/tests/ui/account_with_args.stderr` and confirm it mentions "takes no arguments" and "3-derive form".

- [ ] **Step 3: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-macros/tests/ui/
git commit -m "test(macros/ui): #[account(args)] is a compile_error"
```

---

# PART B — Per-seed `Bumps` + tuple return

## Task B1: Change `Accounts::try_accounts` to return `(Self, Self::Bumps)`

**Files:** Modify `rust/verified-anchor/src/lib.rs`, `rust/verified-anchor-macros/src/lib.rs`, `rust/verified-anchor/tests/behavior.rs`.

The macro currently emits `Result<Self, VAError>`; the trait demands the same. Change BOTH to `Result<(Self, Self::Bumps), VAError>`. The two existing M7a tests that call `try_accounts` get updated to destructure the tuple.

- [ ] **Step 1: Update the trait signature**

In `rust/verified-anchor/src/lib.rs`, replace:
```rust
pub trait Accounts<'info>: Sized {
    type Bumps;
    fn try_accounts(
        program_id: &Pubkey,
        accounts: &'info [AccountInfo<'info>],
        instr_data: &[u8],
    ) -> Result<Self, VAError>;
}
```
with:
```rust
pub trait Accounts<'info>: Sized {
    type Bumps;
    fn try_accounts(
        program_id: &Pubkey,
        accounts: &'info [AccountInfo<'info>],
        instr_data: &[u8],
    ) -> Result<(Self, Self::Bumps), VAError>;
}
```

- [ ] **Step 2: Update macro codegen return type + body**

In `rust/verified-anchor-macros/src/lib.rs`, in `derive_verified_accounts`, the expansion currently ends with (lines ~657-665):
```rust
            fn try_accounts(
                program_id: &::solana_program::pubkey::Pubkey,
                accounts: &'info [::solana_program::account_info::AccountInfo<'info>],
                instr_data: &[u8],
            ) -> ::core::result::Result<Self, ::verified_anchor::VAError> {
                <Self as ::verified_anchor::Validate>::validate(accounts, instr_data, program_id)?;
                ::core::result::Result::Ok(Self { #(#field_inits),* })
            }
```
Replace with (note return type AND tuple return):
```rust
            fn try_accounts(
                program_id: &::solana_program::pubkey::Pubkey,
                accounts: &'info [::solana_program::account_info::AccountInfo<'info>],
                instr_data: &[u8],
            ) -> ::core::result::Result<(Self, Self::Bumps), ::verified_anchor::VAError> {
                <Self as ::verified_anchor::Validate>::validate(accounts, instr_data, program_id)?;
                let __self = Self { #(#field_inits),* };
                let __bumps = #bumps_struct_name;
                ::core::result::Result::Ok((__self, __bumps))
            }
```
The `#bumps_struct_name` is still the empty unit struct (Task B2 fills it in). For B1 we keep `#bumps_struct_name` as a unit constructor; this compiles because `pub struct <Name>Bumps;` (no fields) is constructable with just the name.

- [ ] **Step 3: Update the two M7a tests that call `try_accounts`**

In `rust/verified-anchor/tests/behavior.rs`, find `try_accounts_deserializes_typed_data` and replace:
```rust
    let result: Result<VaultDataStruct, VAError> = <VaultDataStruct as Accounts>::try_accounts(&crate::ID, &accts, &[]);
    let parsed = result.expect("try_accounts should succeed");
    assert_eq!(parsed.vault.data.amount, 999);
    assert_eq!(parsed.vault.data.authority, Pubkey::new_from_array([7u8; 32]));
```
with:
```rust
    let result = <VaultDataStruct as Accounts>::try_accounts(&crate::ID, &accts, &[]);
    let (parsed, _bumps) = result.expect("try_accounts should succeed");
    assert_eq!(parsed.vault.data.amount, 999);
    assert_eq!(parsed.vault.data.authority, Pubkey::new_from_array([7u8; 32]));
```

Find `try_accounts_borsh_failed_on_truncated_data` and replace:
```rust
    let result: Result<VaultDataStruct, VAError> = <VaultDataStruct as Accounts>::try_accounts(&crate::ID, &accts, &[]);
    assert_eq!(result.err(), Some(VAError::BorshFailed { field: "vault" }));
```
with:
```rust
    let result = <VaultDataStruct as Accounts>::try_accounts(&crate::ID, &accts, &[]);
    assert_eq!(result.err(), Some(VAError::BorshFailed { field: "vault" }));
```
(The change is removing the explicit `Result<VaultDataStruct, VAError>` annotation; the assert on `result.err()` still works on `Result<(VaultDataStruct, VaultDataStructBumps), VAError>`.)

- [ ] **Step 4: Build + run all tests**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test --workspace 2>&1 | grep "test result"
```
Expected: every suite green. `behavior` should still be `25 passed` (no new test in B1, just the two existing updates).

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor/src/lib.rs rust/verified-anchor-macros/src/lib.rs rust/verified-anchor/tests/behavior.rs
git commit -m "feat(accounts): try_accounts returns (Self, Self::Bumps) - prepares for per-seed Bumps"
```

---

## Task B2: Per-seed `<Name>Bumps` codegen + populate canonical bumps

**Files:** Modify `rust/verified-anchor-macros/src/lib.rs`, `rust/verified-anchor/tests/behavior.rs`.

The macro currently emits `pub struct <Name>Bumps;` (unit). After B2, for structs with seeded PDAs the macro emits a struct with one `pub <field>: u8` per seeded field, and populates it in `try_accounts` by re-running `find_program_address` on the same seed expression.

- [ ] **Step 1: Write the failing test**

In `rust/verified-anchor/tests/behavior.rs`, append:
```rust
#[derive(VerifiedAccounts)]
struct WithPda<'info> {
    #[account(seeds = [b"vault", arg(0, 4)], bump)]
    pda: verified_anchor::UncheckedAccount<'info>,
}

#[test]
fn bumps_struct_carries_canonical_bump() {
    use verified_anchor::Accounts;
    let program_id = Pubkey::new_unique();
    let arg = [1u8, 2, 3, 4];
    let (pda, expected_bump) = Pubkey::find_program_address(&[b"vault", &arg], &program_id);
    let mut a = Acct { key: pda, owner: Pubkey::new_unique(), lamports: 1, data: vec![], is_signer: false, is_writable: false };
    let accts = [a.info()];
    let (_struct, bumps) = <WithPda as Accounts>::try_accounts(&program_id, &accts, &arg).unwrap();
    assert_eq!(bumps.pda, expected_bump);
}
```

- [ ] **Step 2: Run the test, confirm it fails**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test behavior bumps_struct 2>&1 | tail -15
```
Expected: compile error — `bumps.pda` references a field that does not exist on the unit struct `WithPdaBumps`.

- [ ] **Step 3: Update the macro to emit per-field Bumps**

In `rust/verified-anchor-macros/src/lib.rs`, in `derive_verified_accounts`, find the line:
```rust
    let bumps_struct_name = syn::Ident::new(&format!("{}Bumps", name), name.span());
```
Just AFTER that line, build the list of seeded-field idents AND the corresponding seed-expression token streams (reusing the same seed-expression construction as in `validate_body`):
```rust
    // Identify seeded fields (those with a Constraint::Seeds), preserving order.
    let seeded: Vec<(usize, &FieldSpec, &Vec<SeedElem>)> = specs.iter().enumerate()
        .filter_map(|(i, s)| s.constraints.iter().find_map(|c| {
            if let Constraint::Seeds(elems) = c { Some((i, s, elems)) } else { None }
        }))
        .collect();

    // Build name→index map for resolving `field.key()` seeds.
    let index_of: std::collections::HashMap<String, usize> =
        specs.iter().enumerate().map(|(i, s)| (s.name.clone(), i)).collect();

    // Per-seeded-field: (Ident for the Bumps field, TokenStream of seed slice exprs).
    let bumps_fields: Vec<(syn::Ident, Vec<TokenStream2>)> = seeded.iter().map(|(i, spec, elems)| {
        let fname = syn::Ident::new(&spec.name, name.span());
        let seed_exprs: Vec<TokenStream2> = elems.iter().map(|se| match se {
            SeedElem::Literal(b) => quote! { &#b[..] },
            SeedElem::FieldKey(id) => {
                let fi = *index_of.get(&id.to_string())
                    .unwrap_or_else(|| panic!("seed field `{}` is not a field of this struct", id));
                quote! { accounts[#fi].key.as_ref() }
            }
            SeedElem::InstrArg(off, len) => {
                let end = off + len;
                quote! { &instr_data[#off..#end] }
            }
        }).collect();
        let _ = i; // index reserved for future use (e.g. memoising bumps from validate)
        (fname, seed_exprs)
    }).collect();
```

Then BUILD the Bumps struct declaration + the populated initialiser. Replace the existing emission:
```rust
        pub struct #bumps_struct_name;
```
with:
```rust
        #bumps_struct_decl
```
and the existing `let __bumps = #bumps_struct_name;` (added in B1) with `let __bumps = #bumps_struct_init;`.

Define `bumps_struct_decl` and `bumps_struct_init` BEFORE the `quote!` block:
```rust
    let (bumps_struct_decl, bumps_struct_init) = if bumps_fields.is_empty() {
        // No seeded fields: keep the unit struct + unit constructor.
        (
            quote! { pub struct #bumps_struct_name; },
            quote! { #bumps_struct_name },
        )
    } else {
        let decl_fields: Vec<TokenStream2> = bumps_fields.iter().map(|(fname, _)| {
            quote! { pub #fname: u8 }
        }).collect();
        let init_fields: Vec<TokenStream2> = bumps_fields.iter().map(|(fname, seed_exprs)| {
            quote! {
                #fname: {
                    let __seeds: &[&[u8]] = &[ #(#seed_exprs),* ];
                    let (_pda, __b) = ::solana_program::pubkey::Pubkey::find_program_address(__seeds, program_id);
                    __b
                }
            }
        }).collect();
        (
            quote! { pub struct #bumps_struct_name { #(#decl_fields),* } },
            quote! { #bumps_struct_name { #(#init_fields),* } },
        )
    };
```

NOTE on placement: this block goes BEFORE the `let expanded = quote! { ... }` so the two tokens are in scope when the `quote!` interpolates them.

- [ ] **Step 4: Update the `expanded` block to use the new tokens**

Replace `pub struct #bumps_struct_name;` (the unit decl emitted earlier) with `#bumps_struct_decl`. Replace `let __bumps = #bumps_struct_name;` (from B1) with `let __bumps = #bumps_struct_init;`.

- [ ] **Step 5: Build + run the new test**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test behavior bumps_struct 2>&1 | tail -10
```
Expected: `test result: ok. 1 passed`.

- [ ] **Step 6: Full workspace test (no regressions)**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test --workspace 2>&1 | grep "test result"
```
Expected: every suite green. `behavior` should now be `26 passed` (25 from B1 + 1 new).

- [ ] **Step 7: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-macros/src/lib.rs rust/verified-anchor/tests/behavior.rs
git commit -m "feat(macros): per-seed <Name>Bumps codegen - try_accounts populates canonical bumps"
```

---

# PART C — crates.io publish prep

## Task C1: LICENSE files

**Files:** Create `LICENSE-MIT`, `LICENSE-APACHE` at the repo root.

- [ ] **Step 1: Create `LICENSE-MIT`**

Create `/home/parth/Desktop/PARTH/Verification/LICENSE-MIT`:
```
MIT License

Copyright (c) 2026 Parth Arvind Rathi

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [ ] **Step 2: Create `LICENSE-APACHE`**

Create `/home/parth/Desktop/PARTH/Verification/LICENSE-APACHE` with the full Apache 2.0 text. Use the canonical text from <https://www.apache.org/licenses/LICENSE-2.0.txt>. Copy-paste the entire license body (do NOT abbreviate or paraphrase). Add a final paragraph at the bottom:
```
Copyright 2026 Parth Arvind Rathi

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```

If unable to fetch the canonical Apache text via tooling, use the standard text recorded in `~/.cargo/registry/src/*-apache-license-2.0-*` if cached, else mark this step's commit as "LICENSE-APACHE: short pointer header" with `See https://www.apache.org/licenses/LICENSE-2.0 for full text. Copyright 2026 Parth Arvind Rathi.` and fix in a follow-up commit. PREFER the full text.

- [ ] **Step 3: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add LICENSE-MIT LICENSE-APACHE
git commit -m "chore(license): add LICENSE-MIT + LICENSE-APACHE (Apache-2.0 OR MIT)"
```

---

## Task C2: Publish metadata on the 3 publishable crates

**Files:** Modify `rust/verified-anchor/Cargo.toml`, `rust/verified-anchor-macros/Cargo.toml`, `rust/cargo-verified-anchor/Cargo.toml`.

- [ ] **Step 1: Update `verified-anchor/Cargo.toml` `[package]` block**

Read the current file. Replace the `[package]` block with:
```toml
[package]
name = "verified-anchor"
version = "0.1.0"
edition = "2021"
description = "Formally verified (Lean 4) account-validation runtime for Solana — Anchor-compatible, proof-producing."
license = "Apache-2.0 OR MIT"
repository = "https://github.com/REPLACE_ME/Verification"
homepage = "https://github.com/REPLACE_ME/Verification"
readme = "../../README.md"
keywords = ["solana", "anchor", "verified", "lean4", "proc-macro"]
categories = ["development-tools", "cryptography"]
```
Keep the existing `[dependencies]`, `[dev-dependencies]`, etc. blocks unchanged below.

- [ ] **Step 2: Update `verified-anchor-macros/Cargo.toml` `[package]` block**

Replace the `[package]` block with:
```toml
[package]
name = "verified-anchor-macros"
version = "0.1.0"
edition = "2021"
description = "Proof-producing proc-macros for verified-anchor (#[derive(VerifiedAccounts)], #[derive(AccountData)], #[account])."
license = "Apache-2.0 OR MIT"
repository = "https://github.com/REPLACE_ME/Verification"
homepage = "https://github.com/REPLACE_ME/Verification"
readme = "../../README.md"
keywords = ["solana", "anchor", "verified", "lean4", "proc-macro"]
categories = ["development-tools", "cryptography"]
```
Keep `[lib] proc-macro = true` and the dependency blocks unchanged.

- [ ] **Step 3: Update `cargo-verified-anchor/Cargo.toml` `[package]` block**

Replace the `[package]` block with:
```toml
[package]
name = "cargo-verified-anchor"
version = "0.1.0"
edition = "2021"
description = "Cargo subcommand that discharges verified-anchor proof obligations via Lean."
license = "Apache-2.0 OR MIT"
repository = "https://github.com/REPLACE_ME/Verification"
homepage = "https://github.com/REPLACE_ME/Verification"
readme = "../../README.md"
keywords = ["solana", "anchor", "verified", "lean4", "cargo-subcommand"]
categories = ["development-tools::cargo-plugins", "command-line-utilities"]
```
Keep the rest of the file unchanged.

- [ ] **Step 4: Quick sanity build**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo build --workspace 2>&1 | tail -3
```
Expected: still green.

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor/Cargo.toml rust/verified-anchor-macros/Cargo.toml rust/cargo-verified-anchor/Cargo.toml
git commit -m "chore(cargo): add crates.io publish metadata (desc/license/repo/keywords/categories)"
```

---

## Task C3: Mark non-publishable crates `publish = false`

**Files:** Modify `rust/verified-anchor-program/Cargo.toml`, `rust/verified-anchor-example/Cargo.toml`, `rust/verified-anchor-exploits/Cargo.toml`.

- [ ] **Step 1: Each of the 3 files**

In EACH of the 3 `Cargo.toml` files (verified-anchor-program, verified-anchor-example, verified-anchor-exploits), add inside the existing `[package]` block:
```toml
publish = false
```
Place it after the `edition` line. Example for verified-anchor-program/Cargo.toml:
```toml
[package]
name = "verified-anchor-program"
version = "0.1.0"
edition = "2021"
publish = false
```

- [ ] **Step 2: Sanity build**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo build --workspace 2>&1 | tail -3
```
Expected: still green.

- [ ] **Step 3: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-program/Cargo.toml rust/verified-anchor-example/Cargo.toml rust/verified-anchor-exploits/Cargo.toml
git commit -m "chore(cargo): mark test-fixture crates publish = false (program/example/exploits)"
```

---

## Task C4: Top-level `README.md`

**Files:** Create `/home/parth/Desktop/PARTH/Verification/README.md`.

- [ ] **Step 1: Create the README**

Create `/home/parth/Desktop/PARTH/Verification/README.md` with this exact content (DO NOT abbreviate; the entire body below is the file):

````markdown
# Verified Anchor

Formally verified (Lean 4) account validation for Solana programs — Anchor-compatible, proof-producing.

**Status:** `0.1.0` alpha. M1–M7a complete. Lean theorems depend only on `[propext, Quot.sound]`.

## What it is

Verified Anchor pairs a Lean 4 contract that defines what "valid accounts" means with proof-producing Rust proc-macros that emit Solana validation/lifecycle code whose logic is proven to implement that contract. The Lean side `Codegen.genValidate_sound` theorem reads `M4Subset s → (genValidate s c = true ↔ validates s c)` — the generated validator is observably equivalent to the contract. The Rust side derives an `impl Validate` (the proven gate) and an `impl Accounts<'info>` (the developer surface) that runs `validate` first, then Borsh-deserialises typed account data.

The developer surface is signature-identical to stock Anchor — same `Account<'info, T>`, `Signer<'info>`, `Program<'info, P>`, `Context<'a, 'b, 'c, 'info, T>` shapes — so a verified-anchor instruction handler reads exactly like a stock-Anchor one.

## Quick start

```rust
use verified_anchor::prelude::*;

#[account]
pub struct Vault {
    pub authority: Pubkey,
    pub amount: u64,
}

#[derive(VerifiedAccounts)]
pub struct Transfer<'info> {
    #[account(mut, has_one = authority)]
    pub vault: Account<'info, Vault>,
    pub authority: Signer<'info>,
}

pub fn transfer(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let (ctx_accts, bumps) = Transfer::try_accounts(program_id, accounts, data)?;
    let _ = bumps;
    // ... your handler logic; ctx_accts.vault.amount is the typed payload.
    Ok(())
}
```

Discharge the per-struct proof obligation via Lean:
```bash
cargo verified-anchor check -p my-crate --lean-dir <path-to-lean-source>
```

## What's proven, what isn't

Read `docs/verified-anchor-bridge.md`. Headline:

- **Proven:** `validates` ↔ `genValidate` ↔ generated Rust per-constraint checks for the M4 subset (signer/mut/owner/has_one/seeds/discriminator + init/close Hoare).
- **Outside the proof:** Borsh deserialisation (`Account<T>` payload decoding), CPI effects beyond init/close, the unspoken Solana runtime contract.
- The bridge doc is honest about every transcription gap.

## Compatibility with stock Anchor

`docs/migrating-from-anchor.md` has the side-by-side syntax mapping. Verified-anchor is field-for-field compatible at the account-validation surface; the bundled `#[account]` attribute matches stock Anchor's wire format (`sha256("account:" + Name)[..8]` discriminator).

## Empirical validation

`docs/exploit-case-studies.md` reproduces four real macro-level account-validation bug classes as litesvm before/after: Cashio (has_one/owner/discriminator), type-confusion (discriminator), Crema (owner), PDA seeds. Each scenario asserts naive(attacker) → bad on-chain effect AND verified(attacker) → on-chain `Err`.

## Roadmap

- **M7b (this release):** `#[account]` attribute, per-seed `Bumps`, crates.io packaging.
- **M7c (next):** QEDGen integration + announcement.

## License

Licensed under either of

- Apache License, Version 2.0 (`LICENSE-APACHE`)
- MIT license (`LICENSE-MIT`)

at your option.
````

- [ ] **Step 2: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add README.md
git commit -m "docs: top-level README for crates.io landing page"
```

---

## Task C5: Publish checklist

**Files:** Create `docs/publish-checklist.md`.

- [ ] **Step 1: Create the checklist**

Create `/home/parth/Desktop/PARTH/Verification/docs/publish-checklist.md`:
```markdown
# Publishing verified-anchor to crates.io

Pre-publish steps (must be done before `cargo publish`):

1. **Set the real GitHub URL.** Find all three publishable crates' Cargo.toml files and replace `REPLACE_ME` in `repository` and `homepage` with the real path:
   - `rust/verified-anchor/Cargo.toml`
   - `rust/verified-anchor-macros/Cargo.toml`
   - `rust/cargo-verified-anchor/Cargo.toml`
2. **`cargo login`** with a crates.io API token if you haven't already.
3. **Run dry-runs.** Order matters: macros first (everything else depends on it), then the runtime, then the cargo subcommand:
   ```bash
   cd rust
   cargo publish --dry-run -p verified-anchor-macros
   cargo publish --dry-run -p verified-anchor
   cargo publish --dry-run -p cargo-verified-anchor
   ```
   All three must succeed.
4. **Publish.** Sleep ~60 seconds between publishes so the registry has time to index each crate (without this, the next `cargo publish` can fail because the registry lookup of the newly-uploaded dep hasn't propagated yet):
   ```bash
   cd rust
   cargo publish -p verified-anchor-macros && sleep 60
   cargo publish -p verified-anchor && sleep 60
   cargo publish -p cargo-verified-anchor
   ```

After-publish housekeeping:

- Tag the commit: `git tag v0.1.0 && git push --tags`.
- Verify the crates.io page renders the README correctly (the `readme = "../../README.md"` path means it picks up the workspace-root README).
- Announce.

What is NOT published:

- `verified-anchor-program`, `verified-anchor-example`, `verified-anchor-exploits` are test fixtures (`publish = false`).
- The Lean source under `lean/` is the proof artefact; it is NOT a Rust crate and does not go to crates.io.

What to verify before tagging v0.2.0 / v0.x.0:

- Lean axioms unchanged (`#print axioms verified_anchor::genValidate_sound` → `[propext, Quot.sound]`).
- `cargo test --workspace` green.
- Both `cargo verified-anchor check -p verified-anchor-example` and `-p verified-anchor-exploits` exit 0.
- Both SBF `.so`s rebuild without `PT_DYNAMIC` warnings.
```

- [ ] **Step 2: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add docs/publish-checklist.md
git commit -m "docs: publish checklist for crates.io release"
```

---

## Task C6: `cargo publish --dry-run` for the 3 publishable crates

**Files:** none changed; this is a verification gate.

- [ ] **Step 1: Run dry-runs**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust
cargo publish --dry-run -p verified-anchor-macros 2>&1 | tail -10
cargo publish --dry-run -p verified-anchor 2>&1 | tail -10
cargo publish --dry-run -p cargo-verified-anchor 2>&1 | tail -10
```
Expected: each ends in `Finished` (or `Packaged ... ready`). The `REPLACE_ME` URL is accepted by `--dry-run` (the URL is not validated locally).

If any fails, READ the error. Likely causes:
- Missing required field (description/license already added in C2).
- `path =` dependencies in `[dependencies]` blocks — cargo requires a published version constraint. For `verified-anchor`'s dep on `verified-anchor-macros` (and `cargo-verified-anchor`'s dep on `verified-anchor`), the entry must be `verified-anchor-macros = { path = "../verified-anchor-macros", version = "0.1.0" }`. The `version = "0.1.0"` is required for publish; the `path =` is used during local development. Edit the existing `[dependencies]` block to add `version = "0.1.0"` to each intra-workspace path dep that crosses into a publishable crate.

- [ ] **Step 2: If C6 Step 1 surfaces the path/version requirement, fix it**

For each publishable crate's `[dependencies]` that lists a sibling publishable crate via `path =`, add `version = "0.1.0"`:
- `rust/verified-anchor/Cargo.toml`: `verified-anchor-macros = { path = "../verified-anchor-macros", version = "0.1.0" }`
- `rust/cargo-verified-anchor/Cargo.toml`: any dep on `verified-anchor` (if present) likewise.

The `verified-anchor-program/-example/-exploits` deps DON'T need version pins because those are `publish = false`.

Re-run Step 1 dry-runs after the edit. Loop until all 3 succeed.

- [ ] **Step 3: Commit any fix from Step 2**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor/Cargo.toml rust/cargo-verified-anchor/Cargo.toml
git status -s
# only commit if there are changes
git commit -m "chore(cargo): pin sibling-crate dep versions for cargo publish"
```
If `git status -s` is empty, skip the commit.

---

# PART F — Final gate

## Task F1: Mandatory full gate

**Files:** none modified by this task.

- [ ] **Step 1: Lean build + zero-sorry + axiom audit**
```bash
export PATH="$HOME/.elan/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/lean
lake build 2>&1 | tail -3
grep -rn 'sorry\|admit' VerifiedAnchor/ || echo "PASS lean zero-sorry"
cat > VerifiedAnchor/M7bAuditAx.lean << 'EOF'
import VerifiedAnchor
import VerifiedAnchor.Codegen.StructLifecycle
#print axioms VerifiedAnchor.genValidate_sound
#print axioms VerifiedAnchor.lifecycle_sound
EOF
lake env lean VerifiedAnchor/M7bAuditAx.lean
rm -f VerifiedAnchor/M7bAuditAx.lean
```
Expected: `Build completed successfully (...)`, `PASS lean zero-sorry`, and BOTH `#print axioms` show `[propext, Quot.sound]` (no new axioms vs M7a).

- [ ] **Step 2: SBF rebuilds (no PT_DYNAMIC)**
```bash
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$HOME/.elan/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-program && cargo-build-sbf --no-rustup-override 2>&1 | grep -iE "PT_DYNAMIC|Finished" | tail -3
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-exploits && cargo-build-sbf --no-rustup-override 2>&1 | grep -iE "PT_DYNAMIC|Finished" | tail -3
```
Expected: both `Finished release` with NO `PT_DYNAMIC` warning.

- [ ] **Step 3: Full workspace test suite**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test --workspace 2>&1 | grep -E "Running |test result:" | head -25
```
Expected: every suite green. `behavior` should be 26 (24 M7a + 1 A2 + 1 B2). All M7a suites at their existing counts.

- [ ] **Step 4: Both M5 checks**
```bash
export PATH="$HOME/.elan/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust
cargo run -q -p cargo-verified-anchor -- verified-anchor check -p verified-anchor-example --lean-dir ../lean
echo "EXIT $?"
cargo run -q -p cargo-verified-anchor -- verified-anchor check -p verified-anchor-exploits --lean-dir ../lean
echo "EXIT $?"
```
Expected: both `EXIT 0`; example shows 3 ✓ lines; exploits shows 4 ✓ lines.

- [ ] **Step 5: Document the gate result (no commit unless something failed)**

If everything passed, F1 is done — no commit needed. If something failed, FIX it (do NOT silently lower the bar) and re-run F1.

---

## Done-bar verification (after F1)

1. `#[account]` attribute compiles a struct with the 3 bundled derives; manual 3-derive form still works; behavior test passes; trybuild `account_with_args.rs` asserts arg rejection. ✅ (A1, A2, A3)
2. `Accounts::try_accounts` returns `(Self, Self::Bumps)`; seeded structs emit `pub struct <Name>Bumps { pub <f>: u8, ... }`; bumps populated from `find_program_address`. Non-seeded structs emit empty `<Name>Bumps`. ✅ (B1, B2)
3. All 3 publishable crates pass `cargo publish --dry-run`. ✅ (C2, C6)
4. LICENSE files + README + publish-checklist committed. ✅ (C1, C4, C5)
5. Test fixture crates marked `publish = false`. ✅ (C3)
6. Mandatory full gate green. ✅ (F1)

After F1: do the standard branch-finish — merge `--no-ff` to master with a summary commit message, delete the branch, update `HANDOVER.md` + project memory.
