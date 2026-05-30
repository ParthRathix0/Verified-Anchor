# Verified Anchor — Handover

Context snapshot so a fresh chat can continue without re-deriving anything. Last updated 2026-05-30.

/ project memory at `~/.claude/projects/-home-parth-Desktop-PARTH-Verification/memory/verified-anchor-project.md`
auto-loads each session from this directory — this file is the fuller version. /

---

## What this is

Implementing the **Verified Anchor** proposal (`verified_anchor_proposal.md`): a formally
verified (Lean 4) account-validation contract for Anchor's `#[derive(Accounts)]`, plus
proof-producing Rust proc-macros that generate Solana validation/lifecycle code whose logic
is proven to implement that contract. 7 milestones total; built sequentially, each its own
brainstorm → spec → plan → subagent-driven execution → review → merge cycle.

## Status: M1, M2, M3, M4, M5, M6, M7a, M7b COMPLETE (all merged to `master`)

- **M1 — Lean validation contract.** `lean/VerifiedAnchor/`: concrete Solana model
  (`Solana/`: Pubkey, AccountInfo, real PDA algorithm; only `sha256`/`isOnCurve` axiomatized),
  constraint AST (`Constraints/Ast.lean` — the **Rust↔Lean seam**), `Contract/` with
  `validates : AccountsStruct → Ctx → Prop`, `Decision/` with `validatesBool` +
  `validates_iff_validatesBool`. Example: `Examples/Withdraw.lean`.
- **M2 — proof-producing macros for `mut`/`signer`/`owner`.** `rust/` workspace
  (`verified-anchor-macros` proc-macro + `verified-anchor` runtime, deps `solana-program 2.3.0`).
  Lean `Codegen/`: `genValidate` model + `genValidate_sound` (generated validator ≡ contract).
  Honest trust boundary in `docs/verified-anchor-bridge.md`.
- **M3 — `has_one` (relational) + `init`/`close` (lifecycle).** Relational `genConstraint`,
  `genHasOne`/`genDiscriminator`, `M3Subset` (admits typed `Account<T>`), `genValidate_sound`
  at M3Subset. Lean **Hoare framework** `Codegen/Lifecycle.lean` (`applyInit`/`applyClose` +
  `init_establishes_post`/`close_establishes_post`). Effectful Rust `execute_lifecycle` codegen.
  BPF program crate `rust/verified-anchor-program/` + **litesvm runtime tests**
  (`rust/verified-anchor/tests/runtime_lifecycle.rs`) that execute the generated init/close.
- **M4 — `seeds`/PDA derivation (canonical-only).** `Ctx` is now a structure
  `{ accounts, instrData }`. Third seed source `SeedSpec.instrArg off len` (concrete slice of
  raw instruction data). `genSeeds` mirrors `satisfies (.seeds)` over the concrete
  `findProgramAddress` (opaque `sha256`, **no new axioms**); `genValidate_sound` re-proved at
  `M4Subset` (= M3 + `.seeds`). **Canonical-only**: a declared bump must equal the canonical
  bump — deliberately stricter than stock Anchor's `bump = <stored>` (documented in the bridge
  doc). Generated Rust `validate(accounts, instr_data, program_id)`; macro parses
  `seeds = [b"..", field.key(), arg(off,len)], bump`/`bump = n`. Native tests cross-check the
  real `Pubkey::find_program_address`; litesvm `tests/runtime_seeds.rs` asserts on-chain
  accept/reject.

- **M5 — cargo integration + developer experience.** A `cargo verified-anchor check` subcommand
  (`rust/cargo-verified-anchor/`, std-only) auto-discovers every `#[derive(VerifiedAccounts)]`
  struct via the `inventory` crate + a one-line `verified_anchor::emit_specs!()` in the user's
  lib, generates per-struct Lean obligations, and discharges them with `lake env lean`. Each
  obligation is a uniform `decide`: validation → `decide M4Subset` (`genValidate_sound`);
  lifecycle → `decide StructLifecycleWF` (NEW generic `lifecycle_sound`,
  `Codegen/StructLifecycle.lean`). The macro emits a `compile_error!` for unsupported
  stock-Anchor constraints. Worked example `rust/verified-anchor-example/`; migration guide
  `docs/migrating-from-anchor.md`. **`cargo build` stays Lean-free** (no build.rs); the check is
  the opt-in step. inventory collection works **only same-crate** (so `emit_specs!` is a lib
  `#[cfg(test)] #[test]`).

