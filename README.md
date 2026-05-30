# Verified Anchor

> Formally verified account validation for **Solana** programs.
> A drop-in replacement for `#[derive(Accounts)]` where every macro expansion ships with a Lean&nbsp;4 proof that the generated check matches a formally specified contract — built specifically for the Solana account model (PDAs, sysvars, SBF runtime).

**Status:** `v0.1.0`. Lean theorems' axioms: `[propext, Quot.sound]` only.

## What this is (plain English)

Verified Anchor is a safer version of **Anchor — the framework that almost every Solana program in production uses** to define which accounts an instruction is allowed to touch.

When you write a Solana program in Anchor, you describe the accounts in a struct like "this account must be a signer, this one must be writable, this one must be owned by the program." The Anchor framework turns that struct into checking code at compile time, and your program runs those checks on every Solana transaction — before any business logic, before any lamport movement, before anything.

The problem: **that check-generating code itself has never been formally verified.** A bug in Anchor's macro is a bug in *every* Solana program that uses Anchor — and the macro is hundreds of lines of metaprogramming that nobody has machine-checked end to end.

Verified Anchor is a drop-in replacement where every check-generating macro comes with a Lean 4 proof that the generated code does what the struct said it should. You write the same Solana program code you'd write in stock Anchor. The proof is checked at build time. If it doesn't check, your program doesn't compile.

## Why this is Solana-specific (and not generic)

Verified Anchor only exists for, and only makes sense for, Solana. Concretely:

- **Anchor is Solana-exclusive.** It was built at Solana Labs as the de facto framework for Solana programs. No other blockchain uses Anchor. There is no "Anchor for EVM" or portable variant.
- **The Lean library models the Solana account model directly.** `lean/VerifiedAnchor/Solana/` contains a concrete Solana `Pubkey`, `AccountInfo`, the real `findProgramAddress` PDA derivation algorithm, lamports, rent, and the owner/executable flags — not an abstracted blockchain.
- **The verified constraints are Solana-specific constructs.** `seeds = [...], bump` is the Solana PDA contract. `discriminator` is Anchor's 8-byte sha256 type tag (a Solana convention). `init`/`close` move lamports under Solana's rent system. None of these have a meaning outside Solana.
- **The runtime tests run on a real Solana VM.** `rust/verified-anchor/tests/runtime_*.rs` use [litesvm](https://github.com/LiteSVM/litesvm), an in-process Solana validator. The BPF programs in `rust/verified-anchor-program/` and `rust/verified-anchor-exploits/` are compiled with `cargo-build-sbf` to Solana's SBF target.
- **The four CVE case studies are Solana mainnet incidents** — Cashio, Crema Finance, type-confusion exploits, PDA seeds misuse. Each cost real Solana DeFi users real SOL.
- **The verification chain ties into the Solana ecosystem's existing Lean work** (`QEDGen`, `lean_solana`). Verified Anchor's `AccountsStruct` model is designed to compose with QEDGen's business-logic proofs at the Solana program level — see the [proposal](verified_anchor_proposal.md) for the composition story.

Solana's correctness story has historically focused on the validator (Agave, Firedancer) and on per-program business-logic verification (QEDGen). The framework layer — the Anchor macro that gates almost every Solana transaction in production — has been a silent gap. Verified Anchor closes it.

## Why this matters (the problem)

**Solana mainnet programs have lost over $50M to one family of bug** — and that family lives at exactly the layer this project verifies. The bugs are not exotic. They are variations on the same theme: the program *thought* it was checking an account, but the check was either missing, malformed, or trivially bypassable. **Each of these is what Anchor's `#[derive(Accounts)]` is supposed to prevent.** The reason it doesn't is that the framework itself is unverified.

Four examples — all real, all Solana mainnet incidents, all reproduced in this repository as litesvm before-and-after tests under `rust/verified-anchor-exploits/`:

- **Cashio (March 2022, ~$48M lost):** the program forgot to check that an account's `bank` field actually matched the bank account passed in the same instruction. Attacker passed a forged collateral account.
- **Crema Finance (July 2022, ~$8.8M lost):** the program loaded a "price account" without checking who owns it. Attacker passed an account they owned and forged the price.
- **Type confusion (multiple incidents):** two account types owned by the same program look identical at the owner level. Only an 8-byte tag distinguishes them. Programs that skipped the tag check could be tricked into treating one type as another.
- **PDA seeds misuse:** programs that forget to check that an account *is* the canonical PDA for a given seed set let attackers pass any account in the PDA slot.

Each of these is *exactly* the class of bug Anchor's `#[derive(Accounts)]` is supposed to prevent. The reason the framework doesn't prevent them is that the framework's check-generating code is itself unverified.

Verified Anchor eliminates this class at the framework level. A Lean 4 theorem (`genValidate_sound`) says: for any struct in the supported subset, the generated Rust check is observably equivalent to a contract that defines what "valid" means. If the contract says the accounts are valid, the generated check returns `Ok`. If not, it returns `Err`. The two sides cannot disagree.

You can audit the theorem yourself in one line:

```bash
lake env lean -c '#print axioms VerifiedAnchor.genValidate_sound'
# -> [propext, Quot.sound]
```

No `sorry`, no `Classical.choice`, no escape hatches. The same audit applies to the lifecycle (`init`/`close`) theorem.

## Where this fits in the Solana stack

Solana's correctness depends on a chain of trust:

| Layer                         | Verification status                                           |
|-------------------------------|---------------------------------------------------------------|
| 1. Validator runtime          | Agave / Firedancer — assumed correct, audited & battle-tested |
| 2. SBF execution (compiler + VM) | Assumed faithful to source semantics                       |
| 3. **Anchor `#[derive(Accounts)]` macro** | **Silent gap — this project closes it**              |
| 4. Program business logic     | Per-program (QEDGen, audits, fuzzing)                         |

Layers 1, 2, and 4 receive substantial attention from the Solana ecosystem. Layer 3 has not — and it's where the same assumption is silently embedded into every Anchor-based program in production. Anchor is *universal infrastructure*: a class of bug eliminated at the Anchor level is a class eliminated across the Solana ecosystem. That's the leverage point Verified Anchor targets.

Composability with the rest of the Solana formal-methods stack is deliberate. The `lean/VerifiedAnchor/Solana/` namespace mirrors the model `QEDGen.Solana` exposes — Verified Anchor + QEDGen compose into an end-to-end proof chain: Verified Anchor proves account validation does what the constraints declare; QEDGen proves the business logic does what its invariants declare. **No other major blockchain currently offers this combination at the framework level.**

## What you write (the developer surface)

Verified Anchor is signature-identical to stock Anchor:

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
    // ... your handler logic
    Ok(())
}
```

`#[account]` bundles the three derives stock Anchor's attribute generates (`BorshSerialize`, `BorshDeserialize`, `AccountData` — the discriminator is `sha256("account:" + Name)[..8]`, byte-for-byte real Anchor's wire format).

