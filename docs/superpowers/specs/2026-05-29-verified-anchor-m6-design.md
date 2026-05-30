# Verified Anchor — Milestone 6 Design

*Empirical validation: reproduce four real Solana account-validation exploit classes and show verified-anchor catches them, executed under litesvm.*

Status: **approved design** (2026-05-29). Target: Milestone 6 of `verified_anchor_proposal.md`. Builds on M1–M5 (all on `master`).

---

## 1. Goal and context

M6 is the milestone that converts the technical argument into an empirical, publishable result: take **real historical Solana exploits whose root cause is macro-level account validation**, reproduce the root-cause mechanism, and demonstrate — running under litesvm — that the verified-anchor version rejects the exact attack the naive version falls to.

Scope is **four scenarios**, one per macro-level account-validation bug class, each mapped to the verified-anchor constraint that defends it. This is broader than the proposal's "at least one" — it shows verified-anchor covers the *spectrum* of these bugs, not a single case.

### Decisions locked during brainstorming
1. **Four scenarios** (not one): Cashio/`has_one`, type-confusion/`discriminator`, Crema/`owner`, PDA-misuse/`seeds`.
2. **Demonstration = litesvm before/after** for every scenario: a NAIVE instruction the attacker beats (observable bad on-chain effect) + a VERIFIED instruction that rejects the *same* attacker accounts. No per-scenario Lean theorems (the contracts are already proven in M1–M5; the report cites them).
3. **Minimal-effect modeling** (not faithful SPL-token CPIs): the naive effect is a small observable state change (credit/flag/transfer) that the attack achieves. The verification story is the *account-validation gate*, identical regardless of the downstream effect; this keeps litesvm tests dependency-light.
4. **Dedicated crate** `rust/verified-anchor-exploits/` for the 8 naive/verified instruction arms.
5. **Prod-ready, not demo-ware** (per user): the verified versions use the real product surface (`#[derive(VerifiedAccounts)]` + `cargo verified-anchor check`); and the one real product gap a scenario requires — runtime discriminator enforcement — is **closed generally** (a reusable macro feature), not hacked for the demo.

### The discriminator gap (load-bearing finding, closed in Part 0)
verified-anchor's Rust macro generates runtime checks for `signer`/`mut`/`owner`/`has_one`/`seeds`, but **not** the 8-byte Anchor discriminator — it is modeled and *proven in Lean* (`Constraint.discriminator`, `isM4Constraint`, `genValidate_sound` all cover it) but never enforced by the generated `validate` (`has_one` even reads `data[8..40]`, skipping offset 0). That is a genuine Lean↔Rust transcription defect. M6 closes it (Part 0) so scenario 2 is a real runtime demo *and* the product enforces what it proves.

---

## 2. Repository layout (M6 additions)

```
rust/
├── verified-anchor-macros/src/lib.rs   (MODIFY) add `discriminator = "Name"` parsing + runtime check + lean_spec
├── verified-anchor/
│   ├── src/lib.rs                       (MODIFY) VAError += WrongDiscriminator
│   └── tests/
│       ├── behavior.rs                  (MODIFY) native unit tests for the discriminator check
│       ├── lean_spec.rs                 (MODIFY) assert emitted Constraint.discriminator shape
│       └── runtime_exploits.rs          (NEW) litesvm before/after for all 4 scenarios
├── verified-anchor-exploits/            (NEW) BPF program: naive_/verified_ arms for the 4 scenarios + emit_specs!()
│   ├── Cargo.toml                       cdylib+lib; solana-program; verified-anchor
│   └── src/lib.rs
└── Cargo.toml                           (MODIFY) add the new member

lean/  — NO changes (discriminator already modeled; contracts already proven)

docs/
├── exploit-case-studies.md             (NEW) the M6 report (4 scenarios, postmortems, before/after evidence)
└── superpowers/specs/2026-05-29-verified-anchor-m6-design.md   (this file)
```

---

## 3. Part 0 — close the discriminator gap (prod-ready macro feature)

A reusable feature, not demo-specific.