- **M6 — empirical validation + prod-ready discriminator codegen.** Four real macro-level
  account-validation bug classes reproduced as litesvm before/after in
  `rust/verified-anchor-exploits/` + `rust/verified-anchor/tests/runtime_exploits.rs`:
  Cashio/`has_one`+`owner`+`discriminator`; account type-confusion/`discriminator`;
  Crema/`owner`; PDA seeds. Each scenario asserts naive(attacker)→Ok with observable bad
  on-chain effect, verified(attacker)→Err, verified(legit)→Ok with correct effect. The verified
  versions use the real `#[derive(VerifiedAccounts)]` surface and `cargo verified-anchor check`
  discharges their `M4Subset` contracts. **Closed a real Lean↔Rust gap**: the macro now generates
  a runtime discriminator check from `#[account(discriminator = "Name")]` computing
  `sha256("account:"+Name)[..8]` (real Anchor wire format → interop with real Anchor accounts),
  `VAError::WrongDiscriminator`. Report at `docs/exploit-case-studies.md`.

- **M7a — real Account<'info, T> typing (full drop-in Anchor compatibility).** The macro
  now accepts 6 typed wrappers (`Account<'info, T>`, `Signer<'info>`, `Program<'info, P>`,
  `SystemAccount<'info>`, `UncheckedAccount<'info>`, `AccountInfo<'info>`); bare `u8` is a
  `compile_error!`. A new `#[derive(AccountData)]` proc-macro computes the real Anchor
  `DISCRIMINATOR = sha256("account:" + Name)[..8]` (`rust/verified-anchor-macros/src/account_data_derive.rs`).
  Each derived struct gets BOTH `impl Validate` (proven, unchanged) AND `impl<'info> Accounts<'info>`
  (new developer surface) whose `try_accounts` calls `validate` first, then Borsh-deserialises
  every `Account<'info, T>` field's data. `Context<'a, 'b, 'c, 'info, T>` mirrors stock Anchor.
  `lean_spec` emits real type names (closes the M3 "Vault hardcode" follow-up). Migration is
  clean-break: all ~22 derived structs across the workspace migrated to typed wrappers; new
  trybuild fixture at `rust/verified-anchor-macros/tests/ui/bare_u8.rs` asserts the new error.
  **No Lean source changes; M1-M5 headline theorems' axioms still `[propext, Quot.sound]`** (audited
  on the merge commit). Docs: `docs/migrating-from-anchor.md` "Syntax mapping (M7a)" section,
  `docs/verified-anchor-bridge.md` "Developer surface (M7a)" addendum.

All theorems depend only on `[propext, Quot.sound]` (zero `sorry`/`sorryAx`); verify with
`#print axioms <thm>`.

- **M7b — release packaging.** Three additions: (1) `#[account]` attribute macro bundles
  BorshSerialize + BorshDeserialize + AccountData so users write one line instead of three derives
  (`rust/verified-anchor-macros/src/account_attr.rs`); `#[account(args)]` is a `compile_error!`.
  (2) `Accounts::try_accounts` now returns `(Self, Self::Bumps)`; seeded structs emit
  `pub struct <Name>Bumps { pub <field>: u8, ... }` populated from `find_program_address`,
  non-seeded emit an empty marker — matches stock Anchor's `Context.bumps.pda` shape.
  (3) Dual Apache-2.0 OR MIT license (`LICENSE-MIT`, `LICENSE-APACHE` at repo root), full
  crates.io metadata on the 3 publishable crates (`verified-anchor`, `verified-anchor-macros`,
  `cargo-verified-anchor`); test-fixture crates marked `publish = false`; top-level
  crates.io-ready `README.md`; `docs/publish-checklist.md` documents the order-sensitive
  publish steps. `cargo publish --dry-run` passes for `verified-anchor-macros` and
  `cargo-verified-anchor`; `verified-anchor`'s dry-run is blocked by cargo's first-publish
  chicken-and-egg (documented in checklist). All M1-M5 axioms unchanged.

## Next: M7c (in progress — announcement + publish prep done)

