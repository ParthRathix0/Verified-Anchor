# Verified Anchor

Formally verified (Lean 4) account validation for Solana programs — Anchor-compatible, proof-producing.

**Status:** `0.1.0` alpha. M1–M7a complete. Lean theorems depend only on `[propext, Quot.sound]`.

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

- **M7b (this release):** `#[account]` attribute, per-seed `Bumps`, crates.io packaging.
- **M7c (next):** QEDGen integration + announcement.

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