`#[derive(VerifiedAccounts)]` emits **two** things at compile time: the Rust `impl Validate` (the proven gate) and a Lean obligation (the proof artifact). The Rust check runs on every transaction. The Lean obligation is discharged by `cargo verified-anchor check` against the proven library.

For migration: the side-by-side mapping with stock Anchor is in [`docs/migrating-from-anchor.md`](docs/migrating-from-anchor.md).

## How it works (technical)

### The verification chain

```
User's Rust source
      │
      ▼
  #[derive(VerifiedAccounts)] macro expansion
      │
      ├── Rust validate() function          (runs at txn time on chain)
      │
      └── Lean AccountsStruct literal       (a piece of Lean source)
              │
              ▼
          genValidate (Lean function)
              │
              ▼
          M4Subset s → (genValidate s c = true ↔ validates s c)
              │                                    │
              ▼                                    ▼
       generated checker                  the validation contract
       (proven equivalent to             (what "valid accounts" means
        the right-hand side)              as a Lean proposition)
```

The Rust macro and the Lean function `genValidate` consume the *same* `AccountsStruct` literal. The Rust checker is observably equivalent to `genValidate` by construction (same constraint kinds, same iteration order, same comparisons). `genValidate_sound` is the Lean theorem stating `genValidate` is equivalent to the contract. The full chain — generated Rust ↔ `genValidate` ↔ contract — is what makes the user's program correct by construction for the supported constraint subset.

### What's in scope (the M4 subset)

| Constraint              | Proven by                                  |
|-------------------------|--------------------------------------------|
| `signer`                | `genValidate_sound` (M4Subset)             |
| `mut`                   | `genValidate_sound` (M4Subset)             |
| `owner = <expr>`        | `genValidate_sound` (M4Subset)             |
| `has_one = <field>`     | `genValidate_sound` (M4Subset, relational) |
| `seeds = [...], bump`   | `genValidate_sound` (M4Subset, PDA)        |
| `discriminator = "..."` | `genValidate_sound` (M4Subset)             |
| `init`/`close`          | `lifecycle_sound` (Hoare-style)            |

Bumps: seeded structs get a `<Name>Bumps { pub <field>: u8, ... }` populated with the canonical bump returned by `find_program_address`. Matches stock Anchor's `Context.bumps.<field>` shape exactly.

### What's not proven (honest boundaries)

- **Borsh deserialisation.** `Account<'info, T>` decodes the account payload *after* `validate` succeeds. A `BorshFailed` error is an honest "the bytes weren't valid for T," not a proof gap.
- **CPI effects beyond init/close.** The lifecycle theorem covers account creation and closure. Token transfers, custom program calls, anything else: outside the proven surface.
- **Anchor constraints we don't model.** `realloc`, `zero`, token / mint / associated-token, etc. The macro emits a `compile_error!` pointing at the migration guide if you try to use one.
- **The Solana runtime contract.** We trust the runtime to enforce account ownership, signer flags, and writable flags as documented.

