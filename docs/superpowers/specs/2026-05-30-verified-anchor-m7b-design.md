# Verified Anchor — Milestone 7b Design

**Status:** approved 2026-05-30. M7a (real `Account<'info, T>` typing) shipped immediately before.

**Goal:** make verified-anchor publishable on crates.io with a first-class developer surface — Anchor-style `#[account]` attribute macro, real per-seed `Bumps`, and the release-engineering metadata + LICENSE files needed for `cargo publish`.

**Non-goals:**
- No Lean source changes (M1-M5 axioms must stay `[propext, Quot.sound]`).
- No new proof obligations (the new attribute macro emits user-data struct, not a `VerifiedAccounts` struct — outside the proven surface).
- We do not run `cargo publish` ourselves. The user runs it after reviewing the prepared metadata.

---

## Component A — `#[account]` attribute macro

**Problem:** today users write
```rust
#[derive(borsh::BorshSerialize, borsh::BorshDeserialize, verified_anchor::AccountData)]
pub struct Vault { pub authority: Pubkey, pub amount: u64 }
```
Three derives, two crate paths. Real Anchor users write `#[account] pub struct Vault { ... }`. The verbosity is the largest gap between verified-anchor and stock Anchor at the user-facing surface.

**Design.** A new `#[proc_macro_attribute] pub fn account(args, input)` in `rust/verified-anchor-macros/src/account_attr.rs`. The macro:

1. Parses `input` as a `syn::ItemStruct` (named-fields struct). Rejects enums/unions/tuple-structs with a `compile_error!`.
2. Accepts no attribute args (`#[account(...)]` is reserved for future use — discriminator overrides, zero-copy mode, etc. — but M7b takes no args; passing args is a `compile_error!`).
3. Re-emits the original struct with three derives prepended:
   ```rust
   #[derive(::borsh::BorshSerialize, ::borsh::BorshDeserialize, ::verified_anchor::AccountData)]
   pub struct Vault { ... }
   ```
   Note: `::verified_anchor::AccountData` works because M7a re-exported the derive at the `verified_anchor` crate root (commit `420aecc`).
