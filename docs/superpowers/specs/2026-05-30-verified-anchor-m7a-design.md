# Verified Anchor — Milestone 7a Design

*Real `Account<'info, T>` typing: lift the derive macro from `u8` spec-carriers to Anchor-style typed wrappers + Borsh + `Context<T>`, so a verified-anchor instruction handler is signature-identical to a stock-Anchor one. Foundation for the M7b release.*

Status: **approved design** (2026-05-30). Target: the first of three M7 sub-milestones (M7a = real typed API; M7b = release; M7c = announcement + QEDGen demo). Builds on M1–M6 (all on `master`).

---

## 1. Goal and context

Through M6 the verified-anchor derive accepts `u8` spec-carrier fields with explicit `#[account(...)]` constraints. The Lean model already proves the full validation surface (M1–M4), the runtime enforces it (D1's discriminator closed the last gap), and M5's check discharges per-struct M4-subset contracts via `lake`. What's missing for adoption is the **developer-facing surface**: real Anchor programs use `Account<'info, T>`/`Signer<'info>`/`Program<'info,P>` etc., with typed field access (`ctx.accounts.vault.amount`) via Borsh deserialization. The M5/M6 honest-boundary notes flagged this as the M7 deliverable that makes "drop-in for stock Anchor" credible.

M7a does that lift, **without re-doing any proofs**: the proven validation gate (`validate(accounts, instr_data, program_id)`) stays exactly as it is — the M2–M5 theorems and the D1 discriminator codegen still apply — and a new `Accounts<'info>` trait sits **on top** of it as the developer surface (calling `validate` then Borsh-deserializing each typed field).

### Decisions locked during brainstorming
1. **Full drop-in** (Layer 1 + Layer 2): typed wrappers + Borsh deserialization + `Context<T>` shape matching stock Anchor.
2. **Clean break**: a bare `u8` field becomes a `compile_error!` post-M7a; **all** ~20 existing M1–M6 derived structs migrate to typed wrappers in the same milestone. One API surface, no transition tech debt.
3. **Mirror real Anchor's `Context<'a, 'b, 'c, 'info, T>` signature exactly** so users' muscle memory carries over.
4. **Two coexisting trait layers**: keep `Validate` (the proven `validate(accounts, instr_data, program_id) -> Result<(), VAError>` — the Rust↔Lean transcription boundary stays unchanged) **and** add `Accounts<'info>` (the developer surface: `try_accounts(program_id, accounts, instr_data) -> Result<Self>` that internally calls `validate` then Borsh-deserializes typed fields). This means **no Lean changes, no bridge-doc rewrite** — only the `lean_spec` "Vault"-hardcode (an M3 follow-up) gets closed as a bonus.

---

## 2. The six typed wrappers + auto-implied checks

The macro recognizes these field types (parsed via `syn::Type` pattern matching on the path/generic-arg shape). Each implies a set of runtime checks that **stack** with any explicit `#[account(...)]` attributes on the field.

| Wrapper                          | Implied runtime checks                                          | Borsh deser? |
|----------------------------------|-----------------------------------------------------------------|--------------|
| `Account<'info, T>`              | `owner == crate::ID` AND `data[0..8] == T::DISCRIMINATOR`       | yes (of T)   |
| `Signer<'info>`                  | `is_signer`                                                     | no           |
| `Program<'info, P>`              | `executable` AND `key == P::ID`                                 | no           |
| `SystemAccount<'info>`           | `owner == solana_program::system_program::ID`                   | no           |
| `UncheckedAccount<'info>`        | none (escape hatch — same as stock Anchor)                      | no           |
| `AccountInfo<'info>`             | none (raw)                                                      | no           |

A bare field type (e.g. `vault: u8`) → `compile_error!("verified-anchor: bare field types are no longer supported; use a typed wrapper like Account<'info, Vault>, Signer<'info>, etc. See docs/migrating-from-anchor.md")`.

Explicit `#[account(...)]` attributes still parse and stack:
- `Account<'info, T>` with `#[account(has_one = bank)]` → owner + discriminator + has_one.
- `Account<'info, T>` with `#[account(seeds = [...], bump)]` → owner + discriminator + seeds.
- `Account<'info, T>` with `#[account(init, payer = p, space = n)]` → init effect (M3) + the wrapper's owner/disc constraints apply to the POST-init state (the init step writes the discriminator).
- `Account<'info, T>` with `#[account(mut)]` → also writable.

**The proven `validate` body** is unchanged in shape: it emits the same per-constraint checks in field order. Implied constraints from the wrapper are emitted first, then explicit attribute constraints, identical to how `AccountType.impliedConstraints` already works in the Lean model.

---

## 3. `AccountData` + `ProgramId` traits + derives

User types carried by `Account<'info, T>` need a discriminator and Borsh deser. Marker types carried by `Program<'info, P>` need a `const Pubkey` so the wrapper can check `accounts[i].key == &P::ID`.

```rust
pub trait ProgramId {
    const ID: Pubkey;
}
```

A user (or the verified-anchor prelude) provides marker types like `pub struct System; impl ProgramId for System { const ID: Pubkey = solana_program::system_program::ID; }`. Verified-anchor ships markers for the common programs (System, Token, AssociatedToken) in `prelude::*`; users can write their own for other programs.

The Account-data side:

```rust
pub trait AccountData: borsh::BorshDeserialize + borsh::BorshSerialize {
    const DISCRIMINATOR: [u8; 8];   // sha256("account:" + Name)[..8] — real Anchor wire
}

// new proc-macro in verified-anchor-macros:
#[derive(AccountData)]
pub struct Vault {
    pub authority: Pubkey,
    pub amount: u64,
}
// expands to:
//   #[derive(borsh::BorshDeserialize, borsh::BorshSerialize)]   (added by the proc-macro)
//   impl AccountData for Vault {
//       const DISCRIMINATOR: [u8; 8] = <sha256-computed bytes at macro time>;
//   }
```

The derive uses the same `sha2` dep D1 added to the macro crate. The bytes match real Anchor — so verified-anchor can validate real Anchor accounts (interop), and the M6 cross-check (`d308e82b02987577` for `"Vault"`) still holds.

`Account<'info, T>`'s codegen uses `T::DISCRIMINATOR` for the runtime check — no per-field `discriminator = "Name"` attribute is needed (it's implied by the type). The explicit attribute still works as an override (e.g. for cross-program accounts whose type is in another crate).

