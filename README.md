<div align="center">

# Verified Anchor

**Formally verified account validation for Solana programs.**

[![Lean](https://img.shields.io/badge/Lean-4.30.0-blue?logo=lean&logoColor=white)](https://lean-lang.org)
[![Rust](https://img.shields.io/badge/Rust-1.93%2B-orange?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Solana](https://img.shields.io/badge/Solana-SBF-9945FF?logo=solana&logoColor=white)](https://solana.com)
[![Axioms](https://img.shields.io/badge/axioms-%5Bpropext%2C%20Quot.sound%5D-22c55e)](#audit)
[![License](https://img.shields.io/badge/license-CC--BY--NC--ND--4.0-lightgrey)](LICENSE)

</div>

Verified Anchor is a drop-in replacement for the Anchor `#[derive(Accounts)]` macro that gates almost every Solana transaction in production. The macros emit the same Rust validation code stock Anchor would emit, plus a Lean 4 obligation discharged at build time against a contract that defines what "valid accounts" means in the Solana account model. The proven core covers `signer` / `mut` / `owner` / `has_one` / `seeds` + `bump` / `discriminator`, and the `init` / `close` lifecycle.

## Status

* `v0.1.0`. Initial release.
* Lean theorems' axioms: `[propext, Quot.sound]` only. Zero `sorry` / `admit`.
* Four real Solana mainnet CVE classes reproduced as on-chain before/after tests; all four verified versions reject the attacker.
* Out of scope for v0.1.0: `realloc`, `zero`, token / mint / associated-token constraints. Custom `constraint = ...` expressions. QEDGen composition demo.

## Packages

| Crate | Description |
| --- | --- |
| [`verified-anchor`](rust/verified-anchor) | Runtime traits (`Validate`, `Accounts<'info>`), `VAError`, prelude, `Context<T>`. |
| [`verified-anchor-macros`](rust/verified-anchor-macros) | Proc-macros: `#[derive(VerifiedAccounts)]`, `#[derive(AccountData)]`, `#[account]`. |
| [`cargo-verified-anchor`](rust/cargo-verified-anchor) | Cargo subcommand discharging Lean proof obligations via `lake env lean`. |
| [`verified-anchor-example`](rust/verified-anchor-example) | Worked user crate. |
| [`verified-anchor-exploits`](rust/verified-anchor-exploits) | Empirical exploit suite (Cashio, Crema, type confusion, PDA seeds). |
| [`verified-anchor-program`](rust/verified-anchor-program) | BPF program used by litesvm runtime tests. |

## Documentation

* [Original proposal](verified_anchor_proposal.md) — problem statement, approach, milestones.
* [v0.1.0 announcement post](docs/announcement-v0.1.0.md) — the technical writeup.
* [Trust boundary](docs/verified-anchor-bridge.md) — what's proven, what isn't, the Rust↔Lean correspondence.
* [Migrating from Anchor](docs/migrating-from-anchor.md) — supported constraint subset, syntax mapping.
* [Exploit case studies](docs/exploit-case-studies.md) — four real Solana mainnet incidents, before/after.
* [Publish checklist](docs/publish-checklist.md) — crates.io release steps.

## Quick start

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

Discharge the per-struct proof obligation:

```bash
cargo verified-anchor check -p my-crate --lean-dir <path-to-lean-source>
```

A side-by-side mapping with stock Anchor lives in the [migration guide](docs/migrating-from-anchor.md).

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

<a id="audit"></a>

## Audit the proofs

The headline theorems' axiom dependencies are `[propext, Quot.sound]`, the standard Lean propositional-extensionality and quotient-soundness axioms. No `sorry`, no `Classical.choice`, no `native_decide`.

```bash
cd lean
lake env lean -c '#print axioms VerifiedAnchor.genValidate_sound'
lake env lean -c '#print axioms VerifiedAnchor.lifecycle_sound'
grep -rn 'sorry\|admit' VerifiedAnchor/
```

## Examples

* [`rust/verified-anchor-example`](rust/verified-anchor-example) — worked user crate exercising validation + lifecycle.
* [`rust/verified-anchor-exploits`](rust/verified-anchor-exploits) — four real Solana mainnet CVE classes, naive vs verified.

## Contributing

Issues and audit attempts are welcome. Substantive code patches require a prior contributor agreement granting the author the right to incorporate the change under the project license (see below). Open an issue first if you intend to send code.

## License

[CC BY-NC-ND 4.0](LICENSE). This is not a standard open-source license. Practical effects:

* **NonCommercial.** No use for commercial advantage without prior written permission.
* **NoDerivatives.** No distribution of modified versions.