4. The derive expansion order is left-to-right; Borsh derives must run before `AccountData` (since AccountData only needs the ident, this is fine in either order, but we put Borsh first to match stock Anchor's ordering).

**Re-export.** Add `pub use verified_anchor_macros::account;` to `verified_anchor/src/lib.rs` and to `verified_anchor::prelude`.

**Back-compat.** The manual 3-derive form keeps working — the new attribute is pure sugar. Users who want different Borsh derive flags (e.g. `BorshSchema`) keep writing the explicit form.

**Tests.** A new behavior test in `rust/verified-anchor/tests/behavior.rs`:
```rust
#[verified_anchor::account]
pub struct VaultAttr { pub authority: Pubkey, pub amount: u64 }

#[test] fn account_attribute_implies_borsh_and_discriminator() {
    let d = <VaultAttr as verified_anchor::AccountData>::DISCRIMINATOR;
    assert_eq!(d, disc("VaultAttr"));
    let v = VaultAttr { authority: Pubkey::new_from_array([7; 32]), amount: 42 };
    let bytes = borsh::to_vec(&v).unwrap();
    let v2: VaultAttr = borsh::from_slice(&bytes).unwrap();
    assert_eq!(v2.amount, 42);
    assert_eq!(v2.authority, v.authority);
}
```
Plus a trybuild `compile_fail` fixture (`tests/ui/account_with_args.rs`) asserting `#[account(foo)]` errors with a helpful message.

---

## Component B — Richer per-seed `Bumps`

**Problem:** M7a emits `pub struct <Name>Bumps;` (empty marker, satisfies the trait's assoc type but carries no data). Stock Anchor emits a struct with one `pub <field>: u8` per `#[account(... seeds = [...], bump)]` field, populated to the canonical bump from `find_program_address`. Without this, the verified-anchor `Context.bumps.pda` shape is incompatible with handler code copy-pasted from stock Anchor.

**Design.** In `derive_verified_accounts` (`rust/verified-anchor-macros/src/lib.rs`):

1. After collecting `specs`, build a `seeded_fields: Vec<(usize, &str)>` = `(index, name)` for every spec whose constraints contain `Constraint::Seeds(_)`.
2. Emit the Bumps struct conditionally:
   - `seeded_fields.is_empty()` → keep emitting `pub struct <Name>Bumps;` (unchanged).
   - Otherwise → emit `pub struct <Name>Bumps { pub <f1>: u8, pub <f2>: u8, ... }` (one field per seeded PDA, in declaration order).
3. In `try_accounts`, after the `validate(...)?` call but before constructing `Self`, compute the canonical bump for each seeded field and pack into a Bumps initialiser:
   ```rust
   let bumps = <Name>Bumps {
       <f1>: { let (_pda, b) = ::solana_program::pubkey::Pubkey::find_program_address(<seeds_f1>, program_id); b },
       ...
   };
   ```
   then return `Ok(Self { <field_inits>, /* bumps consumed by caller via Context::new */ })` — but wait, `Bumps` lives on `Context`, not `Self`. Re-read the M7a Context shape.

**Bumps lifecycle.** `Context<T: Accounts<'info>>` has `bumps: T::Bumps`. The `Accounts<'info>` trait's `try_accounts` returns `Self` (the accounts struct). `Bumps` is NOT inside `Self`. So where does it come from?

Two options:
- **(B-i) Change `try_accounts` to return `(Self, Self::Bumps)`** — cleanest, but a trait-signature break for M7a users (only us — no external users yet).
- **(B-ii) Add `Self::Bumps`-returning helper alongside `try_accounts`** — e.g. `fn try_bumps(...)` — uglier, two-step.

**Decision: B-i.** Change the M7a `Accounts` trait to `fn try_accounts(...) -> Result<(Self, Self::Bumps), VAError>`. The `Context` shape is `Context::new(program_id, accounts, remaining_accounts, bumps)` — already takes bumps separately, so the caller does:
```rust
let (accounts, bumps) = T::try_accounts(program_id, accounts, instr_data)?;
let ctx = Context::new(program_id, accounts, remaining_accounts, bumps);
```
This matches stock Anchor's internal generated code shape.

**Trade-off:** the empty-Bumps case still returns `((), ())`-style — actually it returns `(Self, <Name>Bumps)` where `<Name>Bumps` is the empty struct. Callers of non-seeded structs ignore the second tuple element (or use `_`). Acceptable.

**Refactor scope.** The M7a `try_accounts` body is small; updating the return type is mechanical. No call sites in the workspace use `try_accounts` directly except the new behavior tests added at the end of M7a (`try_accounts_deserializes_typed_data`, `try_accounts_borsh_failed_on_truncated_data`). Update those tests to unpack `(accounts, _bumps)`.

**Tests.** Add a behavior test:
```rust
#[derive(VerifiedAccounts)]
struct WithPda<'info> {
    #[account(seeds = [b"vault", arg(0,4)], bump)]
    pda: UncheckedAccount<'info>,
}

#[test] fn bumps_struct_carries_canonical_bump() {
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

**One concern flagged for the plan.** The `find_program_address` runs in `validate` (to compute the canonical bump for the equality check). Calling it AGAIN in `try_accounts` doubles the work for any struct with a PDA. Cheap enough (~30µs on-chain per call), but if perf matters we can refactor to memoize. **Plan deliberately ignores this — YAGNI.**

---

## Component C — crates.io publish prep

**Problem:** the workspace `Cargo.toml`s lack `description`, `license`, `repository`, `keywords`, `categories`. No `LICENSE-*` files. No top-level README ready to be the crates.io landing page. `cargo publish --dry-run` would fail or produce a confusing crates.io page.

**Design.**

### C.1 — LICENSE files at the repo root
Add `LICENSE-MIT` (standard MIT license text) and `LICENSE-APACHE` (standard Apache-2.0 license text), both copyright `Parth Arvind Rathi 2026`. The dual license is the Rust ecosystem default.

### C.2 — Workspace metadata
Update each publishable crate's `Cargo.toml` `[package]` section:
- `version = "0.1.0"` (initial release).
- `edition = "2021"` (already there).
- `description = "..."` per crate (one sentence each):
  - **verified-anchor:** "Formally verified (Lean 4) account-validation runtime for Solana — Anchor-compatible, proof-producing."
  - **verified-anchor-macros:** "Proof-producing proc-macros for verified-anchor (`#[derive(VerifiedAccounts)]`, `#[derive(AccountData)]`, `#[account]`)."
  - **cargo-verified-anchor:** "Cargo subcommand that discharges verified-anchor proof obligations via Lean."
- `license = "Apache-2.0 OR MIT"`.
- `repository = "https://github.com/<user>/Verification"` — **TBD by user before publish** (placeholder for now; flag in publish checklist).
- `keywords = ["solana", "anchor", "verified", "lean4", "proc-macro"]` (max 5, length ≤ 20 each — crates.io constraint).
- `categories = ["development-tools", "cryptography"]` (valid crates.io categories).
- `readme = "../README.md"` (workspace root README; same one for all 3 crates).
- `homepage` — same as `repository` for v0.1.0.

### C.3 — Top-level `README.md`
Polish the existing repo-root README (if any) into a crates.io-ready landing page. Sections:
1. **Tagline + status badge mock.** "Verified Anchor: formally verified account validation for Solana." Status: 0.1.0 alpha.
2. **What it is** — 2 paragraphs covering: Lean 4 contract + proof-producing Rust macros + Anchor-drop-in.
3. **Quick start** — the M7a typed-wrapper struct + `try_accounts` snippet.
4. **What's proven (and what isn't).** Links to `docs/verified-anchor-bridge.md` for the trust boundary.
5. **Compatibility with stock Anchor.** Links to `docs/migrating-from-anchor.md` for the syntax mapping.
6. **Empirical validation.** Links to `docs/exploit-case-studies.md` (M6).
7. **Roadmap.** M7c next (QEDGen + announcement).
8. **License.** Apache-2.0 OR MIT.

### C.4 — Publish checklist (committed as docs/publish-checklist.md)
A short file the user follows:
1. Edit `repository`/`homepage` in all 3 Cargo.tomls to the real GitHub URL.
2. Run `cargo publish --dry-run -p verified-anchor-macros` (publish order: macros → runtime → cargo subcommand).
3. Run `cargo publish -p verified-anchor-macros && sleep 60 && cargo publish -p verified-anchor && sleep 60 && cargo publish -p cargo-verified-anchor`.
4. The sleeps let the new index entry propagate so the downstream crate sees the dep.

### C.5 — Things we don't publish
The on-chain test programs (`verified-anchor-program`, `verified-anchor-exploits`, `verified-anchor-example`) are test fixtures, NOT for crates.io. Mark them `publish = false` in their `Cargo.toml`s.

**Tests for C.** `cargo publish --dry-run` for each of the 3 publishable crates as part of the final gate. No new unit tests needed.

---

## Risks & soundness boundary

- **Component A** sits OUTSIDE the proven surface (it's user-data sugar, not validation codegen). Borsh derives + AccountData are existing M6/M7a-tested machinery; we're just bundling them.
- **Component B** changes the `Accounts` trait's `try_accounts` return type from `Result<Self, VAError>` to `Result<(Self, Self::Bumps), VAError>`. This is the only signature break in M7b. No external users to break (pre-publish), and our own behavior tests are the only call sites — explicitly updated.
- **Component C** is pure metadata + docs. No code change.
- **Lean axioms** stay `[propext, Quot.sound]` — no Lean source touched. The mandatory full gate (`#print axioms` audit) re-verifies this.

---

## File structure

| File | Action | Component |
|------|--------|-----------|
| `rust/verified-anchor-macros/src/account_attr.rs` | NEW | A |
| `rust/verified-anchor-macros/src/lib.rs` | MODIFY (wire attribute macro + Bumps codegen) | A, B |
| `rust/verified-anchor/src/lib.rs` | MODIFY (Accounts trait return type, re-export `account`) | B, A |
| `rust/verified-anchor/src/prelude.rs` | MODIFY (re-export `account`) | A |
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
| `README.md` | NEW or MODIFY | C |
| `docs/publish-checklist.md` | NEW | C |

---

## Done-bar

1. `#[account]` attribute compiles a struct with the 3 bundled derives; the manual 3-derive form still works. Behavior test passes; trybuild fixture asserts arg-rejection.
2. Seeded structs emit a `<Name>Bumps` struct with one `pub <f>: u8` per seeded field; `try_accounts` returns `(Self, Bumps)` populated with canonical bumps. Non-seeded structs emit empty `<Name>Bumps`. Bumps behavior test passes.
3. All 3 publishable crates pass `cargo publish --dry-run`. LICENSE files + README + publish-checklist committed.
4. Mandatory full gate green: lake build + zero sorry; `genValidate_sound`/`lifecycle_sound` axioms still `[propext, Quot.sound]`; both SBF `.so`s clean (no PT_DYNAMIC); `cargo test --workspace` all green; both M5 checks discharge.
5. No M1-M5 axioms changed.