---

## 4. `Context<T>` shape (mirrors stock Anchor)

```rust
pub struct Context<'a, 'b, 'c, 'info, T: Accounts<'info>> {
    pub accounts: T,
    pub program_id: &'a Pubkey,
    pub remaining_accounts: &'c [AccountInfo<'info>],
    pub bumps: T::Bumps,
    _phantom: PhantomData<&'b ()>,
}
```

`T::Bumps` is an associated type — for structs with no seeds/bump fields it's `()`; for structs with seeds + canonical bump it's a generated struct of `u8` fields named after the seed fields (matching `ctx.bumps.vault` access in stock Anchor). M7a's first cut emits `Bumps = ()` for the no-seeds case and a minimal `<Name>Bumps` struct for the seeds case; full parity with Anchor's bumps API is a polish item for M7b.

User instruction handlers become signature-identical to real Anchor:
```rust
pub fn handler(ctx: Context<Transfer>) -> Result<(), VAError> {
    let amount = ctx.accounts.vault.amount;
    // ...
}
```

---

## 5. Two trait layers — proven core + developer surface

```rust
// THE PROVEN LAYER (unchanged from M2–M6, the Lean↔Rust transcription boundary):
pub trait Validate {
    fn validate(accounts: &[AccountInfo], instr_data: &[u8], program_id: &Pubkey)
        -> Result<(), VAError>;
}

// THE DEVELOPER SURFACE (NEW in M7a):
pub trait Accounts<'info>: Sized {
    type Bumps;
    fn try_accounts(
        program_id: &Pubkey,
        accounts: &[AccountInfo<'info>],
        instr_data: &[u8],
    ) -> Result<Self, VAError>;
}
```

For each derived struct the macro emits **both** impls:

- `impl Validate` — exactly as today (no body change beyond the typed-wrapper auto-implications + the lean_spec real-type-name fix). The Lean theorems still apply to this layer.
- `impl Accounts<'info>` — `try_accounts` first calls `<Self as Validate>::validate(accounts, instr_data, program_id)` (the proven gate), then constructs `Self` by populating each typed field from `accounts[i]`:
  - `Account<'info, T>`: `T::try_from_slice(&accounts[i].data.borrow()[8..])` (skip the discriminator, which `validate` already checked) → `Account { info: &accounts[i], data: <deserialized T> }`.
  - `Signer<'info>`: `Signer { info: &accounts[i] }`.
  - `Program<'info, P>`: `Program { info: &accounts[i], _phantom: PhantomData }`.
  - `SystemAccount<'info>` / `UncheckedAccount<'info>` / `AccountInfo<'info>`: just the `AccountInfo` ref.

This composition gives `try_accounts ≡ validate + Borsh-deser`. Borsh deser failures (corrupt data) produce a new `VAError::BorshFailed { field }` variant — but they cannot happen for accounts that passed `validate`'s discriminator check **unless** the bytes after offset 8 are malformed (a separate failure mode from the validation gate). Borsh failures are honest runtime errors, not verification holes.

