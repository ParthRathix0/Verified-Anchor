# Verified Anchor — Handover

Context snapshot so a fresh chat can continue without re-deriving anything. Last updated 2026-05-28.

/ project memory at `~/.claude/projects/-home-parth-Desktop-PARTH-Verification/memory/verified-anchor-project.md`
auto-loads each session from this directory — this file is the fuller version. /

---

## What this is

Implementing the **Verified Anchor** proposal (`verified_anchor_proposal.md`): a formally
verified (Lean 4) account-validation contract for Anchor's `#[derive(Accounts)]`, plus
proof-producing Rust proc-macros that generate Solana validation/lifecycle code whose logic
is proven to implement that contract. 7 milestones total; built sequentially, each its own
brainstorm → spec → plan → subagent-driven execution → review → merge cycle.

## Status: M1, M2, M3, M4, M5 COMPLETE (all merged to `master`)

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

All theorems depend only on `[propext, Quot.sound]` (zero `sorry`/`sorryAx`); verify with
`#print axioms <thm>`.

## Next: M6 → M7

- **M6 — empirical validation** against a historical Solana exploit: identify one whose root
  cause was macro-level account validation, and show the verified-anchor version either fails to
  compile (caught the misuse) or carries a proof its preconditions are unreachable. Use the
  litesvm harness (M3) + the `cargo verified-anchor check` flow (M5).
- **M7 — release + QEDGen integration.**

See the follow-ups before extending further (`docs/superpowers/m{1,2,3,4,5}-followups.md`): esp.
tighten `Constraint.discriminator` to `Vector UInt8 8`; prove the literal `satisfies` corollary
for the Hoare theorems; add a `fieldKey`-seed test (the path is wired but untested).

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
  verified-anchor-macros/            #[derive(VerifiedAccounts)] (syn/quote); parses seeds/bump; inventory submit!; compile_error for unsupported
  verified-anchor/                   Validate trait (validate(accounts, instr_data, program_id)), VAError, SpecEntry/inventory/emit_specs!, tests/ (behavior, lean_spec, runtime_lifecycle, runtime_seeds)
  verified-anchor-program/           BPF program exercising init/close + a seeds PDA instruction (cdylib)
  cargo-verified-anchor/             `cargo verified-anchor check` subcommand (collect→generate→lake), std-only; tests/cli.rs e2e
  verified-anchor-example/           worked user crate (validation + lifecycle) using emit_specs!()
docs/
  verified-anchor-bridge.md          Rust↔Lean correspondence + trust boundary (READ THIS)
  migrating-from-anchor.md           M5 migration guide (supported subset, workflow, boundaries)
  superpowers/specs/                 design docs: 2026-05-27 M1, M2; 2026-05-28 M3, M4; 2026-05-29 M5
  superpowers/plans/                 implementation plans: M1, M2, M3, M4, M5
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
cd rust && cargo test -p verified-anchor --test runtime_lifecycle
```
`litesvm = "0.6"` + the split `solana-*` crates (NOT a monolithic `solana-sdk`). Note: system
`libssl-dev` is absent; if a future dep needs OpenSSL, add `openssl = { version="0.10",
features=["vendored"] }` (cc/perl/make are present). litesvm needs a compiled `.so` (BPF), which
is why the SBF toolchain was installed.

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

Say e.g. "continue Verified Anchor — start M6 (empirical exploit study)". The assistant should: read
this file + `verified_anchor_proposal.md` (Milestone 6 section) + `docs/verified-anchor-bridge.md` +
`docs/superpowers/m5-followups.md`, confirm the build is green (recipes above), then brainstorm M6.
For M6, the litesvm harness (M3) and the `cargo verified-anchor check` flow (M5) are the tools to
reach for; pick a historical exploit whose root cause is macro-level account validation.
