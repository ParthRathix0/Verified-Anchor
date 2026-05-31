# Verified Anchor — Handover

Context snapshot so a fresh chat can continue without re-deriving anything. Last updated 2026-05-31.

/ project memory at `~/.claude/projects/-home-parth-Desktop-PARTH-Verification/memory/verified-anchor-project.md`
auto-loads each session from this directory — this file is the fuller version. /

---

## What this is

Implementing the **Verified Anchor** proposal (`verified_anchor_proposal.md`): a formally
verified (Lean 4) account-validation contract for Anchor's `#[derive(Accounts)]`, plus
proof-producing Rust proc-macros that generate Solana validation/lifecycle code whose logic
is proven to implement that contract. 7 milestones total; built sequentially, each its own
brainstorm → spec → plan → subagent-driven execution → review → merge cycle.

## Status: v0.1.0 SHIPPED (M1–M7c complete, all on `master`, tagged `v0.1.0`)

The repository is public at <https://github.com/ParthRathix0/Verified-Anchor> and is the
submission artefact for the **Solana Superteam capstone**. License: CC BY-NC-ND 4.0
(`LICENSE` at the repo root). Final-state milestones below.

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
  (3) Full crates.io metadata on the 3 publishable crates (`verified-anchor`,
  `verified-anchor-macros`, `cargo-verified-anchor`); test-fixture crates marked
  `publish = false`; top-level crates.io-ready `README.md`; `docs/publish-checklist.md`
  documents the order-sensitive publish steps. `cargo publish --dry-run` passes for
  `verified-anchor-macros` and `cargo-verified-anchor`; `verified-anchor`'s dry-run is blocked
  by cargo's first-publish chicken-and-egg (documented in checklist). All M1–M5 axioms
  unchanged. Note: M7b originally specified dual Apache-2.0 OR MIT; the user replaced this in
  M7c with a CC BY-NC-ND 4.0 single-file `LICENSE` (see M7c below).