- **Macro parse:** `#[account(discriminator = "Vault")]` → `Constraint::Discriminator([u8; 8])`, where the 8 bytes are computed at macro-expansion time as **Anchor's real discriminator** `sha256("account:Vault")[0..8]` (add a `sha2` build/normal dep to `verified-anchor-macros`). This matches real Anchor-created accounts (interop) and the Lean model's `accountDiscriminator "Vault"` formula.
- **Runtime check** (in `validate_body`, mirroring `has_one`): borrow `accounts[i]` data; `if data.len() < 8 || data[0..8] != DISC { return Err(VAError::WrongDiscriminator { field }) }`.
- **lean_spec:** emit `Constraint.discriminator (ByteArray.mk #[b0, …, b7])` for the field. (`Constraint.discriminator` already exists in the AST; `isM4Constraint` already accepts it; no Lean change. `M4Subset` `decide` does not evaluate the bytes, so the M5 `check` discharges it.)
- **VAError:** add `WrongDiscriminator { field: &'static str }` + Display.
- **Tests:** `behavior.rs` — accept a correct-prefix account, reject a wrong-prefix one; `lean_spec.rs` — assert the emitted `Constraint.discriminator (ByteArray.mk #[..])` for a known type name (bytes verified against an independent sha256 of `"account:Vault"`).

This leaves the Rust runtime enforcing all five validation constraints {signer, mut, owner, has_one, seeds, discriminator}, matching the Lean-proven set.

---

## 4. Part 1–4 — the four exploit scenarios

