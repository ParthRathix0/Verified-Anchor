# Verified Anchor — Milestone 6 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Empirically validate verified-anchor against four real Solana account-validation exploit classes — each reproduced as a litesvm before/after (naive succeeds + drains; verified rejects the same attacker accounts) — and close the Lean↔Rust discriminator gap as a prod-ready macro feature so the runtime actually enforces what Lean proves.

**Architecture:** A new BPF program crate `verified-anchor-exploits` holds 4 `naive_<s>` + 4 `verified_<s>` instruction arms; the verified arms use the real product surface (`#[derive(VerifiedAccounts)]`) so `cargo verified-anchor check` discharges their M4-subset contracts. A litesvm suite `tests/runtime_exploits.rs` runs the before/after for each scenario. Part 0 adds a real-Anchor-compatible `discriminator = "Name"` codegen (computes `sha256("account:" + Name)[..8]` in the macro via the `sha2` crate) that closes the gap scenario 2 requires.

**Tech Stack:** Lean 4.30 / Lake (unchanged); Rust 1.93.1, `syn`/`quote`, **`sha2 = "0.10"`** (new, for the macro to compute Anchor discriminators); SBF toolchain (`solana-cli 4.0.0`); `litesvm 0.6` + vendored OpenSSL.

---

## Conventions

- **Lean:** unchanged; just confirm `lake build` stays green + zero-sorry as a regression check at the end. `export PATH="$HOME/.elan/bin:$PATH"`.
- **Rust native:** `cargo` on PATH (1.93.1). The mandatory full gate (HANDOVER) is the truth: rebuild every `.so` + `cargo test --workspace` with SBF tools + elan on PATH.
- **SBF build recipe (load-bearing, also covers verified-anchor-exploits):**
  ```bash
  export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
  cd rust/<program-crate> && cargo-build-sbf --no-rustup-override
  # -> rust/target/deploy/<program>.so   (workspace-shared)
  ```
  The platform-tools rustc must be first on PATH; `--no-rustup-override` avoids the rustup 1.26 toolchain-name bug.
- **Anchor discriminator formula** (the one the macro and tests both use): `sha256(b"account:" ++ <Name>)[..8]`. Real Anchor accounts carry this prefix; matching it means verified-anchor can validate real Anchor-created accounts (interop).
- **Inventory stays out of BPF** (M5 sanity-fix invariant): we add `emit_specs!()` to the new exploits crate; the `target_os = "solana"` gates added in `8c1246e` keep inventory out of its `.so` automatically. No new gating needed here.
- Commit after each task; `.gitignore` already covers `target/`, `lean/.lake/`.

---

## File structure

| File | Responsibility |
|------|----------------|
| `rust/verified-anchor-macros/Cargo.toml` | (MODIFY) + `sha2 = "0.10"` (normal dep — the macro uses it at downstream compile time) |
| `rust/verified-anchor-macros/src/lib.rs` | (MODIFY) parse `discriminator = "Name"`; compute `sha256("account:"+Name)[..8]` in `Constraint::Discriminator([u8;8])`; emit a runtime `data[0..8]` check; emit `Constraint.discriminator (ByteArray.mk #[..])` in `lean_spec` |
| `rust/verified-anchor/src/lib.rs` | (MODIFY) `VAError` += `WrongDiscriminator { field }` + Display |
| `rust/verified-anchor/Cargo.toml` | (MODIFY) + `sha2 = "0.10"` to `[dev-dependencies]` (tests compute the expected discriminator bytes independently) |
| `rust/verified-anchor/tests/behavior.rs` | (MODIFY) accept correct prefix; reject wrong prefix |
| `rust/verified-anchor/tests/lean_spec.rs` | (MODIFY) assert emitted `Constraint.discriminator (ByteArray.mk #[..])` matches an independent sha256 of `"account:Vault"` |
| `rust/verified-anchor/tests/runtime_exploits.rs` | (NEW) one `#[test]` per scenario: naive(attacker)→Ok+badeffect; verified(attacker)→Err; verified(legit)→Ok |
| `rust/verified-anchor-exploits/Cargo.toml` | (NEW) cdylib+lib; solana-program; verified-anchor |
| `rust/verified-anchor-exploits/src/lib.rs` | (NEW) the BPF program: 4 scenarios × (naive + verified) instruction arms; verified structs; `declare_id!`; `emit_specs!()` |
| `rust/Cargo.toml` | (MODIFY) add the new member |
| `docs/exploit-case-studies.md` | (NEW) M6 report: per scenario — incident, root cause, verified-anchor constraint that defends it, before/after evidence, honest boundary |

---

# PART 0 — close the discriminator gap (prod-ready macro feature)

## Task D1: macro emits a `discriminator` runtime check + lean_spec + tests

**Files:** Modify `rust/verified-anchor-macros/Cargo.toml`, `rust/verified-anchor-macros/src/lib.rs`, `rust/verified-anchor/src/lib.rs`, `rust/verified-anchor/Cargo.toml`, `rust/verified-anchor/tests/behavior.rs`, `rust/verified-anchor/tests/lean_spec.rs`.