- **M7c — submission cut.** v0.1.0 announcement post drafted at
  `docs/announcement-v0.1.0.md`; landing page at `web/index.html` (single-file, no
  framework, ~880 lines, deployable on GitHub Pages from `/web`). README rewritten to match
  the conventions of top Solana org repositories — declarative noun-form section headings
  (Status / Packages / Repo structure / Documentation / Quick start / Deep technical dive /
  Build and test / Audit / Examples / Landing page / Contributing / License), badge row,
  no question-led prose. Process-internal documentation removed (`docs/superpowers/`,
  21 files of design specs, plans, and followups); the user-facing docs
  (`announcement-v0.1.0.md`, `verified-anchor-bridge.md`, `migrating-from-anchor.md`,
  `exploit-case-studies.md`) had milestone tags (M3, M4, M6, M7a) and conversational headings
  stripped to read as standard project documentation. License switched to
  CC BY-NC-ND 4.0 (single-file `LICENSE` at the repo root) — NonCommercial and NoDerivatives;
  all three publishable crates' Cargo.toml `license` field updated to `CC-BY-NC-ND-4.0` (SPDX);
  README + announcement reflect the same. All `REPLACE_ME` URLs replaced with the live
  GitHub URL (<https://github.com/ParthRathix0/Verified-Anchor>). Repo pushed to GitHub;
  `v0.1.0` tag created locally and pushed (`origin/v0.1.0`).

  **Deferred from M7c (not blocking v0.1.0):** real `cargo publish` to crates.io (the user
  runs the checklist; macros first → 60s → verified-anchor → 60s → cargo-verified-anchor);
  QEDGen composition demo (gated on QEDGen availability — bumped to a future minor).

All theorems depend only on `[propext, Quot.sound]` (zero `sorry`/`sorryAx`); verify with
`#print axioms <thm>`.

## Post-v0.1.0 hardening (audit pass, 2026-05-31)

A deep seam audit (Rust codegen ↔ Lean `genValidate` model) found and fixed several issues.
All changes are in the working tree on `master` (not yet a new tag). Full gate re-run green:
`lake build` clean, **0 `sorry`/`admit`**, headline axioms still `[propext, Quot.sound]`, both
SBF `.so`s rebuilt clean, `cargo test --workspace` **50 passed / 0 failed** (incl. all litesvm
runtime suites + the cli e2e).

- **Typed-wrapper base checks are now MODELLED, not just transcribed (closed a real seam gap).**
  The macro's `wrapper_implied` emits `owner == system_program` for `SystemAccount<'info>` and
  `executable + key == P::ID` for `Program<'info, P>`. These ran at runtime but had **no Lean
  counterpart** (`AccountType.systemAccount`/`uncheckedAccount` both implied `[]`, and `lean_spec`
  mapped `Program` → `uncheckedAccount`), so the generated validator did unproven work —
  contradicting the headline "every check is proven". Fix: added `Constraint.executable` and
  `Constraint.address (expected : Pubkey)` to the AST; wired `AccountType.impliedConstraints`
  (`systemAccount` → `[owner Pubkey.zero]`, `program id` → `[executable, address id]`); proved
  `genConstraint_{executable,address}_iff`; extended `isM4Constraint`, the M4 dispatcher, and
  `lifecycle_sound`; pointed the macro's `lean_spec` at `AccountType.program Pubkey.zero`. The
  placeholder pubkeys are schematic (the theorem is ∀ over the pubkey), exactly like the existing
  `owner = EXPR` → `ownerPlaceholder` pattern. Closed-loop `#guard`s + `*_sound` theorems added in
  `Codegen/ExampleGenerated.lean` (`sysAcct_*`, `prog_*`). `genValidate_sound` axioms unchanged.
- **`arg(off,len)` seed no longer panics on short instruction data.** The generated slice
  `&instr_data[off..off+len]` (both the validate-side seed block and the `Bumps` init) is now
  clamped to `instr_data.len()` — `&instr_data[off.min(len)..(off+len).min(len)]` — which mirrors
  the Lean model's `ByteArray.extract off (off+len)` (also clamps). A too-short `instr_data` now
  yields a clean `WrongPda`, not an out-of-bounds panic.
- **`execute_lifecycle` is bounds-guarded.** It indexed accounts by field position with no length
  check (a panic on a short slice if a caller invoked it without `validate` first). It now returns
  `NotEnoughAccounts`, mirroring the none-safe Lean `applyInit`/`applyClose` (`accounts[idx]?`).
- **The checker is proven non-vacuous.** Added a negative test
  (`cargo-verified-anchor/src/discharge.rs` → `discharge_rejects_a_false_obligation`) asserting
  that `discharge` FAILS when a Lean obligation is false (an `init` constraint is not in
  `M4Subset`, so `by decide` errors). Previously only the positive path was tested.

Docs updated in the same pass: `docs/verified-anchor-bridge.md` (correspondence-table rows for
`executable`/`address`, the "wrapper base checks are modelled" note, the instr-arg clamping note);
`docs/announcement-v0.1.0.md` (proven-core list); `docs/migrating-from-anchor.md` (wrapper note).

## What is left for the user to do (after submission)

- `docs/publish-checklist.md` walks through `cargo login` → dry-runs → publish.
- GitHub Pages: Settings → Pages → Deploy from a branch → `master` `/web` will serve the
  landing page at <https://parthrathix0.github.io/Verified-Anchor/>. `.nojekyll` is committed.

## Follow-ups (not blocking v0.1.0)

The internal followup notes were removed in the submission cleanup. The substantive carries
that still apply to a future minor are:

- Tighten `Constraint.discriminator` to `Vector UInt8 8` in the Lean AST.
- Prove the literal `satisfies (.init/.close)` proposition as a corollary of
  `init_establishes_post` / `close_establishes_post` (currently a tracked gap mentioned in
  `docs/verified-anchor-bridge.md`).
- Replace the `has_one` offset-8 hardcode with a layout-aware codegen
  (flagged in `docs/exploit-case-studies.md` under Limitations). Model and code currently
  AGREE (both read offset 8), so the proof is honest; this is a feature limit, not a soundness
  gap, and fixing it needs Borsh field-offset extraction from the account type.
- QEDGen composition demo (M7c deferred item).

## Repo layout (as shipped at v0.1.0)

```
README.md                            top-level landing page (badges, packages, deep dive)
LICENSE                              CC BY-NC-ND 4.0 (single file; replaces the earlier
                                     LICENSE-MIT + LICENSE-APACHE)
HANDOVER.md                          this file (kept for fresh-session resumes; the user
                                     plans to remove it post-submission)
verified_anchor_proposal.md          the source proposal (kept for the same reason)

lean/                                Lean 4 library (lake); root import: VerifiedAnchor.lean
  VerifiedAnchor/Solana/             account model + crypto (opaque sha256/isOnCurve)
  VerifiedAnchor/Constraints/        Ast.lean (the seam; SeedSpec/BumpSpec) + Context.lean
                                     (Ctx = {accounts, instrData})
  VerifiedAnchor/Contract/           satisfies (incl. .seeds, canonical-only) + validates
  VerifiedAnchor/Decision/           validatesBool + agreement
  VerifiedAnchor/Codegen/            Generated (genSeeds), Soundness (M4Subset), Lifecycle,
                                     StructLifecycle (lifecycle_sound), ExampleGenerated
  VerifiedAnchor/Examples/Withdraw.lean

rust/                                cargo workspace
  verified-anchor-macros/            #[derive(VerifiedAccounts)], #[derive(AccountData)],
                                     #[account] attribute macro; parses
                                     seeds/bump/discriminator; inventory submit!;
                                     compile_error for unsupported. Trybuild fixtures
                                     for unsupported-constraint, bare-u8, #[account(args)].
  verified-anchor/                   Runtime: Validate / Accounts<'info> traits, VAError,
                                     prelude, Context<T>; SpecEntry/inventory/emit_specs!;
                                     tests/ (behavior 28 tests, lean_spec 4,
                                     runtime_lifecycle 2, runtime_seeds 2,
                                     runtime_exploits 4).
  verified-anchor-program/           BPF program exercising init/close + a seeds PDA
                                     instruction (cdylib; publish = false).
  cargo-verified-anchor/             `cargo verified-anchor check` subcommand
                                     (collect → generate → lake), std-only; tests/cli.rs e2e.
  verified-anchor-example/           Worked user crate (validation + lifecycle) using
                                     emit_specs!() (publish = false).
  verified-anchor-exploits/          Empirical exploit-suite BPF program — 4 scenarios:
                                     naive_* + verified_* instruction arms (publish = false).

docs/                                Project documentation (all submission-facing)
  verified-anchor-bridge.md          Rust↔Lean correspondence + trust boundary
  migrating-from-anchor.md           migration guide (supported subset, workflow, limits)
  exploit-case-studies.md            four real Solana mainnet incidents, before/after
  announcement-v0.1.0.md             v0.1.0 release writeup (technical, audience: Solana devs)
  publish-checklist.md               crates.io release steps for the user to run

web/                                 Self-contained landing page (deployable on GH Pages)
  index.html                         the page (~880 lines, no framework, no build step)
  README.md                          GH Pages deploy instructions
  .nojekyll                          marker so GH Pages serves the file as-is

(Removed during M7c submission cleanup: docs/superpowers/{specs,plans,m*-followups.md} —
21 internal workflow files. The substantive carryovers are listed under Follow-ups above.)
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

## How the work was run (process — historical)

Each milestone followed: `superpowers:brainstorming` → design doc → `superpowers:writing-plans`
→ implementation plan → `superpowers:subagent-driven-development` on a feature branch (fresh
implementer subagent per task; **opus** for hard proofs like `genValidate_sound` and the Hoare
theorems; **sonnet** for mechanical tasks; controller reviewed each committed diff; dedicated
reviewer subagents on heavy/load-bearing tasks) → final whole-implementation review →
`superpowers:finishing-a-development-branch` (merge `--no-ff` to `master`, delete branch).
Toolchain feasibility (SBF, litesvm) was probed before designing to avoid churn.

The design specs and implementation plans this process produced lived under
`docs/superpowers/` and were removed in M7c when the repo was cleaned for submission. The
substance is preserved in the milestone history above and in the documentation under `docs/`.

## To resume in a new chat

The project is at v0.1.0 and shipped. There is no active milestone in flight. Likely reasons
to open a new chat:

- **Pre-publish polish.** Replace `REPLACE_ME` (already done), run `docs/publish-checklist.md`
  to push to crates.io.
- **A follow-up listed above** (`Constraint.discriminator` tightening, `satisfies` corollary,
  layout-aware `has_one`, QEDGen demo). (The earlier `fieldKey` seed-test gap is closed —
  `runtime_exploits` scenario 4 exercises a `user.key()` seed on-chain.)
- **A reported bug or audit finding** against the v0.1.0 surface.

The assistant should: read this file + `verified_anchor_proposal.md` (for context) +
`docs/verified-anchor-bridge.md` (for the trust boundary), confirm the build is green using
the recipes in the **Toolchain recipes** section above, then brainstorm the change.

For a substantive new feature, the mandatory full gate at the end of the change is the same
as it was during M1–M7c: `lake build` clean + `grep -rn 'sorry\|admit' VerifiedAnchor/` empty
+ both headline theorems' `#print axioms` still `[propext, Quot.sound]` + both SBF `.so`s
clean (no `PT_DYNAMIC`) + `cargo test --workspace` green + both
`cargo verified-anchor check` runs exit 0.
