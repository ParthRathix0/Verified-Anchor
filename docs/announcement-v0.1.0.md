# Verified Anchor — formally verified account validation for Solana programs

Verified Anchor is a drop-in replacement for Anchor's `#[derive(Accounts)]` — the macro that gates almost every Solana transaction in production — where every expansion comes with a Lean 4 proof that the generated Rust code satisfies a formally specified validation contract over the Solana account model. Syntax is signature-identical to stock Anchor (`Account<'info, T>`, `Signer<'info>`, `Program<'info, P>`, `Context<'a, 'b, 'c, 'info, T>`); the only thing that changes is that the build now refuses to succeed if the framework can't prove the validation. The Lean library models concrete Solana primitives (real `findProgramAddress` PDA derivation, lamports, rent, owner/executable flags) under a `VerifiedAnchor.Solana` namespace designed to compose with the broader Solana formal-methods stack (`QEDGen.Solana`, `lean_solana`). The proven core covers signer / mut / owner / has\_one / seeds + bump / discriminator constraints — the M4 subset — and the init / close lifecycle. Four CVE-class account-validation bugs from real Solana mainnet incidents (Cashio, Crema Finance, type confusion, PDA seeds misuse) are reproduced as on-chain before/after tests; the verified versions reject the attacker on chain.

## The problem

Anchor is the framework underpinning nearly every Solana program in production. Its `#[derive(Accounts)]` macro expands into the account-validation logic that gates almost every instruction — checking signers, ownership, mutability, PDA derivation, and constraint relationships between accounts. That expansion is unverified procedural-macro code. A bug in the macro is a bug in every program that uses it.

This isn't a hypothetical class of failure. Four well-known incidents map cleanly onto macro-level account-validation flaws:

- **Cashio (March 2022, ~$48M).** A `has_one`-style constraint on a typed account was missing; an attacker passed a forged collateral account whose stored bank pubkey pointed at an attacker-controlled mint.
- **Account type confusion (multiple incidents).** Two account types owned by the same program look identical at the owner-check level. Only the 8-byte discriminator distinguishes them; programs that skipped the discriminator check could be tricked into treating one type as another.
- **Crema Finance (July 2022, ~$8.8M).** A price account whose `owner` should have been the AMM program was loaded without an owner check; the attacker passed an account they owned and forged the price.
- **PDA seeds misuse.** Programs that fail to check that an account *is* the canonical PDA for a given seed set let attackers pass arbitrary accounts in the PDA slot.

Existing defences are auditing, fuzzing, and manual review. None of them eliminate the class at the framework level. Verified Anchor does.

## How it works

### What you write

Verified-anchor user code is, by design, byte-for-byte stock Anchor:

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
    let amount_to_send = ctx.vault.amount;
    // ... your handler logic
    Ok(())
}
```

`#[account]` bundles `BorshSerialize + BorshDeserialize + AccountData` (the discriminator is `sha256("account:" + Name)[..8]`, matching real Anchor's wire format). `#[derive(VerifiedAccounts)]` emits both an `impl Validate` (the proven gate) and an `impl Accounts<'info>` whose `try_accounts` calls `validate` first, then Borsh-deserialises the typed payload. Seeded structs additionally emit a `<Name>Bumps` struct with one `pub <field>: u8` per PDA, populated with the canonical bump from `find_program_address`.

### What gets proven

For every struct `s` in the M4 subset, the Lean theorem `genValidate_sound` reads:

> `M4Subset s → (genValidate s c = true ↔ validates s c)`

In plain English: the generated Rust `validate` returns `Ok(())` exactly when the Lean contract says the supplied accounts satisfy the struct's constraints. The two sides are observably equivalent. The lifecycle theorems (`init_establishes_post` / `close_establishes_post`) discharge analogous Hoare obligations for `init` and `close`.

Both headline theorems depend only on `[propext, Quot.sound]` — Lean's standard propositional-extensionality and quotient-soundness axioms. No `sorry`, no `Classical.choice`, no `native_decide`. The axiom set is auditable in one command:

```
lake env lean -c '#print axioms VerifiedAnchor.genValidate_sound'
```

Per-program obligations are discharged by `cargo verified-anchor check` via a single Lean tactic (`decide M4Subset`); there is no per-struct proof effort.

### What isn't proven