M7c announcement post drafted at `docs/announcement-v0.1.0.md`; all three publishable
Cargo.tomls now carry the live GitHub URL (<https://github.com/ParthRathix0/Verified-Anchor>);
`v0.1.0` tag created locally. Remaining M7c work is gated on user action:

- **Publish to crates.io.** Run `docs/publish-checklist.md` (cargo login → dry-runs → publish
  in order: macros → 60s → verified-anchor → 60s → cargo-verified-anchor). The publish step
  is irreversible and intentionally not automated.
- **Push to GitHub.** `git push origin master --tags` once the remote is wired.
- **QEDGen demo (deferred to v0.2).** Gated on QEDGen availability; not blocking v0.1.0.

See the follow-ups before extending further (`docs/superpowers/m{1,2,3,4,5}-followups.md`): esp.
tighten `Constraint.discriminator` to `Vector UInt8 8`; prove the literal `satisfies` corollary
for the Hoare theorems; add a `fieldKey`-seed test (the path is wired but untested); replace the
`has_one` offset-8 hardcode with a layout-aware codegen (flagged by M6's report).

## Repo layout

```
verified_anchor_proposal.md          the source proposal
lean/                                Lean 4 library (lake); root import: VerifiedAnchor.lean
  VerifiedAnchor/Solana/             account model + crypto (opaque sha256/isOnCurve)
  VerifiedAnchor/Constraints/        Ast.lean (the seam; SeedSpec/BumpSpec) + Context.lean (Ctx = {accounts, instrData})
  VerifiedAnchor/Contract/           satisfies (incl. .seeds, canonical-only) + validates
  VerifiedAnchor/Decision/           validatesBool + agreement
  VerifiedAnchor/Codegen/            Generated (genSeeds), Soundness (M4Subset), Lifecycle, StructLifecycle (lifecycle_sound), ExampleGenerated
  VerifiedAnchor/Examples/Withdraw.lean
rust/                                cargo workspace
  verified-anchor-macros/            #[derive(VerifiedAccounts)] (syn/quote); parses seeds/bump/discriminator; inventory submit!; compile_error for unsupported
  verified-anchor/                   Validate trait (validate(accounts, instr_data, program_id)), VAError, SpecEntry/inventory/emit_specs!, tests/ (behavior, lean_spec, runtime_lifecycle, runtime_seeds, runtime_exploits)
  verified-anchor-program/           BPF program exercising init/close + a seeds PDA instruction (cdylib)
  cargo-verified-anchor/             `cargo verified-anchor check` subcommand (collect→generate→lake), std-only; tests/cli.rs e2e
  verified-anchor-example/           worked user crate (validation + lifecycle) using emit_specs!()
  verified-anchor-exploits/          M6 empirical exploit-suite BPF program (4 scenarios: naive_* + verified_* instruction arms)
docs/
  verified-anchor-bridge.md          Rust↔Lean correspondence + trust boundary (READ THIS)
  migrating-from-anchor.md           M5 migration guide (supported subset, workflow, boundaries)
  exploit-case-studies.md            M6 empirical case studies (4 scenarios + tie-in + honest boundary)
  superpowers/specs/                 design docs: 2026-05-27 M1, M2; 2026-05-28 M3, M4; 2026-05-29 M5, M6
  superpowers/plans/                 implementation plans: M1, M2, M3, M4, M5, M6
  superpowers/m{1,2,3,4,5}-followups.md  deferred items per milestone
HANDOVER.md                          this file
```

## Toolchain recipes (load-bearing — installed during M2/M3)

**Lean** (4.30.0 / Lake 5.0.0, via elan; dep: `batteries` pinned in `lean/lakefile.toml`):
```bash
export PATH="$HOME/.elan/bin:$PATH"
cd lean && lake build                      # full build; #guard/example/theorem are the tests
grep -rn "sorry\|admit" VerifiedAnchor/    # must be empty
```

**Rust native** (rustc 1.93.1):
```bash
cd rust && cargo test -p verified-anchor   # behavior + lean_spec (native, fast)
```

**SBF build** (solana-cli 4.0.0; the rustup path is BROKEN here — use this exact recipe):
```bash
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd rust/verified-anchor-program && cargo-build-sbf --no-rustup-override
# -> rust/target/deploy/verified_anchor_program.so   (workspace-shared target dir)
```
The platform-tools rustc (has `sbf-solana-solana` target) MUST be first on PATH;
`--no-rustup-override` avoids a rustup 1.26 toolchain-name bug.

**litesvm runtime tests** (build the .so first, then):
```bash
cd rust && cargo test -p verified-anchor --test runtime_lifecycle --test runtime_seeds
```
`litesvm = "0.6"` + the split `solana-*` crates (NOT a monolithic `solana-sdk`). Note: system
`libssl-dev` is absent; if a future dep needs OpenSSL, add `openssl = { version="0.10",
features=["vendored"] }` (cc/perl/make are present). litesvm needs a compiled `.so` (BPF), which
is why the SBF toolchain was installed.

**MANDATORY full gate — run ALL of these before declaring any milestone/fix done.** The native
fast tests are NOT sufficient: a change to `verified-anchor` or `verified-anchor-macros` flows
into the BPF program (`verified-anchor-program` depends on `verified-anchor` + derives), so it
can break the `.so` without any native test noticing. (This actually happened: M5's `inventory`
dep + the derive's `inventory::submit!` corrupted the SBF ELF — caught only because a later
sanity pass re-ran the runtime suites. Inventory is now gated by `#[cfg(not(target_os =
"solana"))]`.) Always rebuild the `.so` and run the litesvm suites as part of the gate:
```bash
export PATH="$HOME/.elan/bin:$PATH"; cd lean && lake build && grep -rn "sorry\|admit" VerifiedAnchor/   # lean (must be empty)
cd ../rust && cargo test --workspace --exclude verified-anchor   # native fast tests... (or below)
# THEN the load-bearing part — rebuild .so + runtime:
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$HOME/.elan/bin:$PATH"
cd rust/verified-anchor-program && cargo-build-sbf --no-rustup-override
cd .. && cargo test --workspace   # incl. runtime_lifecycle + runtime_seeds + the cli e2e (needs lake on PATH)
```
Rule of thumb: **if you touched anything under `rust/`, rebuild the `.so` and run `cargo test
--workspace` with both the SBF tools and elan on PATH.**

## Conventions / invariants

- **Zero `sorry`**; every headline theorem verified clean via `#print axioms` ([propext,
  Quot.sound]). `native_decide` avoided (would add a trust axiom) — kernel `decide` only.
- **Opaque `sha256` wall:** constraints that hash (`discriminator`, `seeds`) are decidable but
  don't reduce under `#eval`/`decide`; demonstrate them via concrete data + `checkConstraint`,
  prove the rest symbolically (see `Examples/Withdraw.lean`, `Codegen/ExampleGenerated.lean`).
- **The `Constraints` AST is the Rust↔Lean seam.** Rust `lean_spec()` emits an `AccountsStruct`
  literal; the generated validator's logic is modeled by `genValidate`/`apply*` and proven ≡ contract.
- **Honest trust boundary:** we model effects (e.g. `create_account`), not the CPI dispatch or
  rustc/sBPF codegen. litesvm tests empirically cross-check the effect models.
- **Host-only deps must be BPF-gated.** `verified-anchor` is a dependency of the on-chain
  `verified-anchor-program`, so anything host-only (currently `inventory`, used by the M5
  spec-collection API + the derive's `inventory::submit!`) MUST be behind
  `#[cfg(not(target_os = "solana"))]` — otherwise it gets compiled into the BPF `.so` and can
  corrupt the ELF (inventory's `#[used]` link-section statics → invalid PT_DYNAMIC → loader
  `InvalidAccountData`). Verify with the litesvm runtime tests (see the MANDATORY full gate).

## How the work is run (process)

Each milestone: `superpowers:brainstorming` → design doc in `docs/superpowers/specs/` (committed,
user-reviewed) → `superpowers:writing-plans` → plan in `docs/superpowers/plans/` →
`superpowers:subagent-driven-development` on a feature branch (fresh implementer subagent per
task; **opus** for hard proofs like `genValidate_sound` and the Hoare theorems; **sonnet** for
mechanical tasks; controller reviews each committed diff; dedicated reviewer subagents on
heavy/load-bearing tasks) → final whole-implementation review → `superpowers:finishing-a-development-branch`
(merge `--no-ff` to `master`, delete branch). Toolchain feasibility (SBF, litesvm) is **probed
before designing** to avoid churn.

## To resume in a new chat

Say e.g. "continue Verified Anchor — start M7 (release + Account<'info,T>)". The assistant should:
read this file + `verified_anchor_proposal.md` (Milestone 7 section) + `docs/verified-anchor-bridge.md`
+ `docs/exploit-case-studies.md` (M6 boundary notes flag the spec-carrier API as M7 scope), confirm
the build is green (recipes above), then brainstorm M7. The big M7 lift is **real `Account<'info, T>`
typing** — verified-anchor currently uses `u8` spec-carrier fields with explicit `#[account(...)]`
constraints; M7 lifts this to anchor-lang's typed-account surface so adopting verified-anchor is a
nearly drop-in replacement for stock Anchor (the M5 migration guide describes the path).
