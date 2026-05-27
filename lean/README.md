# Verified Anchor — Lean library (Milestone 1)

A formal **validation contract** for Anchor's `#[derive(Accounts)]` account validation,
in Lean 4. This is Milestone 1 of the [Verified Anchor proposal](../verified_anchor_proposal.md):
the standalone, machine-checkable specification of what Anchor's account validation
*ought* to enforce, plus an executable checker proven to agree with it.

## Layout

| Module | Contents |
|--------|----------|
| `VerifiedAnchor/Solana/` | Concrete Solana account world. `Pubkey` (32 bytes), `AccountInfo`, the PDA-derivation algorithm. SHA-256 and the ed25519 on-curve test are `opaque` (axiomatized); everything else is concrete. |
| `VerifiedAnchor/Constraints/` | The constraint AST (`Constraint`, `AccountType`, `AccountField`, `AccountsStruct`) and the runtime `Ctx`. This AST is the intended Rust↔Lean seam for later milestones. |
| `VerifiedAnchor/Contract/` | `satisfies` (per-constraint semantics) and **`validates : AccountsStruct → Ctx → Prop`** — the headline contract. |
| `VerifiedAnchor/Decision/` | `validatesBool` (executable checker) and the proof **`validates_iff_validatesBool`** that it agrees with the declarative contract, with soundness/completeness corollaries. |
| `VerifiedAnchor/Examples/` | The proposal's `Withdraw` struct, with the checker accepting a good context / rejecting a tampered one, the relational `has_one` check, and direct `validates` proofs. |

## Scope (Milestone 1)

In scope: the constraint subset `init, mut, has_one, seeds, bump, signer, owner, close`
(plus account-type-implied `owner`/`discriminator`), the executable checker + agreement
theorem, and worked examples. **No** macro-expansion proofs (Milestone 2+), **no** Rust
crate yet, **no** full Borsh (only Pubkey-field reads).

A note on `opaque sha256`: constraints that hash (`discriminator`, `seeds`) are fully
modeled and decidable, but do not *reduce* under `#eval`/`decide` because `sha256` has no
computational rule. The examples therefore demonstrate the crypto-free constraints
end-to-end and the relational checks per-constraint. A concrete reference `sha256` is a
tracked follow-up (useful for Milestone 6).

## Build

```bash
export PATH="$HOME/.elan/bin:$PATH"   # elan-installed Lean 4.30.0 / Lake 5.0.0
lake build
```

The whole library builds with **zero `sorry`**. The core theorems depend only on
`[propext, Quot.sound]` (verifiable with `#print axioms`).
