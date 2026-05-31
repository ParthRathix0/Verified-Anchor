# Verified Anchor — formally verified account validation for Solana programs

Verified Anchor is a drop-in replacement for Anchor's `#[derive(Accounts)]` — the macro that
gates almost every Solana transaction in production — where every expansion ships with a
Lean 4 proof that the generated Rust code satisfies a formally specified validation contract
over the Solana account model. The user-facing syntax is identical to stock Anchor
(`Account<'info, T>`, `Signer<'info>`, `Program<'info, P>`, `Context<'a, 'b, 'c, 'info, T>`);
the only thing that changes is that the build refuses to succeed if the framework cannot
prove the validation. The Lean library models concrete Solana primitives — the real
`findProgramAddress` PDA derivation, lamports, rent, owner and executable flags — under a
`VerifiedAnchor.Solana` namespace designed to compose with the broader Solana formal-methods
stack (`QEDGen.Solana`, `lean_solana`). The proven core covers `signer`, `mut`, `owner`,
`has_one`, `seeds` + `bump`, and `discriminator` constraints, the typed-wrapper base checks
(`SystemAccount` ownership, `Program<P>` executable + address), plus the `init` and `close`
lifecycle. Four real Solana mainnet incidents — Cashio, Crema Finance, account type
confusion, and PDA seeds misuse — are reproduced in this repository as on-chain
before/after tests; the verified versions reject the attacker on chain.

## The problem

Anchor is the framework underpinning nearly every Solana program in production. Its
`#[derive(Accounts)]` macro expands into the account-validation logic that gates almost every
instruction: checking signers, ownership, mutability, PDA derivation, and constraint
relationships between accounts. That expansion is unverified procedural-macro code. A bug in
the macro is a bug in every Solana program that uses it.

This is not a hypothetical class of failure. Four well-known incidents map cleanly onto
macro-level account-validation flaws:

- **Cashio (March 2022, ~$48M).** A `has_one`-style constraint on a typed account was
  missing; an attacker passed a forged collateral account whose stored bank pubkey pointed
  at an attacker-controlled mint.
- **Account type confusion (multiple incidents).** Two account types owned by the same
  program look identical at the owner-check level. Only the eight-byte discriminator
  distinguishes them. Programs that skipped the discriminator check could be tricked into
  treating one type as another.
- **Crema Finance (July 2022, ~$8.8M).** A price account whose `owner` should have been the
  AMM program was loaded without an owner check; the attacker passed an account they owned
  and forged the price.
- **PDA seeds misuse.** Programs that fail to check that an account *is* the canonical PDA
  for a given seed set let attackers pass arbitrary accounts in the PDA slot.

The existing defences are auditing, fuzzing, and manual review. None of them eliminate the
class at the framework level. Verified Anchor does.

## How it works

### Developer surface

Verified Anchor user code is byte-for-byte identical to stock Anchor:

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
    // your handler logic
    Ok(())
}
```

`#[account]` bundles `BorshSerialize + BorshDeserialize + AccountData`. The discriminator is
`sha256("account:" + Name)[..8]`, matching stock Anchor's wire format. `#[derive(VerifiedAccounts)]`
emits both an `impl Validate` (the proven gate) and an `impl Accounts<'info>` whose
`try_accounts` calls `validate` first and then Borsh-deserialises the typed payload. Seeded
structs additionally emit a `<Name>Bumps` struct with one `pub <field>: u8` per PDA,
populated with the canonical bump returned by `find_program_address`.

### What is proven

For every struct `s` in the supported subset, the Lean theorem `genValidate_sound` reads:

> `M4Subset s → (genValidate s c = true ↔ validates s c)`

The generated Rust `validate` returns `Ok(())` exactly when the Lean contract says the
supplied accounts satisfy the struct's constraints. The two sides are observably equivalent.
The lifecycle theorems `init_establishes_post` and `close_establishes_post` discharge
analogous Hoare obligations for `init` and `close`.

Both headline theorems depend only on `[propext, Quot.sound]` — Lean's standard
propositional-extensionality and quotient-soundness axioms. No `sorry`, no `Classical.choice`,
no `native_decide`. The axiom set is auditable in one command:

```bash
cd lean && lake env lean Audit.lean   # prints genValidate_sound + lifecycle_sound axioms
```

Per-program obligations are discharged by `cargo verified-anchor check` via a single Lean
tactic (`decide M4Subset`). There is no per-struct proof effort.

### What is not proven

The trust boundary, in full detail, is documented in
[`verified-anchor-bridge.md`](verified-anchor-bridge.md). Summary:

