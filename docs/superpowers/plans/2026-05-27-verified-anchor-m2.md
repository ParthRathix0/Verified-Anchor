# Verified Anchor — Milestone 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust proc-macro `#[derive(VerifiedAccounts)]` that generates `solana-program` validation code for `mut`/`signer`/`owner` plus a Lean `lean_spec`, paired with a Lean operational model (`genValidate`) proven to agree with the M1 contract on the M2 subset, and one closed-loop example.

**Architecture:** A new `rust/` cargo workspace (proc-macro crate + runtime/tests crate using `solana-program`), and a new `lean/VerifiedAnchor/Codegen/` layer in the existing Lean library. The macro emits validation code that transcribes the Lean `gen*` functions; the Lean side proves `genValidate ≡ validates` (M1 contract) for the M2 subset. The Rust↔Lean transcription is documented + differentially tested, not proven (honest trust boundary).

**Tech Stack:** Rust 1.93.1 / cargo, `syn`(full)/`quote`/`proc-macro2`, `solana-program 2`. Lean 4.30.0 / Lake 5.0.0 (existing M1 library).

---

## Conventions for every task

- **Lean toolchain:** prefix lake commands with `export PATH="$HOME/.elan/bin:$PATH"`; Lean work runs in `/home/parth/Desktop/PARTH/Verification/lean`.
- **Rust:** runs in `/home/parth/Desktop/PARTH/Verification/rust`. `cargo` is on PATH already. First build downloads `solana-program`'s dep tree (verified to compile, ~minutes first time).
- **"Test" =** Rust: `cargo test`. Lean: `lake build` (the `#guard`/`example`/`theorem` are the tests; a failing one is a build error).
- **Zero `sorry`** in Lean; no `unsafe` in Rust. Escalate (systematic-debugging) rather than stub.
- **Commit** after each task with the message shown.
- The repo root `.gitignore` already ignores `lean/.lake/`, `rust/target/`, `target/`.

---

## File structure

| File | Responsibility |
|------|----------------|
| `rust/Cargo.toml` | Workspace manifest (2 members) |
| `rust/verified-anchor-macros/Cargo.toml` | proc-macro crate manifest (syn/quote/proc-macro2) |
| `rust/verified-anchor-macros/src/lib.rs` | `#[derive(VerifiedAccounts)]`: parse, codegen `validate`, codegen `lean_spec` |
| `rust/verified-anchor/Cargo.toml` | runtime crate manifest (solana-program, the macro) |
| `rust/verified-anchor/src/lib.rs` | `Validate` trait, `VAError`, re-export of the derive |
| `rust/verified-anchor/tests/behavior.rs` | `validate` accepts good / rejects each violation |
| `rust/verified-anchor/tests/lean_spec.rs` | `lean_spec()` emits the expected literal |
| `lean/VerifiedAnchor/Codegen/Generated.lean` | `genSigner`/`genMut`/`genOwner`, `genConstraint`, `genFieldValidate`, `genValidate` |
| `lean/VerifiedAnchor/Codegen/Soundness.lean` | per-constraint lemmas, `M2Subset`, `genValidate_sound` |
| `lean/VerifiedAnchor/Codegen/ExampleGenerated.lean` | macro-emitted spec + checked obligations |
| `lean/VerifiedAnchor.lean` | (modify) import the 3 new Codegen modules |
| `docs/verified-anchor-bridge.md` | Rust↔Lean correspondence + trust boundary |

**Note on `validate` signature:** the macro generates `fn validate(accounts: &[AccountInfo]) -> Result<(), VAError>` (no `&self`) as a trait method — cleaner than the design's `&self` sketch, since the struct is a compile-time spec carrier and validation is positional over the runtime account slice (matching the Lean `Ctx`). M2 reads only explicit `#[account(...)]` attributes; type-implied constraints (e.g. `Signer` type ⇒ `signer`) are M3+. Field types are therefore ignored by M2 codegen.

---

## Task 0: Rust workspace + crate skeletons

**Files:** Create `rust/Cargo.toml`, `rust/verified-anchor-macros/Cargo.toml`, `rust/verified-anchor-macros/src/lib.rs`, `rust/verified-anchor/Cargo.toml`, `rust/verified-anchor/src/lib.rs`.

- [ ] **Step 1: Workspace manifest**

Create `rust/Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["verified-anchor-macros", "verified-anchor"]
```

- [ ] **Step 2: proc-macro crate manifest**

Create `rust/verified-anchor-macros/Cargo.toml`:
```toml
[package]
name = "verified-anchor-macros"
version = "0.1.0"
edition = "2021"

[lib]
proc-macro = true

[dependencies]
syn = { version = "2", features = ["full"] }
quote = "1"
proc-macro2 = "1"
```

- [ ] **Step 3: proc-macro stub**

Create `rust/verified-anchor-macros/src/lib.rs`:
```rust
use proc_macro::TokenStream;

/// `#[derive(VerifiedAccounts)]` — generates `validate` and `lean_spec`.
/// Stub for now; real codegen lands in later tasks.
#[proc_macro_derive(VerifiedAccounts, attributes(account))]
pub fn derive_verified_accounts(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}
```

- [ ] **Step 4: runtime crate manifest**

Create `rust/verified-anchor/Cargo.toml`:
```toml
[package]
name = "verified-anchor"
version = "0.1.0"
edition = "2021"