**`init` fields are deserialized as if zero-initialized.** Stock Anchor handles `init` accounts specially because their post-init data is just `[disc(8)][zeros(space)]` — a Borsh-deser of all-zeros may not produce a valid T for some types. M7a's approach: for fields with the `init` attribute, `try_accounts` populates `Self.<field>` from the **zero-initialized T** (`T::try_from_slice(&[0u8; size_of_T])`) if Borsh accepts that, else returns `BorshFailed`. The user's handler is expected to write the real field values via the wrapper's `Deref/DerefMut` and re-serialize before tx end (matching stock Anchor's pattern, where `ctx.accounts.new.amount = 42` works because `Account<T>` derefs to T).

(For M7a's first cut, the simpler scope: typed wrappers don't auto-re-serialize on drop. The user writes `ctx.accounts.vault.write_back()` (or similar helper) before tx end if they mutated typed fields. Stock Anchor's auto-write-back via drop is a polish item for M7b's release — flag in the report.)

---

## 6. Lean side (`lean_spec` real type name — closes an M3 follow-up)

Today's `lean_spec_string` hardcodes `AccountType.account "Vault" [layout] Pubkey.zero` for any has_one-bearing field — an M3 follow-up tracked in `docs/superpowers/m3-followups.md`. With typed wrappers, the macro reads the real type name from `Account<'info, T>` and emits:
```
AccountType.account "<T>" [layout] crate::ID
```
where `<T>` is the actual type name (e.g. `Collateral`, `Vault`). The `programId` argument becomes a real `Pubkey` (from `declare_id!`) instead of `Pubkey.zero` — small but real.

For non-`Account` wrappers, lean_spec uses the existing AccountType variants:
- `Signer<'info>` → `AccountType.signer`
- `SystemAccount<'info>` → `AccountType.systemAccount`
- `Program<'info, _>` / `UncheckedAccount<'info>` / `AccountInfo<'info>` → `AccountType.uncheckedAccount` (we don't have a `program` Lean variant yet — UncheckedAccount is the safe lean-side representation; the runtime check fires from the Rust side regardless).

**No Lean source changes.** `AccountType.account` already implies owner+discriminator in the M1 Lean model; `M4Subset` already accepts it; `genValidate_sound` already proves it. The M5 `check` still discharges. We're just emitting nicer, real-type-name-accurate `lean_spec` strings — the M3 follow-up closes for free.

---

## 7. Migration scope (clean break)

Every existing `#[derive(VerifiedAccounts)]` struct in the workspace migrates to typed wrappers (and so do the user types they reference get `#[derive(AccountData)]`). The full list:

- `rust/verified-anchor-program/src/lib.rs` — `InitOne`, `CloseOne`, `CheckPda` (3 structs).
- `rust/verified-anchor-example/src/lib.rs` — `CheckPda`, `Transfer`, `Lifecycle` (3 structs).
- `rust/verified-anchor-exploits/src/lib.rs` — `VerifiedCashio`, `VerifiedConfusion`, `VerifiedCrema`, `VerifiedSeeds` (4 structs) + **new** `#[derive(AccountData)]` on `Collateral`, `Vault`, `Config` (user types).
- `rust/verified-anchor/tests/behavior.rs` — `Transfer`, `OwnedVault`, `CheckOwner`, `PdaAccount`, `PdaDeclaredBump`, `DiscOnly`, `InitClose` (7 structs).
- `rust/verified-anchor/tests/lean_spec.rs` — `Transfer`, `PdaSpec`, `InitClose`, `DiscSpec` (4 structs).
- `rust/verified-anchor-macros/tests/ui/unsupported_constraint.rs` — `Bad` (1 struct; the trybuild fixture).

~22 structs total. Mechanical (each: replace `u8` with the right wrapper; drop the constraints the wrapper now implies; the runtime tests are byte-identical because the runtime checks are the same). The litesvm tests assertions don't change — same naive arms, same verified arms, same on-chain behaviour.

Also update `docs/migrating-from-anchor.md` — the migration table now shows the syntax matching real Anchor much more closely (a major adoption win).

---

## 8. Repository additions

```
rust/verified-anchor/
├── src/
│   ├── lib.rs                  (MODIFY) trim Validate to existing form; add Accounts trait + Bumps assoc-type; pub use prelude
│   ├── account.rs              (NEW) the 6 wrapper structs (Account, Signer, Program, SystemAccount, UncheckedAccount, AccountInfo
│   │                                  re-export) + their Deref/DerefMut where applicable
│   ├── context.rs              (NEW) Context<'a, 'b, 'c, 'info, T>
│   ├── account_data.rs         (NEW) AccountData trait
│   └── prelude.rs              (NEW) pub use Account, Signer, Program, SystemAccount, UncheckedAccount, AccountInfo, Context,
│                                     AccountData (the trait), VerifiedAccounts (re-export of the derive), AccountData (derive)
├── Cargo.toml                  (MODIFY) + borsh = "1"
rust/verified-anchor-macros/
├── src/lib.rs                  (MAJOR REWRITE) field-type parsing (syn::Type) -> wrapper recognition; per-wrapper codegen
│                                              for both Validate (existing shape) and Accounts (new); bare-u8 -> compile_error;
│                                              lean_spec uses real type name
└── src/account_data_derive.rs  (NEW) #[derive(AccountData)] proc-macro: derive Borsh + compute DISCRIMINATOR via sha256

(MIGRATION — see §7 list)
docs/migrating-from-anchor.md   (MODIFY) update to the typed-wrapper syntax; show near-1:1 mapping to stock Anchor
docs/verified-anchor-bridge.md  (MODIFY) one-paragraph addendum: Accounts trait sits on top of Validate; the proven layer is unchanged
docs/superpowers/m3-followups.md  (MODIFY) check off "lean_spec_string hardcodes Vault" (closed in M7a)
```

---

## 9. Testing / gates

- **Native:** all existing `behavior.rs`, `lean_spec.rs`, `compile_fail.rs` tests still pass after migration to typed wrappers; new behaviour tests for the wrapper auto-implications (each wrapper's accept/reject path); a new test for `try_accounts` returning a typed struct with Borsh-deserialized fields; `compile_fail` adds a `tests/ui/bare_u8.rs` fixture asserting bare-`u8` field errors.
- **litesvm:** `runtime_lifecycle.rs`, `runtime_seeds.rs`, `runtime_exploits.rs` all stay green after the program crates migrate to typed wrappers — same on-chain behaviour, same naive vs verified before/after assertions.
- **M5 check:** `cargo verified-anchor check -p verified-anchor-exploits` (and `verified-anchor-example`) still exits 0 — the `lean_spec` change is to the type name only, M4Subset decide still passes.
- **Lean:** `lake build` green (no Lean source changes); the `#print axioms` of the M1–M5 headline theorems unchanged.
- **MANDATORY full gate** (per HANDOVER): SBF builds clean + `cargo test --workspace` all green.

---

## 10. Scope / non-goals (M7a)

**In.** The 6 typed wrappers + auto-implications; Borsh deserialization in `try_accounts`; the `AccountData` trait and derive; `Context<T>`; the `Accounts<'info>` trait alongside the existing `Validate`; full migration of all existing derived structs (~22) to typed wrappers; bare-`u8` → compile_error; `lean_spec` real type name + real programId; docs update (migration guide + bridge addendum); a `compile_fail` fixture for bare-u8.

**Out.** Release packaging (M7b: license, README polish, version 1.0, crates.io publish). Announcement/blog post (M7c). QEDGen integration (M7c, gated on QEDGen availability). Auto-write-back of typed fields on drop (stock-Anchor polish item; M7b). `AccountLoader<T>` for zero-copy access (stock Anchor has it; not needed for v1). `Sysvar<T>` wrapper (deferred). Multi-program workspaces (deferred). Re-deriving the M1–M5 theorems (unchanged — the proven layer doesn't move).

---

## 11. Done-bar for Milestone 7a

1. Macro accepts the 6 typed wrappers; a bare `u8` field produces a `compile_error!` pointing at the migration guide (trybuild fixture asserts the message).
2. `#[derive(AccountData)]` computes `DISCRIMINATOR = sha256("account:" + Name)[..8]` and derives `BorshDeserialize`+`BorshSerialize`; an independent sha256 cross-check in `behavior.rs` confirms the bytes match real Anchor.
3. Each derived struct gets both `impl Validate` (proven) and `impl Accounts<'info>` (developer surface) with `try_accounts` calling `validate` then Borsh-deserializing typed fields.
4. `Context<'a, 'b, 'c, 'info, T>` exists and matches stock Anchor's signature; user instruction handlers can be written as `pub fn h(ctx: Context<MyAccs>) -> Result<()>`.
5. All ~22 existing derived structs migrated; ALL existing M1–M6 tests still pass (native, litesvm, M5 check, compile_fail); SBF `.so`s build clean.
6. `lean_spec` emits real type names + `crate::ID` (no more "Vault" hardcode / `Pubkey.zero` placeholder for typed accounts); `M4Subset` decides true on the new specs; `cargo verified-anchor check` discharges.
7. `docs/migrating-from-anchor.md` updated to show the now-near-drop-in syntax; bridge-doc addendum present; m3-followups.md "Vault hardcode" item checked off.
8. `lake build` green, zero `sorry`; M1–M5 headline theorems' `#print axioms` unchanged (`[propext, Quot.sound]`).