The discriminator check is a real product feature: `#[account(discriminator = "Name")]` → at macro time, compute `sha256("account:" + Name)[..8]` (matching Anchor's wire format), emit a runtime `data[0..8]` check, and emit `Constraint.discriminator (ByteArray.mk #[..])` so the M5 `check` covers it.

- [ ] **Step 1: Add `sha2` to the macro crate + the lib's dev-deps**

In `rust/verified-anchor-macros/Cargo.toml`, under `[dependencies]`, add:
```toml
sha2 = "0.10"
```
In `rust/verified-anchor/Cargo.toml`, under `[dev-dependencies]`, add:
```toml
sha2 = "0.10"
```

- [ ] **Step 2: Add `WrongDiscriminator` to `VAError`**

In `rust/verified-anchor/src/lib.rs`, add the variant to the enum + Display arm:
```rust
    WrongDiscriminator { field: &'static str },
```
```rust
            VAError::WrongDiscriminator { field } => write!(f, "account `{field}` has the wrong 8-byte discriminator"),
```

- [ ] **Step 3: Write the failing behavior tests**

In `rust/verified-anchor/tests/behavior.rs`, append:
```rust
use sha2::{Digest, Sha256};

fn disc(name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(b"account:");
    h.update(name.as_bytes());
    let out = h.finalize();
    let mut d = [0u8; 8];
    d.copy_from_slice(&out[..8]);
    d
}

#[derive(VerifiedAccounts)]
struct DiscOnly {
    #[account(discriminator = "Vault")]
    vault: u8,
}

#[test]
fn discriminator_accepts_matching_prefix() {
    let mut v = Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1,
                       data: disc("Vault").to_vec(), is_signer: false, is_writable: false };
    let accts = [v.info()];
    assert_eq!(DiscOnly::validate(&accts, &[], &any_pid()), Ok(()));
}

#[test]
fn discriminator_rejects_wrong_prefix() {
    let mut v = Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1,
                       data: vec![0u8; 8], is_signer: false, is_writable: false };  // wrong disc (all zeros)
    let accts = [v.info()];
    assert_eq!(DiscOnly::validate(&accts, &[], &any_pid()),
               Err(VAError::WrongDiscriminator { field: "vault" }));
}

#[test]
fn discriminator_rejects_short_data() {
    let mut v = Acct { key: Pubkey::new_unique(), owner: Pubkey::new_unique(), lamports: 1,
                       data: vec![0u8; 4], is_signer: false, is_writable: false };  // too short
    let accts = [v.info()];
    assert_eq!(DiscOnly::validate(&accts, &[], &any_pid()),
               Err(VAError::WrongDiscriminator { field: "vault" }));
}
```
Run `cd rust && cargo test -p verified-anchor --test behavior 2>&1 | tail` — expect FAIL (`unknown constraint discriminator`).

- [ ] **Step 4: Add `Constraint::Discriminator` parsing + the sha2 computation**

In `rust/verified-anchor-macros/src/lib.rs`, at the top add:
```rust
use sha2::{Digest, Sha256};
```
Add the variant to `enum Constraint`:
```rust
    Discriminator([u8; 8]),
```
In `impl Parse for Constraint`, add an arm (alongside `"has_one"` etc.):
```rust
            "discriminator" => {
                input.parse::<Token![=]>()?;
                let lit: syn::LitStr = input.parse()?;
                let mut h = Sha256::new();
                h.update(b"account:");
                h.update(lit.value().as_bytes());
                let out = h.finalize();
                let mut d = [0u8; 8];
                d.copy_from_slice(&out[..8]);
                Ok(Constraint::Discriminator(d))
            }
```
Update the catch-all error's "supported" list to include `discriminator`:
```rust
                format!("{hint}; verified-anchor supports: signer, mut, owner, has_one, init, payer, space, close, seeds, bump, discriminator. See docs/migrating-from-anchor.md"),
```

- [ ] **Step 5: Emit the runtime check in `validate_body` + skip the marker elsewhere**

In `validate_body`'s per-constraint `match`, add (alongside the other arms):
```rust
                Constraint::Discriminator(disc) => {
                    let fname = name;
                    let bs: Vec<u8> = disc.to_vec();
                    quote! {
                        {
                            let data = accounts[#i].try_borrow_data()
                                .map_err(|_| ::verified_anchor::VAError::WrongDiscriminator { field: #fname })?;
                            const __DISC: [u8; 8] = [#(#bs),*];
                            if data.len() < 8 || data[0..8] != __DISC {
                                return Err(::verified_anchor::VAError::WrongDiscriminator { field: #fname });
                            }
                        }
                    }
                },
```
Run `cd rust && cargo test -p verified-anchor --test behavior 2>&1 | tail` — expect the 3 new discriminator tests PASS, all previous behavior tests still PASS.

- [ ] **Step 6: Emit `Constraint.discriminator` in `lean_spec`**

In `lean_constraint`, add an arm:
```rust
        Constraint::Discriminator(d) => {
            let bytes: Vec<String> = d.iter().map(|x| x.to_string()).collect();
            format!("Constraint.discriminator (ByteArray.mk #[{}])", bytes.join(", "))
        }
```

- [ ] **Step 7: Add the lean_spec test verifying real-Anchor bytes**

In `rust/verified-anchor/tests/lean_spec.rs`, append (note: shares the `sha2` import with behavior.rs is unnecessary — we re-import here per the test file):
```rust
use sha2::{Digest, Sha256};

fn disc(name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(b"account:");
    h.update(name.as_bytes());
    let out = h.finalize();
    let mut d = [0u8; 8];
    d.copy_from_slice(&out[..8]);
    d
}

#[derive(VerifiedAccounts)]
struct DiscSpec {
    #[account(discriminator = "Vault")]
    vault: u8,
}

#[test]
fn lean_spec_discriminator_bytes_match_anchor() {
    let d = disc("Vault");
    let expected_constraint = format!(
        "Constraint.discriminator (ByteArray.mk #[{}, {}, {}, {}, {}, {}, {}, {}])",
        d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
    );
    let s = DiscSpec::lean_spec();
    assert!(s.contains(&expected_constraint), "spec missing real-Anchor discriminator bytes:\n{s}");
}
```
Run `cd rust && cargo test -p verified-anchor --test behavior --test lean_spec 2>&1 | grep "test result"` — expect all pass (behavior 12+3 prior + 3 new disc = 18-ish; lean_spec 3 prior + 1 new = 4).

- [ ] **Step 8: Build + commit**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust
cargo build -p verified-anchor-macros 2>&1 | tail -3
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-macros/ rust/verified-anchor/src/lib.rs rust/verified-anchor/Cargo.toml rust/verified-anchor/tests/behavior.rs rust/verified-anchor/tests/lean_spec.rs
git commit -m "feat(macros): discriminator codegen (closes Lean<->Rust gap; real Anchor bytes via sha256)"
```

---

# PART 1 — exploits crate skeleton

## Task E0: `verified-anchor-exploits` crate skeleton

**Files:** Create `rust/verified-anchor-exploits/Cargo.toml`, `rust/verified-anchor-exploits/src/lib.rs`; modify `rust/Cargo.toml`.

- [ ] **Step 1: Add to workspace + manifest**

In `rust/Cargo.toml`, add `"verified-anchor-exploits"` to `members`:
```toml
members = ["verified-anchor-macros", "verified-anchor", "verified-anchor-program", "cargo-verified-anchor", "verified-anchor-example", "verified-anchor-exploits"]
```
Create `rust/verified-anchor-exploits/Cargo.toml`:
```toml
[package]
name = "verified-anchor-exploits"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
solana-program = "2"
verified-anchor = { path = "../verified-anchor" }
```

- [ ] **Step 2: Minimal program skeleton + `declare_id!` + `emit_specs!()`**

Create `rust/verified-anchor-exploits/src/lib.rs`:
```rust
//! Empirical exploit-suite program (M6). Each scenario ships a `naive_<s>` and a
//! `verified_<s>` instruction; the litesvm suite asserts a real before/after.
use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult,
    program_error::ProgramError, pubkey::Pubkey,
};
use verified_anchor::{Validate, VerifiedAccounts};