[dependencies]
solana-program = "2"
verified-anchor-macros = { path = "../verified-anchor-macros" }
```

- [ ] **Step 5: runtime crate stub**

Create `rust/verified-anchor/src/lib.rs`:
```rust
//! Verified Anchor runtime support (Milestone 2).
pub use verified_anchor_macros::VerifiedAccounts;
```

- [ ] **Step 6: Build the workspace**

Run:
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo build 2>&1 | tail -5
```
Expected: downloads `solana-program` deps on first run, then `Finished`. Both crates compile.

- [ ] **Step 7: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/ && git commit -m "feat(rust): scaffold verified-anchor workspace (proc-macro + runtime crates)"
```

---

## Task 1: `VAError` and `Validate` trait

**Files:** Modify `rust/verified-anchor/src/lib.rs`.

- [ ] **Step 1: Write the error type and trait**

Replace `rust/verified-anchor/src/lib.rs` with:
```rust
//! Verified Anchor runtime support (Milestone 2).
use solana_program::account_info::AccountInfo;

pub use verified_anchor_macros::VerifiedAccounts;

/// Why account validation failed. `field` is the struct field name that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VAError {
    MissingSigner { field: &'static str },
    NotWritable { field: &'static str },
    WrongOwner { field: &'static str },
    /// Fewer accounts were supplied than the struct declares.
    NotEnoughAccounts { expected: usize, got: usize },
}

impl core::fmt::Display for VAError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VAError::MissingSigner { field } => write!(f, "account `{field}` must be a signer"),
            VAError::NotWritable { field } => write!(f, "account `{field}` must be writable"),
            VAError::WrongOwner { field } => write!(f, "account `{field}` has the wrong owner"),
            VAError::NotEnoughAccounts { expected, got } =>
                write!(f, "expected {expected} accounts, got {got}"),
        }
    }
}

impl std::error::Error for VAError {}

/// Implemented by `#[derive(VerifiedAccounts)]`. Validation is positional over the
/// runtime account slice (index = field declaration order), matching the Lean `Ctx`.
pub trait Validate {
    fn validate(accounts: &[AccountInfo]) -> Result<(), VAError>;
}
```

- [ ] **Step 2: Build**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo build 2>&1 | tail -5`
Expected: compiles.

- [ ] **Step 3: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor/src/lib.rs
git commit -m "feat(rust): VAError and Validate trait"
```

---

## Task 2: Derive macro — parse attributes and generate `validate`

**Files:** Modify `rust/verified-anchor-macros/src/lib.rs`; Create `rust/verified-anchor/tests/behavior.rs`.

- [ ] **Step 1: Write the failing behavior test first**

Create `rust/verified-anchor/tests/behavior.rs`:
```rust
use solana_program::account_info::AccountInfo;
use solana_program::pubkey::Pubkey;
use verified_anchor::{Validate, VAError, VerifiedAccounts};

// Spec carrier: field names + #[account(..)] attrs define the constraints.
// Field types are ignored by M2 codegen.
#[derive(VerifiedAccounts)]
struct Transfer {
    #[account(mut)]
    vault: u8,
    #[account(signer)]
    authority: u8,
}

/// Build an owned AccountInfo backing store, then the AccountInfo referencing it.
struct Acct {
    key: Pubkey,
    owner: Pubkey,
    lamports: u64,
    data: Vec<u8>,
    is_signer: bool,
    is_writable: bool,
}
impl Acct {
    fn info(&mut self) -> AccountInfo {
        AccountInfo::new(
            &self.key, self.is_signer, self.is_writable,
            &mut self.lamports, &mut self.data, &self.owner, false, 0,
        )
    }
}

fn acct(is_signer: bool, is_writable: bool) -> Acct {
    Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(),
            lamports: 1, data: vec![], is_signer, is_writable }
}

#[test]
fn accepts_valid() {
    let mut v = acct(false, true);    // vault: writable
    let mut a = acct(true, false);    // authority: signer
    let accts = [v.info(), a.info()];
    assert_eq!(Transfer::validate(&accts), Ok(()));
}

#[test]
fn rejects_non_writable_vault() {
    let mut v = acct(false, false);   // vault NOT writable
    let mut a = acct(true, false);
    let accts = [v.info(), a.info()];
    assert_eq!(Transfer::validate(&accts), Err(VAError::NotWritable { field: "vault" }));
}

#[test]
fn rejects_non_signer_authority() {
    let mut v = acct(false, true);
    let mut a = acct(false, false);   // authority NOT signer
    let accts = [v.info(), a.info()];
    assert_eq!(Transfer::validate(&accts), Err(VAError::MissingSigner { field: "authority" }));
}

#[test]
fn rejects_too_few_accounts() {
    let mut v = acct(false, true);
    let accts = [v.info()];           // only 1, struct declares 2
    assert_eq!(Transfer::validate(&accts), Err(VAError::NotEnoughAccounts { expected: 2, got: 1 }));
}
```

- [ ] **Step 2: Run the test to confirm it fails (no codegen yet)**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test behavior 2>&1 | tail -15`
Expected: COMPILE FAILURE — `Transfer::validate` does not exist (the derive is still a stub).

