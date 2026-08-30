<div align="center">

# [Verified Anchor](https://parthrathix0.github.io/Verified-Anchor)

**Formally verified account validation for Solana programs.**

[![crates.io](https://img.shields.io/crates/v/verified-anchor?logo=rust&label=crates.io)](https://crates.io/crates/verified-anchor)
[![Lean](https://img.shields.io/badge/Lean-4.30.0-blue?logo=lean&logoColor=white)](https://lean-lang.org)
[![Solana](https://img.shields.io/badge/Solana-SBF-9945FF?logo=solana&logoColor=white)](https://solana.com)
[![Axioms](https://img.shields.io/badge/axioms-%5Bpropext%2C%20Quot.sound%5D-22c55e)](#audit-the-proofs)
[![License](https://img.shields.io/badge/license-Apache--2.0-22c55e)](LICENSE)
[![CI](https://github.com/ParthRathix0/Verified-Anchor/actions/workflows/ci.yml/badge.svg)](https://github.com/ParthRathix0/Verified-Anchor/actions/workflows/ci.yml)

</div>

Verified Anchor is a formally verified, drop-in replacement for Anchor's #[derive(Accounts)] — the macro that gates account validation on nearly every Solana program. Every macro expansion ships with a build-time Lean 4 proof that the generated Rust validator implements a precise validation contract (signer, mut, owner, has_one, seeds/bump, discriminator, address, executable, rent_exempt, distinct mutable keys, init/close/realloc/zero/init_if_needed, and a relational `constraint = <expr>` sublanguage), so the build fails unless the validation is proven. This eliminates the macro-level account-validation bug class — Cashio, Crema, type confusion, PDA misuse — by construction, with a single dependency: cargo add verified-anchor.

## Why

Solana's correctness depends on a chain of trust: validator runtime (Agave, Firedancer), SBF execution, the Anchor macro that generates per-instruction account validation, and the program's business logic. Layers 1, 2, and 4 receive substantial attention. Layer 3 — the Anchor macro — has not. It is hundreds of lines of procedural-macro code that has never been formally verified, and a bug in it is a bug in every program that depends on it.

The cost of leaving that layer unverified is measurable. Four real Solana mainnet exploits — Cashio (March 2022, ~$48M), Crema Finance (July 2022, ~$8.8M), account-type confusion incidents, and PDA seeds misuse — share the same root cause: a check the program *thought* it was making was either missing, malformed, or trivially bypassable. Each of these is exactly what `#[derive(Accounts)]` is supposed to prevent.

Verified Anchor closes the gap. Every macro expansion ships with a Lean 4 theorem stating that the generated Rust validator is observably equivalent to a contract written in Lean. The theorem is proved once, parameterised over the user's struct. The user writes the same code they would write in stock Anchor. The four CVE classes above are reproduced in this repository as before/after litesvm tests; the verified versions reject the attacker on chain.

## Status

* `v0.4.1` — relicensed to **Apache-2.0**; contribution setup (CLA, contributing guide, security policy) and CI running the full proof gate on every pull request. No API or proof changes.
* `v0.4.0` — M10 constraint-expression sublanguage: `constraint = <expr>` is compiled into a proven relational sublanguage where possible, with an honest, reported escape hatch for the rest; `#[instruction(...)]` named argument binding; a byte-level Borsh field model (`Ty`/`locate`/`readVal`) that gives `has_one` its real field offset instead of a hardcoded one.
* `v0.3.0` — M9 lifecycle parity: `realloc` (+`realloc::payer`/`realloc::zero`, top-up-only and surplus-preserving), `zero` reinit guard, `init_if_needed` (drop-in on a typed `Account<'info, T>`).
* `v0.2.0` — M8 constraint-surface completion: `address`/`executable` explicit annotations, stored/non-canonical bump opt-in, `seeds::program`, automatic distinct-mut-key checking, `rent_exempt = enforce/skip`.
* Lean theorems' axioms: `[propext, Quot.sound]` only. Zero `sorry` / `admit`.
* Out of scope: token / mint / associated-token constraints. Floats and Borsh enums in the constraint sublanguage (fall to the escape hatch).
* Known drop-in gap: **an unlocatable `has_one` target is a build error.** `has_one` needs the target field's real Borsh offset from `T::LAYOUT`, and `#[derive(AccountData)]` truncates that descriptor at the first field it cannot map — a non-literal-length array (`[u8; N]` with `N` a named const), a nested struct, or an enum. A `has_one` target at or behind such a field is rejected at compile time. Unlike `constraint = <expr>`, `has_one` is declarative: there is no developer expression to fall back to. This is the release's largest known departure from "anything Anchor compiles, we compile"; see [`docs/migrating-from-anchor.md`](docs/migrating-from-anchor.md#limitations).

## Packages

| Crate | Description |
| --- | --- |
| [`verified-anchor`](rust/verified-anchor) | Runtime traits (`Validate`, `Accounts<'info>`), `VAError`, prelude, `Context<T>`. |
| [`verified-anchor-macros`](rust/verified-anchor-macros) | Proc-macros: `#[derive(VerifiedAccounts)]`, `#[derive(AccountData)]`, `#[account]`. |
| [`cargo-verified-anchor`](rust/cargo-verified-anchor) | Cargo subcommand discharging Lean proof obligations via `lake env lean`. |
| [`verified-anchor-example`](rust/verified-anchor-example) | Worked user crate. |
| [`verified-anchor-exploits`](rust/verified-anchor-exploits) | Empirical exploit suite (Cashio, Crema, type confusion, PDA seeds). |
| [`verified-anchor-program`](rust/verified-anchor-program) | BPF program used by litesvm runtime tests. |

## Repo structure

```
lean/                                 Lean 4 library (lake build)
  VerifiedAnchor/Solana/              Solana account model + crypto (opaque sha256, isOnCurve)
  VerifiedAnchor/Constraints/         Constraint AST (the Rust↔Lean seam) + Ctx
  VerifiedAnchor/Contract/            `validates : AccountsStruct → Ctx → Prop`
  VerifiedAnchor/Decision/            `validatesBool` + agreement theorem
  VerifiedAnchor/Codegen/             `genValidate` + soundness proofs (Soundness, Lifecycle)
  VerifiedAnchor/Examples/            Worked example (Withdraw.lean)

rust/                                 Cargo workspace
  verified-anchor/                    Runtime crate (traits, errors, prelude, integration tests)
  verified-anchor-macros/             Proc-macro crate
  cargo-verified-anchor/              Cargo subcommand
  verified-anchor-program/            BPF program — init/close + a seeds PDA (litesvm fixture)
  verified-anchor-example/            Worked user crate
  verified-anchor-exploits/           Empirical exploit suite (four CVE classes)

docs/                                 Project documentation
  verified-anchor-bridge.md           Trust boundary + clause-by-clause Rust↔Lean correspondence
  migrating-from-anchor.md            Migration guide + supported constraint subset
  exploit-case-studies.md             The four Solana mainnet incidents, reproduced before/after
  announcement-v0.1.0.md              v0.1.0 release writeup
  publish-checklist.md                crates.io release steps

web/index.html                        Self-contained landing page (deployable on GitHub Pages)
verified_anchor_proposal.md           Original proposal
LICENSE                               Apache-2.0
NOTICE                                Copyright and relicensing notice
CONTRIBUTING.md                       How to contribute (toolchain, gate, invariants)
CLA.md                                Contributor License Agreement
SECURITY.md                           Vulnerability and soundness-gap disclosure
CODE_OF_CONDUCT.md                    Contributor Covenant 2.1
```

## Documentation

* [Original proposal](verified_anchor_proposal.md) — problem statement, approach, milestones.
* [v0.1.0 announcement post](docs/announcement-v0.1.0.md) — the full technical writeup.
* [Trust boundary](docs/verified-anchor-bridge.md) — what is proven, what is not, the Rust↔Lean correspondence.
* [Migrating from Anchor](docs/migrating-from-anchor.md) — supported constraint subset, syntax mapping, opt-outs.
* [Constraint parity matrix](docs/constraint-parity-matrix.md) — every Anchor `#[account]` constraint: proven / honesty-boundary / planned.
* [Exploit case studies](docs/exploit-case-studies.md) — four Solana mainnet incidents reproduced on litesvm.

## Quick start

Install from crates.io — no clone required:

```bash
cargo add verified-anchor             # runtime + the proof-producing macros
cargo install cargo-verified-anchor   # the build-time proof gate
```

Then write the same code you would in stock Anchor:

```rust
use verified_anchor::prelude::*;

declare_id!("YourProgram1111111111111111111111111111111");

#[account]
pub struct Vault {
    pub authority: Pubkey,
    pub amount: u64,
}

#[derive(VerifiedAccounts)]
pub struct Transfer<'info> {
    #[account(mut, has_one = authority)]
    pub vault: Account<'info, Vault>,
    pub authority: Signer<'info>,
}

pub fn transfer<'info>(program_id: &Pubkey, accounts: &'info [AccountInfo<'info>], data: &[u8]) -> ProgramResult {
    let (ctx, _bumps) = Transfer::try_accounts(program_id, accounts, data)?;
    let _amount = ctx.vault.amount;
    // your handler logic
    Ok(())
}

// One line, anywhere in your crate's lib — lets `cargo verified-anchor check` discover the structs.
verified_anchor::emit_specs!();
```

The whole prelude (`Pubkey`, `AccountInfo`, `ProgramResult`, `declare_id!`, the wrappers, the
derives) comes from the single `verified-anchor` dependency — no separate `solana-program` or
`borsh`. Then discharge the per-struct proof obligation (the first run fetches the pinned Lean
proof library automatically; you only need `elan`/`lake` installed):

```bash
cargo verified-anchor check -p my-crate
```

## Deep technical dive

### The verification chain

The `#[derive(VerifiedAccounts)]` macro emits two artefacts from a single source struct:

1. The Rust `impl Validate` — a `validate(accounts, instr_data, program_id) -> Result<(), VAError>` function that runs at transaction time. Per field it walks the declared constraints in order and short-circuits on the first failure.
2. A Lean `AccountsStruct` literal — the same struct, rendered as a value of the Lean type that the proof side consumes.

A Lean function `genValidate : AccountsStruct → Ctx → Bool` recursively interprets the constraint list. By construction, the Rust validator and `genValidate` examine the same constraints in the same order and return the same answer on the same input. Equivalence is by construction; the proof side proves equivalence to the *contract*, not to the Rust.

The contract `validates : AccountsStruct → Ctx → Prop` is defined declaratively in Lean. It says exactly what each constraint kind means — `signer` means the slot is a signer, `has_one = f` means the bytes at the named field's real Borsh offset (located via the field's `Ty` descriptor, not a hardcoded offset) equal the key of the named field, `seeds = […], bump` means the account key is the canonical PDA for those seeds under `program_id`, and so on.

The headline theorem ties the two together:

```
theorem genValidate_sound (s : AccountsStruct) (c : Ctx) (h : M10Subset s) :
  genValidate s c = true ↔ validates s c
```

For every struct in the supported subset (called `M10Subset` in Lean — see the table below), `genValidate` returns `true` precisely when the declarative contract holds. The two sides cannot disagree. The lifecycle theorem `lifecycle_sound` discharges analogous Hoare obligations for `init` and `close`.

Per-program proof obligations are discharged by `cargo verified-anchor check`. For each user struct the cargo tool generates a one-line Lean obligation `decide (M10Subset s)`, then invokes `lake env lean`. If the obligation fails, the build fails.

### Proof scope

| Constraint              | Proven by                                  |
|-------------------------|--------------------------------------------|
| `signer`                | `genValidate_sound`                        |
| `mut`                   | `genValidate_sound`                        |
| `owner = <expr>`        | `genValidate_sound`                        |
| `has_one = <field>`     | `genValidate_sound` (relational)           |
| `seeds = [...], bump`   | `genValidate_sound` (canonical-only PDA)   |
| `seeds = [...], bump = arg(off)` | `genValidate_sound` (stored-bump opt-in) |
| `seeds::program = <expr>` | `genValidate_sound` (foreign program id) |
| `address = <pubkey>`    | `genValidate_sound`                        |
| `executable`            | `genValidate_sound`                        |
| `rent_exempt = enforce` | `genValidate_sound` (opaque `rentExemptMinimum` wall; cross-checked by litesvm) |
| distinct-mut-key check  | `genValidate_sound` (automatic; `allow_duplicate` opt-out) |
| `discriminator = "..."` | `genValidate_sound`                        |
| `zero`                  | `genValidate_sound`                        |
| `constraint = <expr>`   | `genValidate_sound` (relational sublanguage; honest escape hatch otherwise — see below) |
| `SystemAccount` base: `owner`               | `genValidate_sound`    |
| `Program<P>` base: `executable` + `address` | `genValidate_sound`    |
| `init`/`close`          | `lifecycle_sound` (Hoare-style)            |
| `realloc`/`init_if_needed` | `lifecycle_sound` (Hoare-style)         |

### What is proven, what is not

| In the proof | Outside the proof |
|---|---|
| The constraint kinds above. The contract is in `lean/VerifiedAnchor/Contract/`; the proofs are in `lean/VerifiedAnchor/Codegen/`. | Borsh deserialisation of typed account payloads. `BorshFailed` is an honest runtime error, not a silent gap. |
| Concrete Solana primitives — real `findProgramAddress`, lamports, rent, owner / executable flags. Modelled under `VerifiedAnchor.Solana`. | CPI effects beyond `init` / `close`. Token transfers, custom program calls. |
| The init/close lifecycle modelled as state transformers with Hoare pre/post-conditions. `constraint = <expr>` compiled into the proven relational sublanguage. | Token / mint / associated-token constraints (planned M11). `constraint = <expr>` outside the sublanguage (a function call, a multi-segment data path, a float or Borsh-enum field) — routed to an honest, reported escape hatch that still runs, never a `compile_error!` and never a silent gap. |
| Empirical validation: four real Solana mainnet CVE classes are reproduced in `rust/verified-anchor-exploits/` as litesvm before/after. The verified version rejects the attacker on chain in every case. | The Solana runtime contract itself — we trust the runtime to enforce account ownership, signer flags, and writable flags as documented. |

The library's claim is not "your Solana program is now bug-free". The claim is that the macro-level account-validation bug class is eliminated at the framework level for the supported constraint subset. Full discussion in [`docs/verified-anchor-bridge.md`](docs/verified-anchor-bridge.md).

## Build and test

**Lean** (4.30.0, via `elan`; dependency: `batteries`):

```bash
export PATH="$HOME/.elan/bin:$PATH"
cd lean && lake build
```

**Rust workspace** (1.93+):

```bash
cd rust && cargo test --workspace
```

**SBF programs** (requires `solana-cli` platform-tools):

```bash
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd rust/verified-anchor-program && cargo-build-sbf --no-rustup-override
cd rust/verified-anchor-exploits && cargo-build-sbf --no-rustup-override
```

**End-to-end proof discharge** (Lean + cargo together):

```bash
cd rust
cargo run -q -p cargo-verified-anchor -- verified-anchor check -p verified-anchor-example  --lean-dir ../lean
cargo run -q -p cargo-verified-anchor -- verified-anchor check -p verified-anchor-exploits --lean-dir ../lean
```

## Audit the proofs

The headline theorems' axiom dependencies are `[propext, Quot.sound]`, the standard Lean propositional-extensionality and quotient-soundness axioms. No `sorry`, no `Classical.choice`, no `native_decide`.

```bash
cd lean
lake env lean Audit.lean                 # prints both headline theorems' axiom sets
grep -rn 'sorry\|admit' VerifiedAnchor/
```

## Examples

* [`rust/verified-anchor-example`](rust/verified-anchor-example) — worked user crate exercising validation + lifecycle.
* [`rust/verified-anchor-exploits`](rust/verified-anchor-exploits) — four real Solana mainnet CVE classes, naive vs verified.

## Landing page

**Live: <https://parthrathix0.github.io/Verified-Anchor>**

A self-contained static landing page lives under [`web/`](web). It uses no build step, no
framework, and no external scripts beyond Google Fonts. Preview locally with any static
server:

```bash
cd web && python3 -m http.server 8000
# then open http://localhost:8000
```

It is deployed automatically on every push to `master` by
[`.github/workflows/pages.yml`](.github/workflows/pages.yml), which publishes `web/` through
the GitHub Pages **Actions** source. (Pages' branch source only allows `/` or `/docs`, so the
Actions source is used to serve from `web/`.) The page is also Vercel-ready
([`web/vercel.json`](web/vercel.json)): import the repo and set the root directory to `web`.

## Contributing

Contributions are welcome — bug reports, drop-in gaps, soundness findings, docs, tests, and code.

Read [CONTRIBUTING.md](CONTRIBUTING.md) first: it covers the toolchain setup (Lean 4 via elan,
the Solana SBF platform-tools recipe), the mandatory build gate every change must pass, and the
project invariants a patch must not break — zero `sorry`, headline theorems at
`[propext, Quot.sound]`, kernel `decide` only, and BPF-gating for host-only dependencies.

Contributors sign a [CLA](CLA.md) on their first pull request; a bot handles this automatically.
You keep the copyright in your contribution.

Looking for somewhere to start? The
[`good first issue`](https://github.com/ParthRathix0/Verified-Anchor/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22)
label marks issues that are self-contained and do not require knowing the Lean proof layer. Each
one names the files to touch and the check that must pass.

Every pull request runs the full gate in CI — Lean build, zero `sorry`, the axiom audit, the SBF
build, the litesvm runtime suites, and the proof obligations. A green check means your change
preserved every guarantee the project makes.

Security issues and soundness gaps in the proven core follow the private disclosure process in
[SECURITY.md](SECURITY.md) — please do not open a public issue for those.

## License

[Apache-2.0](LICENSE). See [NOTICE](NOTICE) for the copyright and relicensing notice.

Versions 0.1.0 through 0.4.0 were published to crates.io under CC BY-NC-ND 4.0 and still display
that string there, because crates.io freezes licence metadata per published version. The
copyright holder relicenses the whole work: **all versions, including those already published,
are available under Apache-2.0.**