Each scenario is one `naive_<s>` + one `verified_<s>` instruction in `verified-anchor-exploits`, plus a litesvm test asserting: **naive(attacker) → Ok + observable bad effect; verified(attacker) → on-chain Err; verified(legit) → Ok** (the positive control proving the verified version isn't a brick).

### Scenario 1 — Cashio infinite-mint (`has_one`, + `owner`)
*Real:* Cashio, Mar 2022, ~$52M; the mint path failed to validate the collateral↔bank↔mint account chain. (Cite the public post-mortem.)
- **Accounts:** `bank`, `collateral` (program-owned; data = [disc(8)][bank pubkey(32)][amount(8)]), `out`.
- **naive_cashio:** read `collateral.amount`, write it to `out.minted` — WITHOUT checking `collateral` owner or `collateral.bank == bank`. Attacker passes a fake `collateral` (attacker-owned or wrong bank) with `amount = u64::MAX` → `out.minted = MAX`.
- **verified_cashio:** struct with `#[account(owner = crate::ID, discriminator = "Collateral", has_one = bank)] collateral` (+ `bank`). `validate` → fake collateral fails owner/discriminator/has_one. Legit collateral (program-owned, right disc, `bank` at offset 8) → Ok.

### Scenario 2 — Account type-confusion (`discriminator`)
*Real:* the canonical Anchor `Account<T>` bug class — a look-alike account of the wrong type (same program) is deserialized because the type tag isn't checked.
- **Accounts:** two types `Vault` (disc "Vault", holds `authority` pubkey) and `Config` (disc "Config"); `vault`, signer `authority`, `out`.
- **naive_confusion:** read the passed account's bytes as a `Vault` (authority at offset 8) and, if `authority == signer.key`, set `out.authorized = 1` — no discriminator check. Attacker passes a `Config` account whose offset-8 bytes equal their key → naive authorizes.
- **verified_confusion:** `#[account(discriminator = "Vault")] vault` + `#[account(signer)] authority`. The `discriminator` is the distinguishing check (both accounts are program-owned, so `owner` can't tell them apart — this is the case only `discriminator` defends). `validate` → the `Config` account's `data[0..8]` ≠ `sha256("account:Vault")[..8]` → rejected. Legit `Vault` → Ok.

### Scenario 3 — Crema fake-account (`owner`)
*Real:* Crema Finance, Jul 2022; a fake "tick"/price account was trusted because its owner program was never checked. (Cite post-mortem.)
- **Accounts:** `price` (expected owner = a fixed `AMM_PROG`; data = [price(8)]), `out`.
- **naive_crema:** read `price.data` → `out.price` — no owner check. Attacker passes an account they own with a fake price → naive copies it.
- **verified_crema:** `#[account(owner = AMM_PROG)] price`. `validate` → attacker-owned account rejected. Legit (owner = AMM_PROG) → Ok.

### Scenario 4 — PDA seeds misuse (`seeds`/`bump`)
*Real:* the bump/unverified-PDA class — a program trusts a passed "vault" pubkey without re-deriving it from seeds.
- **Accounts:** `user` (signer), `vault` (expected PDA = `find_program_address([b"vault", user], program_id)`), `out`.
- **naive_seeds:** credit `vault` (write `out.credited_to = vault.key`) — no PDA re-derivation. Attacker passes their own account as `vault` → naive credits the attacker.
- **verified_seeds:** `#[account(seeds = [b"vault", user.key()], bump)] vault`. `validate` → non-PDA `vault` rejected. Legit (the derived PDA) → Ok.

NOTE: scenarios 1, 3, 4 are runtime-enforced by existing M2–M4 codegen; scenario 2 is enabled by Part 0. The litesvm tests build the `verified-anchor-exploits` `.so` (SBF recipe) and use the M3-style harness.

---

## 5. M5 tie-in + the report

- **M5 tie-in:** the verified structs are all in the M4 subset, so `cargo verified-anchor check -p verified-anchor-exploits` discharges their contracts. The report states: the safe versions are not just empirically safe — they carry the machine-checked M1–M5 guarantee. (`emit_specs!()` is added to the exploits crate; the M5 SBF gate already keeps inventory out of its `.so`.)
- **Report** `docs/exploit-case-studies.md`: for each scenario — the real incident + post-mortem citation; the root cause; the verified-anchor constraint that defends it; the litesvm before/after evidence (naive succeeds, verified rejects, verified-on-legit succeeds); and an **honest boundary** section: we reproduce the *minimized root-cause mechanism* (not full protocols), effects are representative (no SPL CPI), and the spec-carrier API (`u8` fields + explicit constraints) is not yet real `Account<'info, T>` typing — full anchor-lang API fidelity is M7.

---

## 6. Testing / gates

- **Native:** `behavior.rs` discriminator accept/reject; `lean_spec.rs` discriminator shape; existing suites stay green.
- **litesvm:** `runtime_exploits.rs` — the 4 before/after scenarios (≥3 assertions each), against the built `verified-anchor-exploits` `.so`.
- **M5 check:** `cargo verified-anchor check -p verified-anchor-exploits` exits 0.
- **Mandatory full gate** (per HANDOVER): rebuild all `.so`s + `cargo test --workspace` with SBF tools + elan on PATH; `lake build` + zero-sorry (Lean unchanged, but confirm no regression).

---

## 7. Scope / non-goals (M6)

**In.** The discriminator runtime codegen (Part 0); the 4-scenario `verified-anchor-exploits` crate (naive + verified arms); the litesvm before/after suite; the M5 check tie-in; the case-study report.

**Out.** Full `anchor-lang` `Account<'info, T>` typing / API fidelity (M7); faithful SPL-token CPIs; new Lean theorems (reuse the proven contracts); on-chain mainnet deployment; exploits whose root cause is NOT macro-level account validation (e.g., Mango oracle manipulation — economic, out of scope).

---

## 8. Done-bar for Milestone 6

1. Macro emits a runtime discriminator check from `#[account(discriminator = "Name")]` computing `sha256("account:Name")[..8]`; `VAError::WrongDiscriminator`; `behavior.rs` accept/reject + `lean_spec.rs` shape tests pass; the bytes match an independent sha256 of `"account:Vault"`.
2. `verified-anchor-exploits` builds natively and to a BPF `.so`; `emit_specs!()` present.
3. `runtime_exploits.rs` (litesvm): for ALL 4 scenarios, naive(attacker) → Ok with the observable bad effect; verified(attacker) → on-chain Err; verified(legit) → Ok.
4. `cargo verified-anchor check -p verified-anchor-exploits` exits 0 (the verified structs' contracts discharge).
5. `docs/exploit-case-studies.md` covers all 4 (incident + post-mortem + root cause + defending constraint + before/after evidence + honest boundary).
6. Full mandatory gate green: `lake build` + zero-sorry; `cargo test --workspace` (incl. all runtime suites) with SBF tools + elan on PATH; M1–M5 no regressions.
