# Contributing to Verified Anchor

Verified Anchor is a formally verified account-validation layer for Solana: a Lean 4 contract
describing what Anchor's `#[derive(Accounts)]` *should* do, plus proof-producing Rust macros
whose generated code is proven to implement that contract.

That shape makes contributing here slightly different from a normal Rust project. A patch can
compile, pass every test, and still be wrong — because it widened what the generated validator
accepts without widening the proof. This guide exists so you can avoid that.

Contributions are welcome at every level, from a typo to a new Lean soundness theorem.

---

## Ways to contribute

| Kind | Where to start |
|---|---|
| Something behaves incorrectly | [Bug report](../../issues/new?template=bug_report.yml) |
| The validator accepts what the contract rejects, or a check runs unproven | [Soundness gap](../../issues/new?template=soundness_gap.yml) — read [SECURITY.md](SECURITY.md) first if it is exploitable |
| Real Anchor code that will not compile under `#[derive(VerifiedAccounts)]` | [Drop-in gap](../../issues/new?template=drop_in_gap.yml) |
| Docs, tests, exploit case studies, code | [Send a pull request](#how-to-get-a-change-merged) — open an issue first for anything large |

## How to get a change merged

**Every contribution lands through a pull request.** You will not have push access to this
repository, so you cannot commit to `master` — that is normal and expected. You work on a copy
(a *fork*), then ask for your work to be pulled in. A maintainer reviews it and merges.

If you have found a bug but do not want to fix it yourself, just open an issue and stop there.
That is a real contribution on its own.

### If you want to fix an issue

**1. Say so on the issue first.** Leave a comment such as *"I'd like to take this."* This costs
you nothing and prevents two people quietly doing the same work. For anything larger than a
one-file change, sketch your approach in that comment and wait for a reply before writing code —
it is much cheaper to redirect an approach in a comment than in a finished pull request.

**2. Fork the repository.** Press **Fork** at the top-right of the GitHub page, or:

```bash
gh repo fork ParthRathix0/Verified-Anchor --clone
cd Verified-Anchor
```

**3. Create a branch.** Never work on `master`, even in your own fork — it makes later updates
painful.

```bash
git checkout -b fix-json-escaping
```

**4. Make your change, then run the gate.** The full gate is in
[The mandatory gate](#the-mandatory-gate) below. Run it before you push, not after.

**5. Commit.** One logical change per commit; explain *why* in the body.

```bash
git commit -m "fix: escape struct names in the --json report

Closes #2."
```

Writing `Closes #2` in the body makes GitHub close that issue automatically when the pull
request is merged.

**6. Push to your fork.**

```bash
git push -u origin fix-json-escaping
```

**7. Open the pull request.** GitHub prints a link when you push, or:

```bash
gh pr create --base master --fill
```

Fill in the template: what changed, the issue it closes, its soundness impact, and the gate
checklist. If you could not run part of the gate — no SBF toolchain, for instance — tick what
you ran and say plainly what you skipped. An honest gap is fine; a silently skipped step is not.

**8. Sign the CLA.** On your first pull request only, a **CLA** check appears. Click **Details**
next to it, sign in with your GitHub account and accept. It is recorded once against your account
and never asked again. You keep the copyright in your work — see [CLA.md](CLA.md).

**9. Wait for CI.** Every pull request runs the full gate automatically: Lean build, zero
`sorry`, the axiom audit, the SBF build, the litesvm runtime suites, and the proof obligations.
Expect roughly 15–25 minutes. If something fails, read the log, push a fix to the same branch,
and CI re-runs on its own — there is no need to close and reopen anything.

**10. Respond to review.** Push more commits to the same branch; they appear on the pull request
automatically. Please do not force-push during review, as it makes already-posted comments hard
to follow.

**11. A maintainer merges it.** You do not need merge rights, and you should not need to do
anything further.

### Keeping your branch current

If `master` moves while your pull request is open:

```bash
git remote add upstream https://github.com/ParthRathix0/Verified-Anchor.git
git fetch upstream
git rebase upstream/master
git push --force-with-lease
```

This is the one time force-pushing is expected.

## The two directives

Every scoping decision in this project is judged against these two, not against what makes a
tidy change. If you are unsure whether something belongs, this is the test.

**1. Drop-in.** A developer swaps `#[derive(Accounts)]` → `#[derive(VerifiedAccounts)]` on real,
existing Anchor source and it compiles and runs, unedited. Any construct that forces a developer
to *rewrite* their program is a defect, not a scope boundary. Bespoke syntax that real Anchor
code would never contain is a bug to fix, not a limitation to document.

**2. Prevent all bugs.** The proven core keeps expanding toward every account-validation bug
class. Where a construct cannot yet be proven, it still compiles and runs — as an honest,
clearly signalled unproven escape hatch. Never a compile error, never a silent gap.

When these pull against a smaller, faster change, the change gives way.

## Non-negotiable invariants

A pull request that breaks any of these will not be merged. They are what the project's central
claim rests on.

1. **Zero `sorry` and zero `admit`** anywhere under `lean/VerifiedAnchor/`.
2. **All six headline theorems stay at `[propext, Quot.sound]`:**
   `genValidate_sound`, `lifecycle_sound`, `init_establishes_post`, `close_establishes_post`,
   `realloc_establishes_post`, `initIfNeeded_establishes_post`.
   Enforced by `./scripts/audit-axioms.sh`.
3. **Kernel `decide` only.** `native_decide` adds a trust axiom and is banned.
4. **A new trust boundary must be named and empirically cross-checked, never silently
   axiomatized.** There are exactly four today: `sha256`, `isOnCurve`, `rentExemptMinimum`, and
   the Borsh layout model (cross-checked against the real `borsh` crate by
   [`rust/verified-anchor/tests/borsh_differential.rs`](rust/verified-anchor/tests/borsh_differential.rs)).
   If your change needs a fifth, say so explicitly in the PR and document it in
   [`docs/verified-anchor-bridge.md`](docs/verified-anchor-bridge.md).
5. **Host-only dependencies must be `#[cfg(not(target_os = "solana"))]`-gated.** See the war
   story below for why this is not theoretical.

## Toolchain setup

Three toolchains. All are required to run the full gate.

```bash
# 1. Lean 4.30.0, via elan
curl https://raw.githubusercontent.com/leanprover/elan/master/elan-init.sh -sSf | sh
export PATH="$HOME/.elan/bin:$PATH"
cd lean && lake build

# 2. Rust (stable)
cd rust && cargo test -p verified-anchor

# 3. Solana SBF platform-tools
sh -c "$(curl -sSfL https://release.anza.xyz/stable/install)"
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd rust/verified-anchor-program && cargo-build-sbf --no-rustup-override
```

Two details that will cost you an afternoon if you miss them:

* The **platform-tools `rustc`** — the one carrying the `sbf-solana-solana` target — must be
  **first** on `PATH`. A stock rustup `rustc` cannot build these crates.
* **`--no-rustup-override`** works around a rustup 1.26 toolchain-name bug. Without it,
  `cargo-build-sbf` may fail or silently use the wrong compiler.

## The mandatory gate

Run all of this before opening a pull request. Copy-pasteable:

```bash
export PATH="$HOME/.elan/bin:$PATH"
cd lean && lake build && grep -rn "sorry\|admit" VerifiedAnchor/   # must print nothing
cd .. && ./scripts/audit-axioms.sh                                 # must print AXIOM AUDIT PASSED

export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$HOME/.elan/bin:$PATH"
cd rust/verified-anchor-program && cargo-build-sbf --no-rustup-override
cd ../verified-anchor-exploits && cargo-build-sbf --no-rustup-override
cd .. && cargo test --workspace

cargo verified-anchor check -p verified-anchor-example
cargo verified-anchor check -p verified-anchor-exploits
```

### Why the heavy half is not optional

**Native tests alone are not sufficient.** This is the single most important thing to know about
this repository.

`verified-anchor` is a dependency of the on-chain `verified-anchor-program`. Anything host-only
that leaks into it gets compiled into the BPF `.so`. This has happened: the `inventory` crate's
`#[used]` link-section statics were pulled into the SBF ELF, producing an invalid `PT_DYNAMIC`
segment and a loader `InvalidAccountData` at runtime. Every native test stayed green. The
on-chain program was completely broken and nothing noticed until a later sanity pass re-ran the
litesvm suites.

> **Rule of thumb: if you touched anything under `rust/`, rebuild the `.so` and run
> `cargo test --workspace` with both the SBF tools and elan on `PATH`.**

CI runs this whole gate on every pull request, so you will find out either way — but finding out
locally is faster.

## Repo map and the Rust↔Lean seam

```
lean/VerifiedAnchor/
  Solana/          Account model, Pubkey, the real PDA algorithm, and the Borsh field model
                   (Borsh/Ty.lean, Borsh/Locate.lean). Opaque: sha256, isOnCurve.
  Constraints/     Ast.lean — THE SEAM. The constraint AST, SeedSpec/BumpSpec, and the
                   constraint = <expr> sublanguage (Expr.lean).
  Contract/        satisfies + validates — the declarative contract. What SHOULD happen.
  Decision/        validatesBool + the agreement theorem.
  Codegen/         A model of what the macro actually generates (genValidate, apply*), plus
                   the soundness theorems proving model ≡ contract.
  Examples/

rust/
  verified-anchor-macros/   The proc-macros. lib.rs parses #[account(...)], expr.rs compiles
                            constraint expressions into the proven sublanguage or routes them
                            to the escape hatch, ty_map.rs maps Rust types to Borsh Ty.
  verified-anchor/          Runtime: the Validate and Accounts traits, VAError, Context,
                            layout.rs (the Rust mirror of the Lean Borsh model).
  cargo-verified-anchor/    The `cargo verified-anchor check` subcommand.
  verified-anchor-program/  BPF fixture program (publish = false).
  verified-anchor-example/  Worked user crate (publish = false).
  verified-anchor-exploits/ Empirical exploit suite, BPF (publish = false).
```

**The seam is `Constraints/Ast.lean`.** The Rust macro's `lean_spec()` emits an `AccountsStruct`
literal in that AST. `Codegen/` models the generated validator over the same AST and proves it
equivalent to `Contract/`'s declarative `validates`.

The practical consequence: **if you change what the macro emits, you usually need a matching
change on the Lean side**, or the soundness theorem no longer covers the new behaviour. A new
runtime check with no Lean counterpart is exactly the "unproven work inside a proven claim" bug
this project exists to prevent. If you are adding a check, add its `Constraint` variant, its
`genConstraint_*_iff` lemma, and extend `M10Subset`.

## Contribution types, by difficulty

* **Docs** — corrections, clarity, examples. Start here if you are new. No toolchain needed for
  prose-only changes, though CI will still run.
* **Tests** — extra cases in `rust/verified-anchor/tests/`. Needs the Rust and SBF toolchains.
* **Exploit case studies** — reproduce a real Solana incident as a naive/verified litesvm pair
  in `rust/verified-anchor-exploits/`. Moderate; a great way to learn the codebase.
* **Macro / codegen (Rust)** — new constraint parsing, better diagnostics. Moderate-to-hard;
  read the seam section above first, and expect to touch Lean too.
* **Lean proofs** — new constraints in the proven core, extending `M10Subset`, new Hoare
  effects. Hard. **Open an issue to discuss before writing**, so you do not spend a weekend on
  an approach that will not merge.

## Proof obligations

```bash
cargo verified-anchor check -p <crate>          # human-readable report
cargo verified-anchor check -p <crate> --json   # machine-readable
cargo verified-anchor check -p <crate> --deny-unproven   # any unproven check fails CI
cargo verified-anchor check --lean-dir ./lean   # use a local Lean tree instead of the pinned tag
```

The tool finds every `#[derive(VerifiedAccounts)]` struct, generates a Lean obligation per
struct, and discharges it with `lake env lean`.

**Reading `UNPROVEN_CHECKS`.** A `constraint = <expr>` outside the proven sublanguage is
reported as unproven. This is not a soundness hole: an unproven check is an **additional
conjunct** on the proven core, so it can only reject *more*, never accept more. Soundness holds
unconditionally; only completeness is affected. That said, an unproven check is a place where
the tool is not helping you, so shrinking that list is always a welcome contribution.

## Commit and pull request conventions

The mechanics are in [How to get a change merged](#how-to-get-a-change-merged) above.
These are the conventions that section assumes:

* Conventional-commit prefixes, matching the existing history: `feat:`, `fix:`, `docs:`,
  `chore:`, `ci:`, `test:`, `release:`.
* One logical change per pull request. A licence change and a proof change do not belong
  together.
* Explain *why* in the commit body, not just what. The history here is used as documentation.
* Fill in the gate checklist in the pull request template. If you could not run part of the gate
  (no SBF toolchain, say), tick what you ran and say plainly what you skipped — an honest gap is
  fine, a silently skipped step is not.

## Contributor License Agreement

You sign a [CLA](CLA.md) on your first pull request. A **CLA** check appears on it; click
**Details**, sign in with GitHub and accept. It is recorded permanently against your account, so
every later pull request you open is covered automatically.

**You keep the copyright in your contribution.** The CLA grants a licence, it is not an
assignment.

## Security

Do not open a public issue for an exploitable finding. Soundness holes in the proven core are
treated as security issues — see [SECURITY.md](SECURITY.md) for what that means precisely and
how to report privately.

## Code of conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md).
