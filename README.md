<div align="center">

# Verified Anchor

**Formally verified account validation for Solana programs.**

[![Lean](https://img.shields.io/badge/Lean-4.30.0-blue?logo=lean&logoColor=white)](https://lean-lang.org)
[![Rust](https://img.shields.io/badge/Rust-1.93%2B-orange?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Solana](https://img.shields.io/badge/Solana-SBF-9945FF?logo=solana&logoColor=white)](https://solana.com)
[![Axioms](https://img.shields.io/badge/axioms-%5Bpropext%2C%20Quot.sound%5D-22c55e)](#audit-the-proofs)
[![License](https://img.shields.io/badge/license-CC--BY--NC--ND--4.0-lightgrey)](LICENSE)

</div>

Verified Anchor is a drop-in replacement for the Anchor `#[derive(Accounts)]` macro that gates almost every Solana transaction in production. The macros emit the same Rust validation code stock Anchor would emit, plus a Lean 4 obligation discharged at build time against a contract that defines what "valid accounts" means in the Solana account model. The proven core covers `signer` / `mut` / `owner` / `has_one` / `seeds` + `bump` / `discriminator`, the typed-wrapper base checks (`SystemAccount` ownership, `Program<P>` executable + address), and the `init` / `close` lifecycle.

## Why

Solana's correctness depends on a chain of trust: validator runtime (Agave, Firedancer), SBF execution, the Anchor macro that generates per-instruction account validation, and the program's business logic. Layers 1, 2, and 4 receive substantial attention. Layer 3 — the Anchor macro — has not. It is hundreds of lines of procedural-macro code that has never been formally verified, and a bug in it is a bug in every program that depends on it.

The cost of leaving that layer unverified is measurable. Four real Solana mainnet exploits — Cashio (March 2022, ~$48M), Crema Finance (July 2022, ~$8.8M), account-type confusion incidents, and PDA seeds misuse — share the same root cause: a check the program *thought* it was making was either missing, malformed, or trivially bypassable. Each of these is exactly what `#[derive(Accounts)]` is supposed to prevent.

Verified Anchor closes the gap. Every macro expansion ships with a Lean 4 theorem stating that the generated Rust validator is observably equivalent to a contract written in Lean. The theorem is proved once, parameterised over the user's struct. The user writes the same code they would write in stock Anchor. The four CVE classes above are reproduced in this repository as before/after litesvm tests; the verified versions reject the attacker on chain.

## Status

* `v0.1.1`, published on crates.io (`cargo add verified-anchor`). `v0.1.0` is the tagged submission snapshot.
* Lean theorems' axioms: `[propext, Quot.sound]` only. Zero `sorry` / `admit`.
* Out of scope: `realloc`, `zero`, token / mint / associated-token constraints. Custom `constraint = ...` expressions. QEDGen composition demo.

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
LICENSE                               CC BY-NC-ND 4.0
```

## Documentation

* [Original proposal](verified_anchor_proposal.md) — problem statement, approach, milestones.
* [v0.1.0 announcement post](docs/announcement-v0.1.0.md) — the full technical writeup.
* [Trust boundary](docs/verified-anchor-bridge.md) — what is proven, what is not, the Rust↔Lean correspondence.
* [Migrating from Anchor](docs/migrating-from-anchor.md) — supported constraint subset, syntax mapping.
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

pub fn transfer(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let (ctx, _bumps) = Transfer::try_accounts(program_id, accounts, data)?;
    let _amount = ctx.vault.amount;
    // your handler logic
    Ok(())
}
```

Discharge the per-struct proof obligation (the first run fetches the pinned Lean proof library automatically; you only need `elan`/`lake` installed):

```bash
cargo verified-anchor check -p my-crate
```

## Deep technical dive

### The verification chain

The `#[derive(VerifiedAccounts)]` macro emits two artefacts from a single source struct:

1. The Rust `impl Validate` — a `validate(accounts, instr_data, program_id) -> Result<(), VAError>` function that runs at transaction time. Per field it walks the declared constraints in order and short-circuits on the first failure.
2. A Lean `AccountsStruct` literal — the same struct, rendered as a value of the Lean type that the proof side consumes.

A Lean function `genValidate : AccountsStruct → Ctx → Bool` recursively interprets the constraint list. By construction, the Rust validator and `genValidate` examine the same constraints in the same order and return the same answer on the same input. Equivalence is by construction; the proof side proves equivalence to the *contract*, not to the Rust.

The contract `validates : AccountsStruct → Ctx → Prop` is defined declaratively in Lean. It says exactly what each constraint kind means — `signer` means the slot is a signer, `has_one = f` means the 32 bytes at offset 8 of the account data equal the key of the named field, `seeds = […], bump` means the account key is the canonical PDA for those seeds under `program_id`, and so on.

The headline theorem ties the two together:

```
theorem genValidate_sound (s : AccountsStruct) (c : Ctx) (h : M4Subset s) :
  genValidate s c = true ↔ validates s c
```

For every struct in the supported subset (called `M4Subset` in Lean — see the table below), `genValidate` returns `true` precisely when the declarative contract holds. The two sides cannot disagree. The lifecycle theorem `lifecycle_sound` discharges analogous Hoare obligations for `init` and `close`.

Per-program proof obligations are discharged by `cargo verified-anchor check`. For each user struct the cargo tool generates a one-line Lean obligation `decide (M4Subset s)`, then invokes `lake env lean`. If the obligation fails, the build fails.

### Proof scope

| Constraint              | Proven by                                  |
|-------------------------|--------------------------------------------|
| `signer`                | `genValidate_sound`                        |
| `mut`                   | `genValidate_sound`                        |
| `owner = <expr>`        | `genValidate_sound`                        |
| `has_one = <field>`     | `genValidate_sound` (relational)           |
| `seeds = [...], bump`   | `genValidate_sound` (canonical-only PDA)   |
| `discriminator = "..."` | `genValidate_sound`                        |
| `SystemAccount` base: `owner`               | `genValidate_sound`    |
| `Program<P>` base: `executable` + `address` | `genValidate_sound`    |
| `init`/`close`          | `lifecycle_sound` (Hoare-style)            |

### What is proven, what is not

| In the proof | Outside the proof |
|---|---|
| The constraint kinds above. The contract is in `lean/VerifiedAnchor/Contract/`; the proofs are in `lean/VerifiedAnchor/Codegen/`. | Borsh deserialisation of typed account payloads. `BorshFailed` is an honest runtime error, not a silent gap. |
| Concrete Solana primitives — real `findProgramAddress`, lamports, rent, owner / executable flags. Modelled under `VerifiedAnchor.Solana`. | CPI effects beyond `init` / `close`. Token transfers, custom program calls. |
| The init/close lifecycle modelled as state transformers with Hoare pre/post-conditions. | Anchor constraints not modelled in v0.1.0: `realloc`, `zero`, token / mint / associated-token, custom `constraint = expr`. The macro emits a `compile_error!` with a migration-guide pointer. |
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

A self-contained static landing page lives under [`web/`](web). It uses no build step, no
framework, and no external scripts beyond Google Fonts. Preview locally with any static
server:

```bash
cd web && python3 -m http.server 8000
# then open http://localhost:8000
```

To host on **GitHub Pages**:

1. In the repository on GitHub, open **Settings → Pages**.
2. Under **Build and deployment**, set the source to **Deploy from a branch**.
3. Choose **Branch:** `master` and **Folder:** `/web`. Save.

The site goes live at <https://parthrathix0.github.io/Verified-Anchor/> within a minute. A
`web/.nojekyll` file is committed so GitHub Pages serves the file as-is without running it
through Jekyll.

## Contributing

Issues and audit attempts are welcome. Substantive code patches require a prior contributor agreement granting the author the right to incorporate the change under the project license (see below). Open an issue first if you intend to send code.

## License

[CC BY-NC-ND 4.0](LICENSE). This is not a standard open-source license. Practical effects:

* **NonCommercial.** No use for commercial advantage without prior written permission.
* **NoDerivatives.** No distribution of modified versions.