Honest boundaries — read `docs/verified-anchor-bridge.md` for the full version:

- **Borsh deserialisation.** The `Account<'info, T>` payload decoder runs after `validate` succeeds. A `BorshFailed` error is a transcription concern, not a verification hole.
- **CPI effects beyond `init` and `close`.** Verified-anchor's lifecycle proofs cover account creation and account close. Other CPIs — token transfers, custom program calls — are outside the proven surface.
- **The M4 subset.** Stock Anchor has constraint kinds we don't model: `realloc`, `zero`, token / mint / associated-token, etc. The macro emits a `compile_error!` for any unsupported constraint pointing at the migration guide.
- **The Solana runtime contract.** We trust the runtime to enforce account ownership, signer flags, and writable flags as described.

The claim is not "your Solana program is now bug-free." The claim is: the class of macro-level account-validation bug — the class that ate Cashio, Crema, the type-confusion incidents — is eliminated at the framework level.

### Under the hood

The proof-producing macro emits two artefacts per derived struct: the Rust `impl Validate` and a Lean `AccountsStruct` literal describing the same constraints. `genValidate` in Lean is a function that takes an `AccountsStruct` and produces a Lean function observably equivalent to the Rust one; `genValidate_sound` proves that function equals the contract. Because the same `AccountsStruct` literal seeds both sides, the Rust validator and the Lean validator can be checked equal by `decide` — uniformly, for any struct in the M4 subset.

The Solana primitives are modelled concretely in Lean (real PDA derivation, real seed semantics); only `sha256` and `isOnCurve` are opaque axioms. The PDA derivation theorem is proved over the concrete `findProgramAddress` with no extra axioms beyond those two.

## The empirical part

Talk is cheap; the four incidents above are reproduced in the M6 exploit suite as litesvm before/after, in `rust/verified-anchor-exploits/`. Each scenario ships a `naive_<scenario>` instruction (no validation — what the original bug looked like) and a `verified_<scenario>` instruction (using `#[derive(VerifiedAccounts)]`). The test asserts three things:

1. **Naïve + attacker accounts → `Ok` with observable bad effect on-chain.** The attacker wins against the unverified code path.
2. **Verified + attacker accounts → on-chain `Err`.** Same attack, same accounts, the verified path rejects.
3. **Verified + legit accounts → `Ok` with the correct effect.** No false negatives.

Concretely: the Cashio scenario reads `collateral.amount` at offset 40 and credits it to an output account. The attacker passes a forged `Collateral`-shaped account whose `bank` field points at attacker-controlled mint; naïve credits the inflated amount, verified rejects because `has_one = bank` fails. Same shape for the other three.

Full case studies (with the original incident timelines, the attack mechanics, and the bridge from CVE to verified-anchor constraint) are in `docs/exploit-case-studies.md`.

## Getting started

```bash
cargo add verified-anchor
cargo add verified-anchor-macros
cargo install cargo-verified-anchor
```

Then in your program:

```rust
use verified_anchor::prelude::*;
```

The migration guide at `docs/migrating-from-anchor.md` has a side-by-side syntax mapping; for most Anchor codebases it's a near-1:1 swap. The supported constraint set is signer / mut / owner / has\_one / init / payer / space / close / seeds / bump / discriminator. Anything else is a `compile_error!` with a pointer at the docs.

Per-struct proof obligations are discharged off the hot path:

```bash
cargo verified-anchor check -p my-crate --lean-dir <path-to-lean-source>
```

This runs `lake env lean` against a generated `decide M4Subset` proof. `cargo build` stays Lean-free.

## Roadmap

- **v0.1.0 (this release):** M1–M7 complete. Lean contract, proof-producing macros, PDA derivation, cargo integration, M4 subset proven sound, empirical M6 case studies, typed-wrapper API, `#[account]` attribute, per-seed `Bumps`, dual-package release.
- **Deferred:** QEDGen composition demo (gated on QEDGen availability); widening the verified constraint subset (realloc, token, zero-copy), `AccountLoader<T>`, `Sysvar<T>`, IDE/LSP surfacing of unmet obligations.

Source: <https://github.com/ParthRathix0/Verified-Anchor>. Issues and audit attempts welcome; substantive patches via prior agreement (see the repo README for license details — CC BY-NC-ND 4.0).