- [ ] **Step 3: Implement parsing + `validate` codegen**

Replace `rust/verified-anchor-macros/src/lib.rs` with:
```rust
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse_macro_input, punctuated::Punctuated, Data, DeriveInput, Expr, Fields, Token,
};

/// One M2 constraint parsed from a field's `#[account(...)]`.
enum Constraint {
    Signer,
    Mut,
    Owner(Expr),
}

/// Parse the constraints from a single `#[account(...)]` attribute's tokens.
/// Accepts a comma-separated list of: `signer`, `mut`, `owner = EXPR`.
fn parse_account_attr(attr: &syn::Attribute) -> syn::Result<Vec<Constraint>> {
    let mut out = Vec::new();
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("signer") {
            out.push(Constraint::Signer);
            Ok(())
        } else if meta.path.is_ident("mut") {
            out.push(Constraint::Mut);
            Ok(())
        } else if meta.path.is_ident("owner") {
            let value = meta.value()?;          // parses `=`
            let expr: Expr = value.parse()?;    // parses EXPR
            out.push(Constraint::Owner(expr));
            Ok(())
        } else {
            Err(meta.error("unsupported constraint for Milestone 2 (only signer, mut, owner)"))
        }
    })?;
    Ok(out)
}

/// `mut` is a keyword; `parse_nested_meta`'s `meta.path.is_ident("mut")` still matches the
/// raw identifier path, so no special handling is needed beyond the check above.

struct FieldSpec {
    name: String,
    constraints: Vec<Constraint>,
}

fn collect_fields(input: &DeriveInput) -> syn::Result<Vec<FieldSpec>> {
    let Data::Struct(ds) = &input.data else {
        return Err(syn::Error::new_spanned(input, "VerifiedAccounts requires a struct"));
    };
    let Fields::Named(named) = &ds.fields else {
        return Err(syn::Error::new_spanned(&ds.fields, "VerifiedAccounts requires named fields"));
    };
    let mut specs = Vec::new();
    for field in &named.named {
        let name = field.ident.as_ref().unwrap().to_string();
        let mut constraints = Vec::new();
        for attr in &field.attrs {
            if attr.path().is_ident("account") {
                constraints.extend(parse_account_attr(attr)?);
            }
        }
        specs.push(FieldSpec { name, constraints });
    }
    Ok(specs)
}