solana_program::declare_id!("VAExp11111111111111111111111111111111111111");

/// Fixed expected "AMM" owner pubkey for scenario 3 (Crema/owner). Any constant works;
/// the test sets the legit price account's owner to this value.
pub const AMM_PROG: Pubkey = Pubkey::new_from_array([
    0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A,
    0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A,
    0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A,
    0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A,
]);

entrypoint!(process);
pub fn process(_program_id: &Pubkey, _accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    match data.first() {
        // Scenarios are wired in Tasks E1..E4.
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

verified_anchor::emit_specs!();
```

- [ ] **Step 3: Native build (no scenarios yet, but the workspace must compile)**

Run: `cd /home/parth/Desktop/PARTH/Verification/rust && cargo build -p verified-anchor-exploits 2>&1 | tail -5` — expect success (`Pubkey::new_from_array` etc. compile).

- [ ] **Step 4: SBF build (must succeed even with empty dispatch — confirms inventory stays out of BPF after M5 fix)**

Run:
```bash
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-exploits && cargo-build-sbf --no-rustup-override 2>&1 | tail -3
ls -la /home/parth/Desktop/PARTH/Verification/rust/target/deploy/verified_anchor_exploits.so
```
Expected: `.so` produced; NO `PT_DYNAMIC dynamic table is invalid` warning (proves the M5 `target_os = "solana"` gates work for the new crate too).

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/Cargo.toml rust/verified-anchor-exploits/
git commit -m "feat(exploits): verified-anchor-exploits BPF skeleton (declare_id, emit_specs!)"
```

---

# PART 2 — the four scenarios

> Each scenario follows the same shape: account-layout decisions, `naive_<s>`, `verified_<s>` (using the real product surface), one instruction-tag pair in `process()`, and one `#[test]` in `runtime_exploits.rs` asserting all three required outcomes. The litesvm test file is created in **Task E1** and *extended* by E2/E3/E4 — keep the helpers shared.

## Task E1: Scenario 1 — Cashio infinite-mint (`has_one` + `owner` + `discriminator`)

**Files:** Modify `rust/verified-anchor-exploits/src/lib.rs`; create `rust/verified-anchor/tests/runtime_exploits.rs`.

**Account layout (matches what the verified struct expects):**
- `collateral.data` = `[disc(8)][bank_pubkey(32)][amount_u64_le(8)]` (48 bytes). `disc` = `sha256("account:Collateral")[..8]`. Owner must be `crate::ID` for verified to accept.
- `bank.data` = (any; the verified struct names `bank` but has no constraint on its bytes — `has_one` compares `collateral.data[8..40]` to `bank.key`, not to bank's data).
- `out.data` = `[u64_le(8)]` (8 bytes); writable.

- [ ] **Step 1: Add the verified struct + the two instruction arms**

In `rust/verified-anchor-exploits/src/lib.rs`, BEFORE `verified_anchor::emit_specs!();`, add:
```rust
/// Verified Cashio: collateral must be program-owned, carry the right discriminator,
/// AND link to `bank` via the offset-8 has_one field.
#[derive(VerifiedAccounts)]
pub struct VerifiedCashio {
    #[account(owner = crate::ID, discriminator = "Collateral", has_one = bank)]
    pub collateral: u8,
    pub bank: u8,
    #[account(mut)]
    pub out: u8,
}

fn write_u64_le(dst: &mut [u8], v: u64) {
    dst[..8].copy_from_slice(&v.to_le_bytes());
}
fn read_u64_le(src: &[u8]) -> u64 { u64::from_le_bytes(src[..8].try_into().unwrap()) }

fn naive_cashio(accounts: &[AccountInfo]) -> ProgramResult {
    // attacker beats this: no checks; reads `collateral.amount` and credits `out`.
    let coll_data = accounts[0].try_borrow_data().map_err(|_| ProgramError::InvalidAccountData)?;
    let amount = read_u64_le(&coll_data[40..48]);
    drop(coll_data);
    let mut out_data = accounts[2].try_borrow_mut_data().map_err(|_| ProgramError::InvalidAccountData)?;
    write_u64_le(&mut out_data, amount);
    Ok(())
}
fn verified_cashio(accounts: &[AccountInfo], program_id: &Pubkey) -> ProgramResult {
    VerifiedCashio::validate(accounts, &[], program_id)
        .map_err(|_| ProgramError::InvalidArgument)?;
    // same effect as naive — but unreachable for attacker accounts (validate rejected)
    naive_cashio(accounts)
}
```
And in `process()`'s `match`, replace the `_ => Err(...)` with:
```rust
        Some(0) => naive_cashio(accounts),
        Some(1) => verified_cashio(accounts, _program_id),
        _ => Err(ProgramError::InvalidInstructionData),
```
Also rename `_program_id` → `program_id` in the `process` signature so the arm can pass it.

- [ ] **Step 2: SBF rebuild**
```bash
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-exploits && cargo-build-sbf --no-rustup-override 2>&1 | tail -3
```
Expected: success, no PT_DYNAMIC warning.

- [ ] **Step 3: Create `runtime_exploits.rs` with shared helpers + the Scenario 1 test**

Create `rust/verified-anchor/tests/runtime_exploits.rs`:
```rust
//! M6 empirical suite: per scenario, naive(attacker)->Ok+badeffect; verified(attacker)->Err;
//! verified(legit)->Ok. Loads the verified-anchor-exploits BPF program.
use litesvm::LiteSVM;
use solana_account::Account;
use solana_instruction::{account_meta::AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;
use std::path::PathBuf;
use sha2::{Digest, Sha256};

fn so_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // rust/verified-anchor
    p.pop(); // rust/
    p.push("target/deploy/verified_anchor_exploits.so");
    p
}

/// The same fixed program id the exploits crate's `declare_id!` uses.
fn program_id() -> Pubkey {
    "VAExp11111111111111111111111111111111111111".parse().unwrap()
}

/// AMM_PROG constant — must match the one in verified-anchor-exploits/src/lib.rs.
fn amm_prog() -> Pubkey { Pubkey::new_from_array([0x0A; 32]) }

fn disc(name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(b"account:");
    h.update(name.as_bytes());
    let out = h.finalize();
    let mut d = [0u8; 8];
    d.copy_from_slice(&out[..8]);
    d
}

fn fresh_svm() -> (LiteSVM, Keypair) {
    let mut svm = LiteSVM::new();
    svm.add_program_from_file(program_id(), so_path())
        .expect("load .so (run cargo-build-sbf first)");
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000).unwrap();
    (svm, payer)
}

fn send(svm: &mut LiteSVM, payer: &Keypair, tag: u8, metas: Vec<AccountMeta>) -> Result<(), ()> {
    let ix = Instruction { program_id: program_id(), data: vec![tag], accounts: metas };
    let bh = svm.latest_blockhash();
    let tx = Transaction::new(&[payer], Message::new(&[ix], Some(&payer.pubkey())), bh);
    svm.send_transaction(tx).map(|_| ()).map_err(|_| ())
}

fn build_collateral(disc8: [u8;8], bank: Pubkey, amount: u64) -> Vec<u8> {
    let mut d = vec![0u8; 48];
    d[0..8].copy_from_slice(&disc8);
    d[8..40].copy_from_slice(&bank.to_bytes());
    d[40..48].copy_from_slice(&amount.to_le_bytes());
    d
}

#[test]
fn scenario_1_cashio_has_one_before_after() {
    let (mut svm, payer) = fresh_svm();
    let bank = Pubkey::new_unique();
    let out = Pubkey::new_unique();
    let attacker_coll = Pubkey::new_unique();
    let legit_coll = Pubkey::new_unique();
    let attacker_owner = Pubkey::new_unique();

    // bank: any account (verified struct has no constraints on `bank`'s bytes).
    svm.set_account(bank, Account { lamports: 1, data: vec![], owner: program_id(), executable: false, rent_epoch: 0 }).unwrap();
    // out: program-owned, writable, 8 bytes.
    svm.set_account(out, Account { lamports: 1, data: vec![0u8; 8], owner: program_id(), executable: false, rent_epoch: 0 }).unwrap();
    // Fake collateral: wrong owner, wrong disc, wrong bank, HUGE amount.
    svm.set_account(attacker_coll, Account {
        lamports: 1, data: build_collateral([0xFF; 8], Pubkey::new_unique(), u64::MAX),
        owner: attacker_owner, executable: false, rent_epoch: 0,
    }).unwrap();
    // Legit collateral: program-owned, real disc, right bank, modest amount.
    svm.set_account(legit_coll, Account {
        lamports: 1, data: build_collateral(disc("Collateral"), bank, 42),
        owner: program_id(), executable: false, rent_epoch: 0,
    }).unwrap();

    let metas_attacker = vec![
        AccountMeta::new_readonly(attacker_coll, false),
        AccountMeta::new_readonly(bank, false),
        AccountMeta::new(out, false),
    ];
    let metas_legit = vec![
        AccountMeta::new_readonly(legit_coll, false),
        AccountMeta::new_readonly(bank, false),
        AccountMeta::new(out, false),
    ];

    // 1) naive(attacker) -> Ok + observable bad effect (out.minted == u64::MAX)
    assert!(send(&mut svm, &payer, 0, metas_attacker.clone()).is_ok(),
            "naive must accept the attacker accounts (the bug)");
    let out_data = svm.get_account(&out).unwrap().data;
    assert_eq!(u64::from_le_bytes(out_data[..8].try_into().unwrap()), u64::MAX,
               "naive must credit the attacker's HUGE amount");

    // Reset out for the next call's positive control.
    svm.set_account(out, Account { lamports: 1, data: vec![0u8; 8], owner: program_id(), executable: false, rent_epoch: 0 }).unwrap();

    // 2) verified(attacker) -> Err
    assert!(send(&mut svm, &payer, 1, metas_attacker).is_err(),
            "verified must reject the attacker accounts");

    // 3) verified(legit) -> Ok + correct effect (out.minted == 42)
    assert!(send(&mut svm, &payer, 1, metas_legit).is_ok(),
            "verified must accept legit collateral");
    let out_data = svm.get_account(&out).unwrap().data;
    assert_eq!(u64::from_le_bytes(out_data[..8].try_into().unwrap()), 42,
               "verified must process the legit amount");
}
```

- [ ] **Step 4: Run scenario 1**

```bash
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-exploits && cargo-build-sbf --no-rustup-override 2>&1 | tail -2
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test runtime_exploits scenario_1 2>&1 | tail -15
```
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-exploits/src/lib.rs rust/verified-anchor/tests/runtime_exploits.rs
git commit -m "feat(exploits): scenario 1 (Cashio infinite-mint: has_one + owner + discriminator)"
```

---

## Task E2: Scenario 2 — Account type-confusion (`discriminator`)

**Files:** Modify `rust/verified-anchor-exploits/src/lib.rs`; extend `rust/verified-anchor/tests/runtime_exploits.rs`.

**Account layout:** both `Vault` and `Config` are program-owned with `[disc(8)][authority/field_pubkey(32)]` = 40 bytes. Discriminators: `sha256("account:Vault")[..8]` vs `sha256("account:Config")[..8]`.

- [ ] **Step 1: Add the verified struct + the two arms**

In `rust/verified-anchor-exploits/src/lib.rs`, before `emit_specs!()`, add:
```rust
/// Verified type-disambiguation: discriminator is the distinguishing check (both account
/// types are program-owned, so owner can't tell them apart — only the 8-byte tag can).
#[derive(VerifiedAccounts)]
pub struct VerifiedConfusion {
    #[account(discriminator = "Vault")]
    pub vault: u8,
    #[account(signer)]
    pub authority: u8,
    #[account(mut)]
    pub out: u8,
}

fn naive_confusion(accounts: &[AccountInfo]) -> ProgramResult {
    // attacker beats this: reads bytes [8..40] of accounts[0] as an "authority" pubkey
    // without checking the 8-byte discriminator. If it matches the signer, grants access.
    let data = accounts[0].try_borrow_data().map_err(|_| ProgramError::InvalidAccountData)?;
    if data.len() >= 40 && &data[8..40] == accounts[1].key.as_ref() && accounts[1].is_signer {
        drop(data);
        let mut out_data = accounts[2].try_borrow_mut_data().map_err(|_| ProgramError::InvalidAccountData)?;
        out_data[0] = 1; // "authorized" flag
    }
    Ok(())
}
fn verified_confusion(accounts: &[AccountInfo], program_id: &Pubkey) -> ProgramResult {
    VerifiedConfusion::validate(accounts, &[], program_id)
        .map_err(|_| ProgramError::InvalidArgument)?;
    naive_confusion(accounts)
}
```
Wire into `process()`:
```rust
        Some(2) => naive_confusion(accounts),
        Some(3) => verified_confusion(accounts, program_id),
```

- [ ] **Step 2: SBF rebuild + extend the test file with scenario 2**

```bash
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-exploits && cargo-build-sbf --no-rustup-override 2>&1 | tail -2
```
Then in `rust/verified-anchor/tests/runtime_exploits.rs`, append:
```rust
fn build_typed_acct(disc8: [u8;8], field: Pubkey) -> Vec<u8> {
    let mut d = vec![0u8; 40];
    d[0..8].copy_from_slice(&disc8);
    d[8..40].copy_from_slice(&field.to_bytes());
    d
}

#[test]
fn scenario_2_type_confusion_discriminator_before_after() {
    let (mut svm, payer) = fresh_svm();
    let out = Pubkey::new_unique();

    // Attacker key signs; out is program-owned writable.
    svm.set_account(out, Account { lamports: 1, data: vec![0u8; 8], owner: program_id(), executable: false, rent_epoch: 0 }).unwrap();

    // Attacker accounts:
    //   "vault" is actually a Config account whose offset-8 bytes equal the attacker's pubkey
    //   so the naive auth-by-bytes check passes; signer = attacker.
    let attacker_vault = Pubkey::new_unique();
    svm.set_account(attacker_vault, Account {
        lamports: 1, data: build_typed_acct(disc("Config"), payer.pubkey()),
        owner: program_id(), executable: false, rent_epoch: 0,
    }).unwrap();

    let metas_attacker = vec![
        AccountMeta::new_readonly(attacker_vault, false),
        AccountMeta::new_readonly(payer.pubkey(), true),  // signer
        AccountMeta::new(out, false),
    ];

    // 1) naive(attacker) -> out[0] = 1 (authorized)
    assert!(send(&mut svm, &payer, 2, metas_attacker.clone()).is_ok(),
            "naive must accept the type-confused account");
    assert_eq!(svm.get_account(&out).unwrap().data[0], 1,
               "naive must set the authorized flag");

    svm.set_account(out, Account { lamports: 1, data: vec![0u8; 8], owner: program_id(), executable: false, rent_epoch: 0 }).unwrap();

    // 2) verified(attacker) -> Err
    assert!(send(&mut svm, &payer, 3, metas_attacker).is_err(),
            "verified must reject the wrong-disc account");

    // 3) verified(legit) -> Ok + out[0] = 1
    let legit_vault = Pubkey::new_unique();
    svm.set_account(legit_vault, Account {
        lamports: 1, data: build_typed_acct(disc("Vault"), payer.pubkey()),
        owner: program_id(), executable: false, rent_epoch: 0,
    }).unwrap();
    let metas_legit = vec![
        AccountMeta::new_readonly(legit_vault, false),
        AccountMeta::new_readonly(payer.pubkey(), true),
        AccountMeta::new(out, false),
    ];
    assert!(send(&mut svm, &payer, 3, metas_legit).is_ok(),
            "verified must accept legit Vault");
    assert_eq!(svm.get_account(&out).unwrap().data[0], 1);
}
```

- [ ] **Step 3: Run**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test runtime_exploits scenario_2 2>&1 | tail -15
```
Expected: PASS.

- [ ] **Step 4: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-exploits/src/lib.rs rust/verified-anchor/tests/runtime_exploits.rs
git commit -m "feat(exploits): scenario 2 (account type-confusion, discriminator-defended)"
```

---

## Task E3: Scenario 3 — Crema fake-account (`owner`)

**Files:** Modify `rust/verified-anchor-exploits/src/lib.rs`; extend `rust/verified-anchor/tests/runtime_exploits.rs`.

**Layout:** `price.data` = `[u64_le(8)]` (8 bytes). Owner: legit must be `AMM_PROG`.

- [ ] **Step 1: Add the verified struct + the two arms**

Insert into `verified-anchor-exploits/src/lib.rs` before `emit_specs!()`:
```rust
#[derive(VerifiedAccounts)]
pub struct VerifiedCrema {
    #[account(owner = AMM_PROG)]
    pub price: u8,
    #[account(mut)]
    pub out: u8,
}

fn naive_crema(accounts: &[AccountInfo]) -> ProgramResult {
    let pd = accounts[0].try_borrow_data().map_err(|_| ProgramError::InvalidAccountData)?;
    let p = read_u64_le(&pd);
    drop(pd);
    let mut out_data = accounts[1].try_borrow_mut_data().map_err(|_| ProgramError::InvalidAccountData)?;
    write_u64_le(&mut out_data, p);
    Ok(())
}
fn verified_crema(accounts: &[AccountInfo], program_id: &Pubkey) -> ProgramResult {
    VerifiedCrema::validate(accounts, &[], program_id)
        .map_err(|_| ProgramError::InvalidArgument)?;
    naive_crema(accounts)
}
```
And add to `process()`:
```rust
        Some(4) => naive_crema(accounts),
        Some(5) => verified_crema(accounts, program_id),
```

- [ ] **Step 2: SBF rebuild + extend the test file with scenario 3**

```bash
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-exploits && cargo-build-sbf --no-rustup-override 2>&1 | tail -2
```
Append to `runtime_exploits.rs`:
```rust
#[test]
fn scenario_3_crema_owner_before_after() {
    let (mut svm, payer) = fresh_svm();
    let out = Pubkey::new_unique();
    svm.set_account(out, Account { lamports: 1, data: vec![0u8; 8], owner: program_id(), executable: false, rent_epoch: 0 }).unwrap();

    // Attacker price: attacker-owned, fake price u64::MAX.
    let attacker_price = Pubkey::new_unique();
    let attacker_owner = Pubkey::new_unique();
    svm.set_account(attacker_price, Account {
        lamports: 1, data: u64::MAX.to_le_bytes().to_vec(),
        owner: attacker_owner, executable: false, rent_epoch: 0,
    }).unwrap();
    // Legit price: AMM_PROG-owned, modest price.
    let legit_price = Pubkey::new_unique();
    svm.set_account(legit_price, Account {
        lamports: 1, data: 1_000_u64.to_le_bytes().to_vec(),
        owner: amm_prog(), executable: false, rent_epoch: 0,
    }).unwrap();

    let metas_a = vec![AccountMeta::new_readonly(attacker_price, false), AccountMeta::new(out, false)];
    let metas_l = vec![AccountMeta::new_readonly(legit_price, false), AccountMeta::new(out, false)];

    assert!(send(&mut svm, &payer, 4, metas_a.clone()).is_ok());
    assert_eq!(u64::from_le_bytes(svm.get_account(&out).unwrap().data[..8].try_into().unwrap()), u64::MAX);

    svm.set_account(out, Account { lamports: 1, data: vec![0u8; 8], owner: program_id(), executable: false, rent_epoch: 0 }).unwrap();
    assert!(send(&mut svm, &payer, 5, metas_a).is_err());

    assert!(send(&mut svm, &payer, 5, metas_l).is_ok());
    assert_eq!(u64::from_le_bytes(svm.get_account(&out).unwrap().data[..8].try_into().unwrap()), 1_000);
}
```

- [ ] **Step 3: Run + commit**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test runtime_exploits scenario_3 2>&1 | tail -15
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-exploits/src/lib.rs rust/verified-anchor/tests/runtime_exploits.rs
git commit -m "feat(exploits): scenario 3 (Crema fake-account, owner-defended)"
```

---

## Task E4: Scenario 4 — PDA seeds misuse (`seeds`/`bump`)

**Files:** Modify `rust/verified-anchor-exploits/src/lib.rs`; extend `rust/verified-anchor/tests/runtime_exploits.rs`.

**Layout:** `vault` carries no data (key is what's checked). `out.data` = 32 bytes (we write the credited-to pubkey there).

- [ ] **Step 1: Add the verified struct + the two arms**

Insert into `verified-anchor-exploits/src/lib.rs` before `emit_specs!()`:
```rust
#[derive(VerifiedAccounts)]
pub struct VerifiedSeeds {
    #[account(signer)]
    pub user: u8,
    #[account(seeds = [b"vault", user.key()], bump)]
    pub vault: u8,
    #[account(mut)]
    pub out: u8,
}

fn naive_seeds(accounts: &[AccountInfo]) -> ProgramResult {
    // attacker beats this: trusts whatever pubkey was passed as "vault".
    let mut out_data = accounts[2].try_borrow_mut_data().map_err(|_| ProgramError::InvalidAccountData)?;
    out_data[..32].copy_from_slice(&accounts[1].key.to_bytes());
    Ok(())
}
fn verified_seeds(accounts: &[AccountInfo], program_id: &Pubkey) -> ProgramResult {
    VerifiedSeeds::validate(accounts, &[], program_id)
        .map_err(|_| ProgramError::InvalidArgument)?;
    naive_seeds(accounts)
}
```
And add to `process()`:
```rust
        Some(6) => naive_seeds(accounts),
        Some(7) => verified_seeds(accounts, program_id),
```

- [ ] **Step 2: SBF rebuild + extend the test file with scenario 4**

```bash
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-exploits && cargo-build-sbf --no-rustup-override 2>&1 | tail -2
```
Append to `runtime_exploits.rs`:
```rust
#[test]
fn scenario_4_seeds_pda_before_after() {
    let (mut svm, payer) = fresh_svm();
    let user = Keypair::new();
    svm.airdrop(&user.pubkey(), 1_000_000).unwrap();
    let out = Pubkey::new_unique();
    svm.set_account(out, Account { lamports: 1, data: vec![0u8; 32], owner: program_id(), executable: false, rent_epoch: 0 }).unwrap();

    let (pda, _bump) = Pubkey::find_program_address(&[b"vault", &user.pubkey().to_bytes()], &program_id());
    let attacker_vault = Pubkey::new_unique();   // NOT the derived PDA

    // We must sign with both payer (fee payer) and user (signer in metas).
    let send_with_user = |svm: &mut LiteSVM, tag: u8, vault: Pubkey| -> Result<(), ()> {
        let ix = Instruction {
            program_id: program_id(), data: vec![tag],
            accounts: vec![
                AccountMeta::new_readonly(user.pubkey(), true),  // signer
                AccountMeta::new_readonly(vault, false),
                AccountMeta::new(out, false),
            ],
        };
        let bh = svm.latest_blockhash();
        let tx = Transaction::new(&[&payer, &user], Message::new(&[ix], Some(&payer.pubkey())), bh);
        svm.send_transaction(tx).map(|_| ()).map_err(|_| ())
    };

    // 1) naive(attacker) credits the attacker's account.
    assert!(send_with_user(&mut svm, 6, attacker_vault).is_ok());
    assert_eq!(&svm.get_account(&out).unwrap().data[..32], &attacker_vault.to_bytes());

    svm.set_account(out, Account { lamports: 1, data: vec![0u8; 32], owner: program_id(), executable: false, rent_epoch: 0 }).unwrap();
    // 2) verified(attacker) -> Err
    assert!(send_with_user(&mut svm, 7, attacker_vault).is_err());

    // 3) verified(legit PDA) -> Ok + credits the PDA.
    assert!(send_with_user(&mut svm, 7, pda).is_ok());
    assert_eq!(&svm.get_account(&out).unwrap().data[..32], &pda.to_bytes());
}
```

- [ ] **Step 3: Run + commit**
```bash
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test -p verified-anchor --test runtime_exploits scenario_4 2>&1 | tail -15
cd /home/parth/Desktop/PARTH/Verification
git add rust/verified-anchor-exploits/src/lib.rs rust/verified-anchor/tests/runtime_exploits.rs
git commit -m "feat(exploits): scenario 4 (PDA seeds misuse, seeds-defended)"
```

---

# PART 3 — report + final gates

## Task R1: Write `docs/exploit-case-studies.md`

**Files:** Create `docs/exploit-case-studies.md`.

- [ ] **Step 1: Write the report**

Create `docs/exploit-case-studies.md`:
```markdown
# verified-anchor — Empirical Exploit Case Studies (M6)

Four real macro-level account-validation bug classes, each reproduced as a litesvm
**before/after**: a naive instruction that the attacker beats (with an observable bad
on-chain effect), and a `#[derive(VerifiedAccounts)]` instruction that rejects the same
attacker accounts. Run them with the M6 mandatory gate (see HANDOVER).

The verified versions also pass `cargo verified-anchor check -p verified-anchor-exploits`,
i.e. each carries the **machine-checked M1–M5 guarantee** that the generated `validate`
implements the M1 contract (`genValidate_sound` at `M4Subset`, generic `lifecycle_sound`).

## How we reproduce

For each scenario we model the **minimized root-cause mechanism**, not the full protocol.
Effects are deliberately small and observable (a credit, a flag, a copied pubkey) so the
litesvm test can compare "did the bug land?" vs "was the attack rejected?". The defending
constraint is the same one a real fix uses; the production effect (e.g., Cashio's $52M mint)
is identical in mechanism, only larger in stakes.

We do NOT issue real SPL-token CPIs in the test programs — adding a token-program dependency
buys no additional verification rigor (the validation gate is identical regardless of what
follows it) and would slow every test. See `verified-anchor-exploits/src/lib.rs` for the
exact effect each naive arm performs.

## Scenarios

### 1. Cashio infinite-mint — `has_one` + `owner` + `discriminator`
**Incident.** Cashio Mar 22 2022, ~$52M. The mint path failed to validate the
collateral↔bank↔mint account chain; an attacker minted unlimited CASH against fake
collateral. Public post-mortems are widely cited (search: "Cashio infinite mint post-mortem").

**Root cause.** Missing validation of the typed account `Collateral` (owner, discriminator,
and the link `collateral.bank == bank`).

**Defending constraint(s).** `#[account(owner = crate::ID, discriminator = "Collateral", has_one = bank)]` on the collateral field.

**Evidence (`scenario_1_cashio_has_one_before_after`):**
- `naive_cashio(attacker_coll, bank, out)` → `Ok`; `out.minted == u64::MAX` (the bug lands).
- `verified_cashio(attacker_coll, bank, out)` → on-chain `Err` (`WrongOwner` / `WrongDiscriminator` / `WrongHasOne`).
- `verified_cashio(legit_coll, bank, out)` → `Ok`; `out.minted == 42`.

### 2. Account type-confusion — `discriminator`
**Incident.** The canonical Anchor `Account<T>` bug class: deserializing a look-alike
account of the wrong type (same program). The 8-byte discriminator was specifically
designed to stop this.

**Root cause.** Missing 8-byte discriminator check; bytes at offset 8 from the wrong type
look like the right type (e.g., a Config's stored pubkey == the attacker's pubkey).

**Defending constraint.** `#[account(discriminator = "Vault")]` (computes `sha256("account:Vault")[..8]` at macro time — same bytes as real Anchor).

**Evidence (`scenario_2_type_confusion_discriminator_before_after`):**
- `naive_confusion(attacker_vault_actually_config, attacker_signer, out)` → `Ok`; `out.authorized = 1`.
- `verified_confusion(...)` → `Err` (discriminator mismatch).
- `verified_confusion(legit_vault, ...)` → `Ok`; `out.authorized = 1`.

(Note: this scenario is what motivated **Part 0** of M6 — verified-anchor's Rust runtime did
not previously enforce the discriminator that Lean proves; M6 closes that gap as a real
product feature, not a demo-specific hack.)

### 3. Crema fake-account — `owner`
**Incident.** Crema Finance, Jul 2022. An attacker passed a fake "tick"/price account because
its owner program was never checked. Public post-mortems cover the details.

**Root cause.** Missing owner check.

**Defending constraint.** `#[account(owner = AMM_PROG)]`.

**Evidence (`scenario_3_crema_owner_before_after`):**
- `naive_crema(attacker_price, out)` → `Ok`; `out.price == u64::MAX`.
- `verified_crema(...)` → `Err` (`WrongOwner`).
- `verified_crema(legit_price, out)` → `Ok`; `out.price == 1_000`.

### 4. PDA seeds misuse — `seeds`/`bump`
**Class.** Programs accept a "vault"/PDA pubkey passed in instruction data without
re-deriving it from seeds (or only re-derive against a non-canonical bump).

**Root cause.** Missing PDA re-derivation.

**Defending constraint.** `#[account(seeds = [b"vault", user.key()], bump)]` (canonical-only
in verified-anchor — stricter than stock Anchor's stored-bump form; see the bridge doc).

**Evidence (`scenario_4_seeds_pda_before_after`):**
- `naive_seeds(user, attacker_vault, out)` → `Ok`; `out` records the attacker's pubkey.
- `verified_seeds(...)` → `Err` (`WrongPda`).
- `verified_seeds(user, real_pda, out)` → `Ok`; `out` records the PDA.

## Tie-in to the proven contract

For all four verified structs, `cargo verified-anchor check -p verified-anchor-exploits`
emits per-struct `example : M4Subset <spec> := by decide` (and for any lifecycle field, an
`example : StructLifecycleWF <spec> := by decide`), `lake env lean` discharges them with
axioms `[propext, Quot.sound]` only. So the safe versions don't just empirically reject
attacker accounts — they carry the M1–M5 machine-checked guarantee that the generated
`validate` implements the M1 contract.

## Honest boundary

- We reproduce the **mechanism**, not the full protocol; effects are minimized and observable.
- The spec-carrier API (`u8` fields + explicit `#[account(...)]` constraints) is not yet
  real `Account<'info, T>` typing. Full anchor-lang API fidelity is M7.
- The `has_one` codegen reads offset 8 (the 32 bytes after the discriminator), assuming the
  Anchor layout. A different layout breaks the check (documented Lean follow-up: tighten the
  AST and the codegen).
- We model the on-chain *effect* of these instructions in litesvm; we do not prove the
  rustc/sBPF codegen translates `validate` faithfully (the project boundary, cf. CompCert).
- We chose four exploits whose root cause is **macro-level account validation**. Bugs whose
  root cause is something else (Mango Markets-style oracle manipulation, signer-replay flaws,
  CPI-trust bugs) are out of scope for M6.
```

- [ ] **Step 2: Commit**
```bash
cd /home/parth/Desktop/PARTH/Verification
git add docs/exploit-case-studies.md
git commit -m "docs(m6): exploit case-studies report (4 scenarios + tie-in + honest boundary)"
```

---

## Task F1: Final mandatory full gate

**Files:** None modified (gate run only).

- [ ] **Step 1: Run the mandatory full gate**

```bash
export PATH="$HOME/.elan/bin:$PATH"; cd /home/parth/Desktop/PARTH/Verification/lean && lake build 2>&1 | tail -1
grep -rn "sorry\|admit" VerifiedAnchor/ || echo "PASS lean zero-sorry"
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$HOME/.elan/bin:$PATH"
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-program && cargo-build-sbf --no-rustup-override 2>&1 | grep -iE "PT_DYNAMIC|Finished" | tail -1
cd /home/parth/Desktop/PARTH/Verification/rust/verified-anchor-exploits && cargo-build-sbf --no-rustup-override 2>&1 | grep -iE "PT_DYNAMIC|Finished" | tail -1
cd /home/parth/Desktop/PARTH/Verification/rust && cargo test --workspace 2>&1 | grep -E "Running |test result:|FAILED"
# M5 check on the exploits crate
cd /home/parth/Desktop/PARTH/Verification/rust && cargo run -q -p cargo-verified-anchor -- verified-anchor check -p verified-anchor-exploits --lean-dir ../lean
```
Expected:
- `lake build` → `Build completed successfully (20 jobs).`
- zero-sorry CLEAN
- Both `.so` builds finish with NO `PT_DYNAMIC dynamic table is invalid` warning (the M5 fix invariant holds).
- `cargo test --workspace` — every suite green: behavior (18+ with disc), lean_spec (4+), cli (1), compile_fail (1), runtime_lifecycle (2), runtime_seeds (2), **runtime_exploits (4)**, generator unit tests (3), example emit (1), program no-tests, etc. NO failures anywhere.
- `cargo verified-anchor check -p verified-anchor-exploits` → exit 0 with `✓ VerifiedCashio (validation)`, `✓ VerifiedConfusion (validation)`, `✓ VerifiedCrema (validation)`, `✓ VerifiedSeeds (validation)`, and `All 4 proof obligation(s) discharged.`

(Note: the verified structs in the exploits crate do NOT have init/close fields, so all 4 are validation-kind. If a future variant needs lifecycle, the M5 generator handles both kinds.)

- [ ] **Step 2: Sanity — full grep for unintentional `sorry`/`admit`/`native_decide` anywhere in the project**

Run: `grep -rn "sorry\|admit\|native_decide" /home/parth/Desktop/PARTH/Verification/lean/VerifiedAnchor/ /home/parth/Desktop/PARTH/Verification/rust/ /home/parth/Desktop/PARTH/Verification/docs/ 2>/dev/null | grep -v "/.lake/" | grep -v "/target/" | grep -v "case-studies"` — expect empty (the case-studies doc may contain the word "admit"; the grep excludes it explicitly).

- [ ] **Step 3: There's nothing to commit (the F1 task is the gate run). If anything red, fix and re-gate before declaring done.**

---

## Done-bar verification (after F1)

1. Macro: `#[account(discriminator = "Name")]` produces a runtime `data[0..8]` check using `sha256("account:Name")[..8]`; `VAError::WrongDiscriminator`; behavior tests accept/reject + lean_spec test verifies the bytes match an independent sha256. ✅ (D1)
2. `verified-anchor-exploits` builds natively and to BPF `.so` (no PT_DYNAMIC warning); `emit_specs!()` present. ✅ (E0)
3. `runtime_exploits.rs`: 4 scenarios, each asserting naive(attacker)→Ok+badeffect, verified(attacker)→Err, verified(legit)→Ok. ✅ (E1–E4)
4. `cargo verified-anchor check -p verified-anchor-exploits` exits 0 with 4 ✓ lines. ✅ (F1)
5. `docs/exploit-case-studies.md` covers all 4 scenarios + tie-in + honest boundary. ✅ (R1)
6. Mandatory full gate green: `lake build` + zero-sorry; SBF builds clean; `cargo test --workspace` (incl. runtime suites) all green; M1–M5 no regressions. ✅ (F1)
