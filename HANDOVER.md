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

## Status: M1, M2, M3 COMPLETE (all merged to `master`)

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

All theorems depend only on `[propext, Quot.sound]` (zero `sorry`/`sorryAx`); verify with
`#print axioms <thm>`.

## Next: M4 → M7

- **M4 — `seeds`/PDA derivation.** Verify the generated PDA-derivation code matches declared
  seeds. Axiomatize `find_program_address` (already modeled concretely over opaque crypto in
  `Solana/Crypto.lean`). Extend the subset + codegen + (likely) a `seeds`-specific soundness.
- **M5 — cargo integration + full `anchor-lang` API compat.** A cargo plugin / build flow that
  runs `lake build` alongside `cargo build`; real Anchor-compatible API surface.
- **M6 — empirical validation** against a historical Solana exploit (use the litesvm harness).
- **M7 — release + QEDGen integration.**

Before extending the AST for M4, see the follow-ups (esp. tightening
`Constraint.discriminator` to `Vector UInt8 8`).

## Repo layout

```
verified_anchor_proposal.md          the source proposal
lean/                                Lean 4 library (lake); root import: VerifiedAnchor.lean
  VerifiedAnchor/Solana/             account model + crypto (opaque sha256/isOnCurve)
  VerifiedAnchor/Constraints/        Ast.lean (the seam) + Context.lean
  VerifiedAnchor/Contract/           satisfies + validates
  VerifiedAnchor/Decision/           validatesBool + agreement
  VerifiedAnchor/Codegen/            Generated, Soundness, Lifecycle, ExampleGenerated
  VerifiedAnchor/Examples/Withdraw.lean
rust/                                cargo workspace
  verified-anchor-macros/            #[derive(VerifiedAccounts)] (syn/quote)
  verified-anchor/                   Validate trait, VAError, tests/ (behavior, lean_spec, runtime_lifecycle)
  verified-anchor-program/           BPF program exercising init/close (cdylib)
docs/
  verified-anchor-bridge.md          Rust↔Lean correspondence + trust boundary (READ THIS)
  superpowers/specs/                 design docs: 2026-05-27 M1, M2; 2026-05-28 M3
  superpowers/plans/                 implementation plans: M1, M2, M3
  superpowers/m{1,2,3}-followups.md  deferred items per milestone
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

Say e.g. "continue Verified Anchor — start M4 (seeds/PDA)". The assistant should: read this file
+ `verified_anchor_proposal.md` (Milestone 4 section) + `docs/verified-anchor-bridge.md` +
`docs/superpowers/m3-followups.md`, confirm the build is green (recipes above), then brainstorm M4.