fn validate_body(specs: &[FieldSpec]) -> TokenStream2 {
    let n = specs.len();
    let mut checks = Vec::new();
    for (i, spec) in specs.iter().enumerate() {
        let name = &spec.name;
        for c in &spec.constraints {
            let check = match c {
                Constraint::Signer => quote! {
                    if !accounts[#i].is_signer {
                        return Err(::verified_anchor::VAError::MissingSigner { field: #name });
                    }
                },
                Constraint::Mut => quote! {
                    if !accounts[#i].is_writable {
                        return Err(::verified_anchor::VAError::NotWritable { field: #name });
                    }
                },
                Constraint::Owner(expr) => quote! {
                    if accounts[#i].owner != &(#expr) {
                        return Err(::verified_anchor::VAError::WrongOwner { field: #name });
                    }
                },
            };
            checks.push(check);
        }
    }
    quote! {
        fn validate(accounts: &[::solana_program::account_info::AccountInfo]) -> ::core::result::Result<(), ::verified_anchor::VAError> {
            if accounts.len() < #n {
                return Err(::verified_anchor::VAError::NotEnoughAccounts { expected: #n, got: accounts.len() });
            }
            #(#checks)*
            Ok(())
        }
    }
}

#[proc_macro_derive(VerifiedAccounts, attributes(account))]
pub fn derive_verified_accounts(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let specs = match collect_fields(&input) {
        Ok(s) => s,
        Err(e) => return e.to_compile_error().into(),
    };
    let name = &input.ident;
    let body = validate_body(&specs);
    let expanded = quote! {
        impl ::verified_anchor::Validate for #name {
            #body
        }
    };
    expanded.into()
}

// silence unused import until lean_spec (Task 3) uses Punctuated/Token
#[allow(unused_imports)]
use {Punctuated as _Punctuated, Token as _Token};
```
NOTE on the trailing `use` shim: `Punctuated`/`Token` are imported for Task 3. If they
cause an "unused import" *error* (warnings are fine), delete them from the top `use` and
the shim line now; re-add in Task 3. Prefer deleting the shim line and just removing
`Punctuated, Token` from the import for this task.

- [ ] **Step 4: Run the behavior tests to confirm they pass**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test behavior 2>&1 | tail -15`
Expected: 4 passed.

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-macros/src/lib.rs rust/verified-anchor/tests/behavior.rs
git commit -m "feat(macros): derive VerifiedAccounts generating validate (mut/signer/owner)"
```

---

## Task 3: Derive macro — generate `lean_spec`

**Files:** Modify `rust/verified-anchor-macros/src/lib.rs`; Create `rust/verified-anchor/tests/lean_spec.rs`.

The macro emits, in the same `impl`, an associated `fn lean_spec() -> String` returning the
M1 `AccountsStruct` literal. For M2 every field is emitted with `AccountType.uncheckedAccount`
(types are ignored), `programId` is the all-zero pubkey placeholder, `owner = EXPR` emits
`Constraint.owner ownerPlaceholder` (an opaque Lean constant — see ExampleGenerated note),
and `signer`/`mut` emit `Constraint.signer`/`Constraint.mut`.

- [ ] **Step 1: Write the failing test**

Create `rust/verified-anchor/tests/lean_spec.rs`:
```rust
use verified_anchor::VerifiedAccounts;

#[derive(VerifiedAccounts)]
struct Transfer {
    #[account(mut)]
    vault: u8,
    #[account(signer)]
    authority: u8,
}

#[test]
fn lean_spec_matches() {
    let expected = "\
{ programId := Pubkey.zero
, fields :=
  [ { name := \"vault\", ty := AccountType.uncheckedAccount, constraints := [Constraint.mut] }
  , { name := \"authority\", ty := AccountType.uncheckedAccount, constraints := [Constraint.signer] } ] }";
    assert_eq!(Transfer::lean_spec(), expected);
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test lean_spec 2>&1 | tail -10`
Expected: COMPILE FAILURE — `Transfer::lean_spec` does not exist.

- [ ] **Step 3: Implement `lean_spec` codegen**

In `rust/verified-anchor-macros/src/lib.rs`, add a function that renders the spec string at
*macro* time (the field names + constraint kinds are known at expansion), and emit it as a
string literal. Add this function:
```rust
fn lean_constraint(c: &Constraint) -> String {
    match c {
        Constraint::Signer => "Constraint.signer".to_string(),
        Constraint::Mut => "Constraint.mut".to_string(),
        Constraint::Owner(_) => "Constraint.owner ownerPlaceholder".to_string(),
    }
}

fn lean_spec_string(specs: &[FieldSpec]) -> String {
    let mut fields = Vec::new();
    for spec in specs {
        let cs: Vec<String> = spec.constraints.iter().map(lean_constraint).collect();
        fields.push(format!(
            "  {{ name := \"{}\", ty := AccountType.uncheckedAccount, constraints := [{}] }}",
            spec.name,
            cs.join(", ")
        ));
    }
    // join fields as a Lean list with leading `[ ` and `, ` separators / `] }` close
    let body = if fields.is_empty() {
        "[]".to_string()
    } else {
        let mut lines = String::from("\n  [ ");
        lines.push_str(fields[0].trim_start());
        for f in &fields[1..] {
            lines.push_str("\n  , ");
            lines.push_str(f.trim_start());
        }
        lines.push_str(" ]");
        lines
    };
    format!("{{ programId := Pubkey.zero\n, fields :={} }}", body)
}
```
NOTE: the exact whitespace must match the test's `expected` string. After implementing,
run the test; if it fails on whitespace, adjust EITHER the generator OR the test's
`expected` so they agree (the test is the spec of the format — pick one canonical form and
make both match). The semantic requirement: valid Lean that elaborates against the M1
`AccountsStruct`/`AccountType`/`Constraint` definitions.

Then extend the derive output to include `lean_spec`. Change the `expanded` in
`derive_verified_accounts` to:
```rust
    let lean = lean_spec_string(&specs);
    let expanded = quote! {
        impl ::verified_anchor::Validate for #name {
            #body
        }
        impl #name {
            /// The Milestone-1 `AccountsStruct` literal for this struct (Lean source).
            pub fn lean_spec() -> ::std::string::String {
                #lean.to_string()
            }
        }
    };
```

- [ ] **Step 4: Run the test (and behavior tests) to confirm pass**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor 2>&1 | tail -15`
Expected: all behavior tests + `lean_spec_matches` pass.

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-macros/src/lib.rs rust/verified-anchor/tests/lean_spec.rs
git commit -m "feat(macros): generate lean_spec (M1 AccountsStruct literal)"
```

---

## Task 4: Lean — the operational generated-validator model

**Files:** Create `lean/VerifiedAnchor/Codegen/Generated.lean`.

- [ ] **Step 1: Write the module**

Create `lean/VerifiedAnchor/Codegen/Generated.lean`:
```lean
import VerifiedAnchor.Contract.Validates

namespace VerifiedAnchor

/-- Per-constraint Bool checks, exactly transcribing the generated Rust `if`s. -/
def genSigner (a : AccountInfo) : Bool := a.isSigner
def genMut    (a : AccountInfo) : Bool := a.isWritable
def genOwner  (expected : Pubkey) (a : AccountInfo) : Bool := decide (a.owner = expected)

/-- Operational check of one M2 constraint against the resolved account. Constraints
    outside the M2 subset are not generated, so they return `false` here. -/
def genConstraint (a : AccountInfo) : Constraint → Bool
  | .signer  => genSigner a
  | .mut     => genMut a
  | .owner e => genOwner e a
  | _        => false

/-- The generated per-field check: resolve the account, then every (implied ++ explicit)
    constraint must pass. A missing account fails. -/
def genFieldValidate (s : AccountsStruct) (c : Ctx) (idx : Nat) (f : AccountField) : Bool :=
  match Ctx.atField s c idx with
  | none   => false
  | some a => (f.ty.impliedConstraints ++ f.constraints).all (genConstraint a)

/-- The generated validator: well-formed account count, then every field validates.
    Mirrors the emitted Rust `validate` (positional, short-circuiting). -/
def genValidate (s : AccountsStruct) (c : Ctx) : Bool :=
  decide (c.length = s.fields.length) &&
    s.fields.zipIdx.all (fun p => genFieldValidate s c p.2 p.1)

end VerifiedAnchor
```

- [ ] **Step 2: Build**

Run: `cd /home/parth/Desktop/PARTH/Verification/lean && export PATH="$HOME/.elan/bin:$PATH" && lake build VerifiedAnchor.Codegen.Generated`
Expected: success. If `(... ).all f` argument order errors, recall `List.all : List α → (α → Bool) → Bool` so `l.all (genConstraint a)` is correct; if not, use `List.all (f.ty.impliedConstraints ++ f.constraints) (genConstraint a)`.

- [ ] **Step 3: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/VerifiedAnchor/Codegen/Generated.lean
git commit -m "feat(codegen): operational genValidate model of the generated validator"
```

---

## Task 5: Lean — per-constraint soundness lemmas

**Files:** Create `lean/VerifiedAnchor/Codegen/Soundness.lean`.

- [ ] **Step 1: Write the per-constraint lemmas**

Create `lean/VerifiedAnchor/Codegen/Soundness.lean`:
```lean
import VerifiedAnchor.Codegen.Generated

namespace VerifiedAnchor

/-- Given the account resolves, `genSigner` agrees with the `signer` contract case. -/
theorem genConstraint_signer_iff (s c idx f a) (h : Ctx.atField s c idx = some a) :
    genConstraint a Constraint.signer = true ↔ satisfies s c idx f Constraint.signer := by
  simp [genConstraint, genSigner, satisfies, Option.satisfiesSome, h]

theorem genConstraint_mut_iff (s c idx f a) (h : Ctx.atField s c idx = some a) :
    genConstraint a Constraint.mut = true ↔ satisfies s c idx f Constraint.mut := by
  simp [genConstraint, genMut, satisfies, Option.satisfiesSome, h]

theorem genConstraint_owner_iff (s c idx f a e) (h : Ctx.atField s c idx = some a) :
    genConstraint a (Constraint.owner e) = true ↔ satisfies s c idx f (Constraint.owner e) := by
  simp [genConstraint, genOwner, satisfies, Option.satisfiesSome, h]

end VerifiedAnchor
```
NOTE: each `satisfies … .signer` unfolds to `(Ctx.atField s c idx).satisfiesSome (fun a => a.isSigner = true)`; with `h` rewriting the option to `some a`, `Option.satisfiesSome (some a) P` simplifies to `P a`. If `simp` doesn't fully close:
- add `Option.some.injEq`, `exists_eq_left'`, or unfold `Option.satisfiesSome` manually with `constructor`/`rintro`;
- for owner, `decide (a.owner = e) = true ↔ a.owner = e` is `decide_eq_true_iff`; include it in the `simp` set.
These three lemmas MUST prove with no `sorry`.

- [ ] **Step 2: Build**

Run: `lake build VerifiedAnchor.Codegen.Soundness`
Expected: success.

- [ ] **Step 3: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/VerifiedAnchor/Codegen/Soundness.lean
git commit -m "feat(codegen): per-constraint soundness lemmas (signer/mut/owner)"
```

---

## Task 6: Lean — `M2Subset` and the generic soundness theorem

**Files:** Modify `lean/VerifiedAnchor/Codegen/Soundness.lean`.

This is the milestone's headline theorem. Append to `Soundness.lean`.

- [ ] **Step 1: Define the M2 subset predicate**

Append:
```lean
namespace VerifiedAnchor

/-- The constraint kinds M2's generated code handles. -/
def isM2Constraint : Constraint → Bool
  | .signer => true | .mut => true | .owner _ => true | _ => false

/-- Account types whose implied constraints stay within the M2 subset
    (everything except `.account`, which implies the out-of-subset `discriminator`). -/
def isM2Type : AccountType → Bool
  | .account _ _ _ => false
  | _ => true

/-- A struct is in the M2 subset when every field uses an M2 type and only M2 constraints. -/
def M2Subset (s : AccountsStruct) : Prop :=
  ∀ f ∈ s.fields, isM2Type f.ty = true ∧ ∀ k ∈ f.constraints, isM2Constraint k = true

instance (s : AccountsStruct) : Decidable (M2Subset s) := by
  unfold M2Subset; infer_instance

end VerifiedAnchor
```

- [ ] **Step 2: Build the predicate**

Run: `lake build VerifiedAnchor.Codegen.Soundness`
Expected: success (the `Decidable` instance elaborates via `List.decidableBAll`).

- [ ] **Step 3: Prove the per-field equivalence lemma**

Append:
```lean
namespace VerifiedAnchor

/-- Under the M2 subset, the generated check of a single resolved constraint agrees with the
    contract's `satisfies`. -/
theorem genConstraint_iff_satisfies (s c idx f a k)
    (h : Ctx.atField s c idx = some a) (hk : isM2Constraint k = true) :
    genConstraint a k = true ↔ satisfies s c idx f k := by
  cases k with
  | signer  => exact genConstraint_signer_iff s c idx f a h
  | mut     => exact genConstraint_mut_iff s c idx f a h
  | owner e => exact genConstraint_owner_iff s c idx f a e h
  | _       => simp [isM2Constraint] at hk

/-- Under the M2 subset, the generated per-field check agrees with `fieldValidates`,
    given the account resolves (guaranteed by well-formedness at the top level). -/
theorem genFieldValidate_iff (s c idx f a)
    (h : Ctx.atField s c idx = some a)
    (htype : isM2Type f.ty = true)
    (hcons : ∀ k ∈ f.constraints, isM2Constraint k = true) :
    genFieldValidate s c idx f = true ↔ fieldValidates s c idx f := by
  unfold genFieldValidate fieldValidates
  rw [h]
  -- Goal: (impl ++ expl).all (genConstraint a) = true ↔ ∀ k ∈ (impl ++ expl), satisfies …
  rw [List.all_eq_true]
  constructor
  · intro hall k hkmem
    have hk : isM2Constraint k = true := by
      -- every k in impl++expl is an M2 constraint: explicit by hcons, implied by htype
      rcases List.mem_append.mp hkmem with himpl | hexpl
      · -- implied constraints of an M2 type are signer-only (or empty)
        revert himpl; cases hf : f.ty <;> simp [AccountType.impliedConstraints, isM2Constraint] <;>
          first | (intro h'; subst h'; rfl) | (simp_all [isM2Type])
      · exact hcons k hexpl
    exact (genConstraint_iff_satisfies s c idx f a k h hk).mp (hall k hkmem)
  · intro hall k hkmem
    have hk : isM2Constraint k = true := by
      rcases List.mem_append.mp hkmem with himpl | hexpl
      · revert himpl; cases hf : f.ty <;> simp [AccountType.impliedConstraints, isM2Constraint] <;>
          first | (intro h'; subst h'; rfl) | (simp_all [isM2Type])
      · exact hcons k hexpl
    exact (genConstraint_iff_satisfies s c idx f a k h hk).mpr (hall k hkmem)

end VerifiedAnchor
```
NOTE: the `cases hf : f.ty` blocks discharge "every implied constraint is M2". For the four
allowed types implied lists are `[signer]`/`[]`; for `.account` the `htype : isM2Type f.ty`
hypothesis is `false = true` after `cases`, closed by `simp_all [isM2Type]`. If the
combinator `first | … | …` is brittle, split into explicit per-type cases:
`| signer => simp [AccountType.impliedConstraints] at himpl; subst himpl; rfl`,
`| uncheckedAccount => simp [AccountType.impliedConstraints] at himpl`,
`| systemAccount => simp [AccountType.impliedConstraints] at himpl`,
`| program _ => simp [AccountType.impliedConstraints] at himpl`,
`| account _ _ _ => simp [isM2Type] at htype`.
Use whichever form is robust. `List.all_eq_true : l.all p = true ↔ ∀ x ∈ l, p x = true` —
confirm the exact name (`List.all_eq_true`); fall back to `by simp [List.all_eq_true]` /
`List.all_iff_forall`.

- [ ] **Step 4: Prove the top-level theorem**

KEY INSIGHT: do NOT split the top conjunction with `and_congr` — that loses the shared
`WellFormed` fact. In the `←` direction you need `WellFormed` (the first conjunct) to prove
each field's account resolves (a field with no constraints is vacuously `fieldValidates` even
when its account is missing, but well-formedness rules that out). So destruct the conjunction
with `rintro ⟨hwf, hall⟩` and thread `hwf` into the per-field resolution. Append:
```lean
namespace VerifiedAnchor

/-- THE MILESTONE-2 THEOREM: the generated validator agrees with the M1 contract for every
    struct in the M2 subset. Proved once, parameterized over the user's annotation. -/
theorem genValidate_sound (s : AccountsStruct) (c : Ctx) (h : M2Subset s) :
    genValidate s c = true ↔ validates s c := by
  unfold genValidate validates
  rw [Bool.and_eq_true, decide_eq_true_iff]
  constructor
  · rintro ⟨hwf, hall⟩
    refine ⟨hwf, ?_⟩
    rw [List.all_eq_true] at hall
    intro p hp
    have hmemf : p.1 ∈ s.fields := (List.mem_zipIdx hp).1
    obtain ⟨htype, hcons⟩ := h p.1 hmemf
    have hgf := hall p hp
    obtain ⟨a, ha⟩ : ∃ a, Ctx.atField s c p.2 = some a := by
      unfold genFieldValidate at hgf
      cases hr : Ctx.atField s c p.2 with
      | none => rw [hr] at hgf; simp at hgf
      | some a => exact ⟨a, rfl⟩
    exact (genFieldValidate_iff s c p.2 p.1 a ha htype hcons).mp hgf
  · rintro ⟨hwf, hall⟩
    refine ⟨hwf, ?_⟩
    rw [List.all_eq_true]
    intro p hp
    have hmemf : p.1 ∈ s.fields := (List.mem_zipIdx hp).1
    obtain ⟨htype, hcons⟩ := h p.1 hmemf
    have hidx : p.2 < s.fields.length := (List.mem_zipIdx hp).2
    obtain ⟨a, ha⟩ : ∃ a, Ctx.atField s c p.2 = some a := by
      have hlt : p.2 < c.length := by rw [hwf]; exact hidx
      unfold Ctx.atField
      exact ⟨c[p.2], (List.getElem?_eq_getElem hlt).symm ▸ rfl⟩
    exact (genFieldValidate_iff s c p.2 p.1 a ha htype hcons).mpr (hall p hp)

end VerifiedAnchor
```
NOTE: the lemma names to confirm in Lean 4.30 (adjust if needed):
- `List.mem_zipIdx` — gives `p.1 ∈ l ∧ p.2 < l.length` (or returns the index bound). If the
  shape differs, use `List.zipIdx` lemmas available (`List.mem_zipIdx_iff_getElem?` etc.) or
  prove the two facts (`p.1 ∈ s.fields`, `p.2 < s.fields.length`) directly by induction.
- `List.getElem?_eq_getElem : i < l.length → l[i]? = some l[i]` — for showing the account
  resolves under well-formedness. Alternative: `List.getElem?_eq_some_iff` / `List.getElem?_lt`.
- `List.all_eq_true` as in Task 5.
- `decide_eq_true_iff` for the `WellFormed` conjunct.
The proof MUST end with no `sorry`. If a lemma name is wrong, find the 4.30 equivalent
(`exact?`/`apply?` are useful) — do not stub.

- [ ] **Step 5: Build and verify no sorry**

Run:
```bash
cd /home/parth/Desktop/PARTH/Verification/lean && export PATH="$HOME/.elan/bin:$PATH"
lake build VerifiedAnchor.Codegen.Soundness
grep -n "sorry\|admit" VerifiedAnchor/Codegen/Soundness.lean || echo "clean"
```
Expected: build succeeds; `clean`.

- [ ] **Step 6: Confirm the theorem is real (no sorryAx)**

Run:
```bash
cd /home/parth/Desktop/PARTH/Verification/lean && export PATH="$HOME/.elan/bin:$PATH"
printf 'import VerifiedAnchor.Codegen.Soundness\n#print axioms VerifiedAnchor.genValidate_sound\n' > VerifiedAnchor/AxTmp.lean
lake env lean VerifiedAnchor/AxTmp.lean; rm -f VerifiedAnchor/AxTmp.lean
```
Expected: axioms are `[propext, Quot.sound]` (and possibly `Classical.choice`), NOT `sorryAx`.

- [ ] **Step 7: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/VerifiedAnchor/Codegen/Soundness.lean
git commit -m "feat(codegen): prove genValidate_sound (generated validator implements M1 contract on M2 subset)"
```

---

## Task 7: Lean — closed-loop generated example

**Files:** Create `lean/VerifiedAnchor/Codegen/ExampleGenerated.lean`.

This pastes the actual `lean_spec()` output for the `Transfer` struct (from Task 3) and
discharges a contract obligation via `genValidate_sound`. `Transfer` uses only `mut`+`signer`
(no `owner`), so the spec is fully concrete and `decide`-able (no opaque `ownerPlaceholder`).

- [ ] **Step 1: Get the canonical macro output for `Transfer`**

The `expected` string asserted in `rust/verified-anchor/tests/lean_spec.rs` (Task 3) IS the
verbatim `Transfer::lean_spec()` output (that test passing proves it). Use exactly that
string as the `transfer` literal in Step 2. (Optional cross-check: add a temporary
`#[test] fn print_spec() { println!("{}", Transfer::lean_spec()); }` and run with
`cargo test -p verified-anchor print_spec -- --nocapture`, then remove it.)

- [ ] **Step 2: Write the example module**

Create `lean/VerifiedAnchor/Codegen/ExampleGenerated.lean`:
```lean
import VerifiedAnchor.Codegen.Soundness

namespace VerifiedAnchor.Examples
open VerifiedAnchor

/-- Opaque placeholder for an `owner = EXPR` whose pubkey is unknown at macro time.
    (Unused by this example, which has no `owner` constraint; declared so emitted specs
    that DO use `owner` still elaborate.) -/
opaque ownerPlaceholder : Pubkey

/-- ▼▼▼ This block is the verbatim output of `Transfer::lean_spec()` (Rust, Task 3). ▼▼▼ -/
def transfer : AccountsStruct :=
{ programId := Pubkey.zero
, fields :=
  [ { name := "vault", ty := AccountType.uncheckedAccount, constraints := [Constraint.mut] }
  , { name := "authority", ty := AccountType.uncheckedAccount, constraints := [Constraint.signer] } ] }
/-- ▲▲▲ end generated block ▲▲▲ -/

/-- A vault account (writable) and an authority account (signer). -/
def goodCtx : Ctx :=
  [ { key := Pubkey.zero, lamports := 0, data := ByteArray.empty, owner := Pubkey.zero,
      rentEpoch := 0, isSigner := false, isWritable := true, executable := false }
  , { key := Pubkey.zero, lamports := 0, data := ByteArray.empty, owner := Pubkey.zero,
      rentEpoch := 0, isSigner := true, isWritable := false, executable := false } ]

/-- Tampered: the authority is not a signer. -/
def tamperedCtx : Ctx :=
  [ goodCtx.get! 0
  , { (goodCtx.get! 1) with isSigner := false } ]

#guard genValidate transfer goodCtx = true
#guard genValidate transfer tamperedCtx = false

/-- `transfer` is in the M2 subset (no `.account` types, only mut/signer). -/
theorem transfer_M2 : M2Subset transfer := by decide

/-- The generated validator accepting the good context PROVES the M1 contract holds —
    via the generic soundness theorem. This is the closed loop: Rust struct → emitted Lean
    spec → machine-checked contract obligation. -/
theorem transfer_good_validates : validates transfer goodCtx :=
  (genValidate_sound transfer goodCtx transfer_M2).mp (by decide)

end VerifiedAnchor.Examples
```
NOTE: if the `with` syntax on `goodCtx.get! 1` is awkward, define the two accounts as named
defs and reuse them. If `#guard`/`decide` is slow, the contexts are tiny so it should be
fine; do NOT switch to `native_decide` (keep the axiom footprint at `[propext, Quot.sound]`).
The literal in `transfer` MUST be exactly the macro's `lean_spec()` output — if Task 3's
format differs, paste the real output here.

- [ ] **Step 3: Build**

Run: `lake build VerifiedAnchor.Codegen.ExampleGenerated`
Expected: success; both `#guard`s pass; both theorems compile.

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/VerifiedAnchor/Codegen/ExampleGenerated.lean
git commit -m "feat(codegen): closed-loop example (macro lean_spec -> genValidate_sound obligation)"
```

---

## Task 8: Bridge doc, root wiring, full builds

**Files:** Create `docs/verified-anchor-bridge.md`; Modify `lean/VerifiedAnchor.lean`.

- [ ] **Step 1: Write the bridge doc**

Create `docs/verified-anchor-bridge.md`:
```markdown
# Verified Anchor — the Rust↔Lean bridge (Milestone 2)

How the generated Rust validator relates to the machine-checked Lean proof, and exactly
what is and isn't proven.

## Clause-by-clause correspondence

| Generated Rust (`validate`) | Lean model (`genConstraint`) | Discharges M1 contract case |
|---|---|---|
| `if !accounts[i].is_signer { Err(MissingSigner) }` | `genSigner a := a.isSigner` | `satisfies … .signer` |
| `if !accounts[i].is_writable { Err(NotWritable) }` | `genMut a := a.isWritable` | `satisfies … .mut` |
| `if accounts[i].owner != &expected { Err(WrongOwner) }` | `genOwner e a := decide (a.owner = e)` | `satisfies … (.owner e)` |
| `if accounts.len() < n { Err(NotEnoughAccounts) }` | `decide (c.length = s.fields.length)` | `WellFormed` |

Per-field, the Rust checks every constraint in order and short-circuits; `genFieldValidate`
folds `genConstraint` with `&&` over `impliedConstraints ++ constraints`. `genValidate`
conjoins well-formedness with all fields.

## What is proven

`theorem genValidate_sound : M2Subset s → (genValidate s c = true ↔ validates s c)` — the
Lean model of the generated validator agrees with the Milestone-1 contract for every struct
in the M2 subset (fields of type signer/unchecked/system/program, constraints in
mut/signer/owner). Proved once, parameterized over the annotation; axioms `[propext, Quot.sound]`.

## What is transcription (documented + tested, not proven)

The Rust `validate` body is a clause-by-clause transcription of `genValidate` per the table
above. This correspondence is NOT machine-checked across the language boundary; it is
verified by shared accept/reject test vectors run in both `rust/verified-anchor/tests/behavior.rs`
and the Lean `#guard`s in `Codegen/ExampleGenerated.lean`.

## What is out of scope

rustc/LLVM/sBPF code generation fidelity — i.e. that the compiled binary faithfully executes
the Rust source. This is the standard boundary of source-level verification (cf. CompCert),
and is not addressed by Verified Anchor at any milestone.
```

- [ ] **Step 2: Wire the Codegen modules into the Lean root**

Edit `lean/VerifiedAnchor.lean` — append after the existing imports:
```lean
import VerifiedAnchor.Codegen.Generated
import VerifiedAnchor.Codegen.Soundness
import VerifiedAnchor.Codegen.ExampleGenerated
```

- [ ] **Step 3: Full builds (both toolchains) + gates**

Run:
```bash
cd /home/parth/Desktop/PARTH/Verification/lean && export PATH="$HOME/.elan/bin:$PATH" && lake build 2>&1 | tail -4
grep -rn "sorry\|admit" VerifiedAnchor/ && echo "SORRY FOUND (FAIL)" || echo "PASS: lean zero-sorry"
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test 2>&1 | tail -8
```
Expected: `lake build` green; `PASS: lean zero-sorry`; all Rust tests pass.

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add lean/VerifiedAnchor.lean docs/verified-anchor-bridge.md
git commit -m "docs+wiring: bridge doc and Codegen root imports; M2 build/test green"
```

---

## Done-bar verification (run after Task 8)

1. `cargo test` green in `rust/` (behavior + lean_spec). ✅ (Task 8 Step 3)
2. `lake build` green, zero `sorry`, including `Codegen`. ✅ (Task 8 Step 3)
3. `genValidate_sound` proved, no `sorryAx`. ✅ (Task 6 Step 7)
4. Three per-constraint lemmas proved. ✅ (Task 5)
5. `ExampleGenerated`: `genValidate` true/false + `transfer_good_validates` via the generic theorem. ✅ (Task 7)
6. `docs/verified-anchor-bridge.md` documents correspondence + trust boundary. ✅ (Task 8)
7. M1 still green (full `lake build` includes M1 modules). ✅ (Task 8 Step 3)
```