The claim isn't "your program is now bug-free." It's "the class of bug that ate Cashio, Crema, and the type-confusion incidents is eliminated at the framework level."

Full discussion: [`docs/verified-anchor-bridge.md`](docs/verified-anchor-bridge.md).

### The empirical part

Four real CVE classes are reproduced in `rust/verified-anchor-exploits/` as litesvm before-and-after tests. Each scenario ships a `naive_<name>` and a `verified_<name>` instruction. The test asserts:

1. Naïve + attacker accounts → `Ok` with bad on-chain effect (attacker wins).
2. Verified + attacker accounts → `Err` (attacker rejected).
3. Verified + legit accounts → `Ok` with correct effect (no false negative).

All four pass. See [`docs/exploit-case-studies.md`](docs/exploit-case-studies.md) for the incident-by-incident analysis.

## Repo layout

```
lean/                       Lean 4 library (`lake build`)
  VerifiedAnchor/Solana/     account model + crypto (opaque sha256/isOnCurve)
  VerifiedAnchor/Constraints/  the Rust↔Lean seam (AST + Ctx)
  VerifiedAnchor/Contract/   validates : AccountsStruct → Ctx → Prop
  VerifiedAnchor/Decision/   validatesBool + agreement
  VerifiedAnchor/Codegen/    genValidate + soundness proofs
  VerifiedAnchor/Examples/   worked example (Withdraw.lean)

rust/                       Cargo workspace
  verified-anchor/           runtime (Validate / Accounts<'info> traits, VAError, prelude)
  verified-anchor-macros/    proc-macros (#[derive(VerifiedAccounts)], #[derive(AccountData)], #[account])
  cargo-verified-anchor/     cargo subcommand: discharges Lean obligations via `lake env lean`
  verified-anchor-program/   BPF program — init/close + a seeds PDA (litesvm fixture)
  verified-anchor-example/   worked user crate (validation + lifecycle)
  verified-anchor-exploits/  empirical exploit suite (4 real CVE classes)

docs/
  verified-anchor-bridge.md     Rust↔Lean correspondence + trust boundary
  migrating-from-anchor.md      migration guide + supported constraint subset
  exploit-case-studies.md       four real bug classes, reproduced on litesvm
  announcement-v0.1.0.md        v0.1.0 release post
  publish-checklist.md          crates.io publish steps

web/index.html                Project landing page (deployable on GitHub Pages)
verified_anchor_proposal.md   Original proposal
```

## Building + verifying

**Lean** (4.30.0 via elan; dep: `batteries`):
```bash
export PATH="$HOME/.elan/bin:$PATH"
cd lean && lake build                       # full build
grep -rn 'sorry\|admit' VerifiedAnchor/     # must be empty
lake env lean -c '#print axioms VerifiedAnchor.genValidate_sound'
lake env lean -c '#print axioms VerifiedAnchor.lifecycle_sound'
# both must read: [propext, Quot.sound]
```

**Rust** (1.93+; native + SBF):
```bash
cd rust && cargo test --workspace
# SBF rebuild (needs solana-cli platform-tools):
export PATH="$HOME/.cache/solana/v1.53/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd rust/verified-anchor-program && cargo-build-sbf --no-rustup-override
cd rust/verified-anchor-exploits && cargo-build-sbf --no-rustup-override
```

**End-to-end proof discharge** (Lean + cargo together):
```bash
cd rust
cargo run -q -p cargo-verified-anchor -- verified-anchor check -p verified-anchor-example  --lean-dir ../lean
cargo run -q -p cargo-verified-anchor -- verified-anchor check -p verified-anchor-exploits --lean-dir ../lean
# both exit 0 after discharging every per-struct M4Subset obligation via lake.
```

## Roadmap

- **v0.1.0 (this release):** M1–M7 complete. Lean contract, proof-producing macros, PDA derivation, cargo integration, empirical exploit suite, drop-in Anchor-compatible typed wrappers, `#[account]` attribute, per-seed `Bumps`, dual-package release.
- **Deferred:** QEDGen composition demo (gated on QEDGen availability); widening the verified constraint subset (`realloc`, token, zero-copy); `AccountLoader<T>`; `Sysvar<T>`; IDE/LSP surfacing of unmet proof obligations.

## License

Licensed under the **Creative Commons Attribution-NonCommercial-NoDerivatives 4.0 International License** (CC BY-NC-ND 4.0). See [`LICENSE`](LICENSE) for the full text.

This is **not a standard open-source license**. Practical effects:

- **NonCommercial.** You may not use the work for commercial advantage or monetary compensation without separate written permission from the author.
- **NoDerivatives.** You may not distribute modified versions of the work.

Contributions are welcome via issues; substantive code patches require a contributor agreement granting the author the right to incorporate the change. Open an issue first if you intend to send code.
