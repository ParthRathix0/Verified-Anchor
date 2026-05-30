# Verified Anchor

Formally verified (Lean 4) account validation for Solana programs — Anchor-compatible, proof-producing.

**Status:** `v0.1.0`. M1–M7 complete. Lean theorems depend only on `[propext, Quot.sound]`.

## Repo layout

```
lean/                       Lean 4 library (lake build); root: VerifiedAnchor.lean
  VerifiedAnchor/Solana/     concrete account model + crypto (opaque sha256/isOnCurve)
  VerifiedAnchor/Constraints/  AST (the Rust↔Lean seam) + Ctx
  VerifiedAnchor/Contract/   satisfies / validates : AccountsStruct → Ctx → Prop
  VerifiedAnchor/Decision/   validatesBool + agreement theorem
  VerifiedAnchor/Codegen/    genValidate + soundness proofs (M4Subset, lifecycle)
  VerifiedAnchor/Examples/   worked example (Withdraw.lean)

rust/                       Cargo workspace
  verified-anchor/           runtime: Validate / Accounts<'info> traits, VAError, prelude
  verified-anchor-macros/    proc-macros: #[derive(VerifiedAccounts)], #[derive(AccountData)], #[account]
  cargo-verified-anchor/     `cargo verified-anchor check` subcommand (discharges obligations via lake)
  verified-anchor-program/   BPF program exercising init/close + a seeds PDA
  verified-anchor-example/   worked user crate (validation + lifecycle)
  verified-anchor-exploits/  empirical exploit-suite (4 real CVE classes, naive vs verified)

docs/
  verified-anchor-bridge.md     Rust↔Lean correspondence + trust boundary (start here)
  migrating-from-anchor.md      migration guide + supported constraint subset
  exploit-case-studies.md       four real bug classes, reproduced on litesvm
  announcement-v0.1.0.md        v0.1.0 release post (technical, audience: Solana devs)
  publish-checklist.md          crates.io publish steps

verified_anchor_proposal.md   original proposal (problem / approach / milestones)
```

For a reviewer: start with `verified_anchor_proposal.md` for context, then
`docs/verified-anchor-bridge.md` for the trust boundary and proof chain, then
`docs/exploit-case-studies.md` for empirical validation. The Lean headline
theorems live at `lean/VerifiedAnchor/Codegen/Soundness.lean`
(`genValidate_sound`) and `lean/VerifiedAnchor/Codegen/StructLifecycle.lean`
(`lifecycle_sound`).

## What it is

Verified Anchor pairs a Lean 4 contract that defines what "valid accounts" means with proof-producing Rust proc-macros that emit Solana validation/lifecycle code whose logic is proven to implement that contract. The Lean side `Codegen.genValidate_sound` theorem reads `M4Subset s → (genValidate s c = true ↔ validates s c)` — the generated validator is observably equivalent to the contract. The Rust side derives an `impl Validate` (the proven gate) and an `impl Accounts<'info>` (the developer surface) that runs `validate` first, then Borsh-deserialises typed account data.

The developer surface is signature-identical to stock Anchor — same `Account<'info, T>`, `Signer<'info>`, `Program<'info, P>`, `Context<'a, 'b, 'c, 'info, T>` shapes — so a verified-anchor instruction handler reads exactly like a stock-Anchor one.

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
    let (ctx_accts, bumps) = Transfer::try_accounts(program_id, accounts, data)?;
    let _ = bumps;
    // ... your handler logic; ctx_accts.vault.amount is the typed payload.
    Ok(())
}
```

Discharge the per-struct proof obligation via Lean:
```bash
cargo verified-anchor check -p my-crate --lean-dir <path-to-lean-source>
```

## Building + verifying

Two toolchains. Lean for the proof side, Rust for the runtime + macros.

**Lean (4.30.0 via elan; dep: `batteries`):**
```bash
export PATH="$HOME/.elan/bin:$PATH"
cd lean && lake build                       # full build; everything must succeed
grep -rn 'sorry\|admit' VerifiedAnchor/     # must be empty
lake env lean -c '#print axioms VerifiedAnchor.genValidate_sound'
lake env lean -c '#print axioms VerifiedAnchor.lifecycle_sound'
# both must read: [propext, Quot.sound]
```

**Rust (1.93+; native + SBF):**
```bash
cd rust && cargo test --workspace            # 12+ test suites
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

## What's proven, what isn't

Read `docs/verified-anchor-bridge.md`. Headline:

- **Proven:** `validates` ↔ `genValidate` ↔ generated Rust per-constraint checks for the M4 subset (signer/mut/owner/has_one/seeds/discriminator + init/close Hoare).
- **Outside the proof:** Borsh deserialisation (`Account<T>` payload decoding), CPI effects beyond init/close, the unspoken Solana runtime contract.
- The bridge doc is honest about every transcription gap.

## Compatibility with stock Anchor

`docs/migrating-from-anchor.md` has the side-by-side syntax mapping. Verified-anchor is field-for-field compatible at the account-validation surface; the bundled `#[account]` attribute matches stock Anchor's wire format (`sha256("account:" + Name)[..8]` discriminator).

## Empirical validation

`docs/exploit-case-studies.md` reproduces four real macro-level account-validation bug classes as litesvm before/after: Cashio (has_one/owner/discriminator), type-confusion (discriminator), Crema (owner), PDA seeds. Each scenario asserts naive(attacker) → bad on-chain effect AND verified(attacker) → on-chain `Err`.

## Roadmap

- **v0.1.0 (this release):** M1–M7 complete. Lean contract, proof-producing
  macros, PDA derivation, cargo integration, empirical exploit suite,
  full drop-in Anchor-compatible typed wrappers, `#[account]` attribute,
  per-seed `Bumps`, dual-package release.
- **Deferred:** QEDGen composition demo (gated on QEDGen availability); widening
  the verified constraint subset (`realloc`, token, zero-copy); `AccountLoader<T>`;
  `Sysvar<T>`; IDE/LSP surfacing of unmet proof obligations.

## License

Licensed under the **Creative Commons Attribution-NonCommercial-NoDerivatives
4.0 International License** (CC BY-NC-ND 4.0). See `LICENSE` for the full text.

This is **not a standard open-source license**. Practical effects:

- **NonCommercial.** You may not use the work for commercial advantage or
  monetary compensation without separate written permission from the author.
- **NoDerivatives.** You may not distribute modified versions of the work.

Contributions are welcome via issues; substantive code patches must be
accompanied by a contributor agreement granting the author the right to
incorporate the change. Open an issue first if you intend to send code.