- **Borsh deserialisation.** The `Account<'info, T>` payload decoder runs after `validate`
  succeeds. A `BorshFailed` error is a transcription concern, not a verification hole.
- **CPI effects beyond `init` and `close`.** Verified Anchor's lifecycle proofs cover account
  creation and account close. Other CPIs — token transfers, custom program calls — are
  outside the proven surface.
- **Constraints outside the supported subset.** Stock Anchor has constraint kinds the
  framework does not model: `realloc`, `zero`, token / mint / associated-token, and custom
  `constraint = expr`. The macro emits a `compile_error!` for any unsupported constraint,
  pointing at the migration guide.
- **The Solana runtime contract.** The library trusts the runtime to enforce account
  ownership, signer flags, and writable flags as documented.

The library's claim is not that Solana programs become bug-free. The claim is that the
macro-level account-validation bug class — the class that ate Cashio, Crema, and the
type-confusion incidents — is eliminated at the framework level.

### Under the hood

The proof-producing macro emits two artefacts per derived struct: the Rust `impl Validate`
and a Lean `AccountsStruct` literal describing the same constraints. `genValidate` in Lean is
a function that takes an `AccountsStruct` and produces a Lean Boolean function observably
equivalent to the Rust one. `genValidate_sound` proves that function equals the contract.
Because the same `AccountsStruct` literal seeds both sides, the Rust validator and the Lean
validator can be checked equal by `decide`, uniformly, for any struct in the supported
subset.

The Solana primitives are modelled concretely in Lean: real PDA derivation, real seed
semantics. Only `sha256` and `isOnCurve` are opaque axioms. The PDA derivation theorem is
proved over the concrete `findProgramAddress` with no extra axioms beyond those two.

## Empirical validation

The four incidents above are reproduced as litesvm before/after tests in
`rust/verified-anchor-exploits/`. Each scenario ships a `naive_<scenario>` instruction (no
validation — what the original bug looked like) and a `verified_<scenario>` instruction
(using `#[derive(VerifiedAccounts)]`). The test asserts three things:

1. Naive instruction with attacker accounts returns `Ok` with an observable bad effect on
   chain. The attacker wins against the unverified code path.
2. Verified instruction with the same attacker accounts returns on-chain `Err`. The
   verified path rejects.
3. Verified instruction with legitimate accounts returns `Ok` with the correct effect. No
   false negatives.

Concretely, the Cashio scenario reads `collateral.amount` at offset 40 and credits it to an
output account. The attacker passes a forged `Collateral`-shaped account whose `bank` field
points at an attacker-controlled mint. The naive version credits the inflated amount; the
verified version rejects because `has_one = bank` fails. The same shape applies to the
other three scenarios.

Full case studies — original incident timelines, attack mechanics, and the connection from
CVE to the corresponding verified-anchor constraint — live in
[`exploit-case-studies.md`](exploit-case-studies.md).

## Getting started

```bash
cargo add verified-anchor
cargo add verified-anchor-macros
cargo install cargo-verified-anchor
```

Then in your Solana program:

```rust
use verified_anchor::prelude::*;
```

The migration guide at [`migrating-from-anchor.md`](migrating-from-anchor.md) has a
side-by-side syntax mapping; for most Anchor codebases it is a near-1:1 swap. The supported
constraint set is `signer` / `mut` / `owner` / `has_one` / `init` / `payer` / `space` /
`close` / `seeds` / `bump` / `discriminator`. Anything else is a `compile_error!` with a
pointer back to the docs.

Per-struct proof obligations are discharged off the hot path:

```bash
cargo verified-anchor check -p my-crate --lean-dir <path-to-lean-source>
```

This runs `lake env lean` against a generated `decide M4Subset` proof. `cargo build`
stays Lean-free.

## Roadmap

- **v0.1.0 (this release).** Lean contract, proof-producing macros, PDA derivation, cargo
  integration, the supported subset proven sound, empirical case studies of four real Solana
  mainnet incidents, typed-wrapper API matching stock Anchor, `#[account]` attribute,
  per-seed `Bumps`, packaged for crates.io.
- **Deferred.** QEDGen composition demo (gated on QEDGen availability); widening the
  verified constraint subset (`realloc`, token, zero-copy); `AccountLoader<T>`; `Sysvar<T>`;
  IDE / LSP surfacing of unmet proof obligations.

Source: <https://github.com/ParthRathix0/Verified-Anchor>. Issues and audit attempts are
welcome; substantive patches require prior agreement under the project licence
(CC BY-NC-ND 4.0; see the repository README).
