# Verified Anchor — the Rust↔Lean bridge (Milestone 2)

How the generated Rust validator relates to the machine-checked Lean proof, and exactly
what is and isn't proven.

## Clause-by-clause correspondence

| Generated Rust (`validate`) | Lean model (`genConstraint`) | Discharges M1 contract case |
|---|---|---|
| `if !accounts[i].is_signer { Err(MissingSigner) }` | `genSigner a := a.isSigner` | `satisfies … .signer` |
| `if !accounts[i].is_writable { Err(NotWritable) }` | `genMut a := a.isWritable` | `satisfies … .mut` |
| `if accounts[i].owner != &expected { Err(WrongOwner) }` | `genOwner e a := decide (a.owner = e)` | `satisfies … (.owner e)` |
| `if accounts.len() < n { Err(NotEnoughAccounts) }` | `decide (c.length = s.fields.length)` | `WellFormed` |

The generated `validate` has signature `fn validate(accounts: &[AccountInfo]) -> Result<(), VAError>`
(an associated method of the `Validate` trait — no `&self`; the struct is a compile-time
spec carrier and validation is positional over the runtime account slice, index = field
declaration order, matching the Lean `Ctx`). Per field it checks every constraint in order
and short-circuits on the first failure; `genFieldValidate` folds `genConstraint` with `&&`
over `impliedConstraints ++ constraints`. `genValidate` conjoins well-formedness with all
fields.

## What is proven

`theorem genValidate_sound (s c) (h : M2Subset s) : genValidate s c = true ↔ validates s c`
— the Lean model of the generated validator agrees with the Milestone-1 contract for every
struct in the M2 subset (`M2Subset`: each field's type ∈ {signer, unchecked, system,
program} and each explicit constraint ∈ {mut, signer, owner}). Proved once, parameterized
over the user's annotation. `#print axioms` reports `[propext, Quot.sound]` only — no
`sorryAx`, no `Classical.choice`. Three per-constraint lemmas
(`genConstraint_{signer,mut,owner}_iff`) connect each `gen*` to the corresponding M1
`satisfies` case.

`M2Subset` deliberately excludes the typed `Account<T>` (`AccountType.account`), because that
type implies a `discriminator` constraint outside the M2 subset — that arrives in Milestone 3.

## What is transcription (documented + tested, not proven)

The Rust `validate` body is a clause-by-clause transcription of `genValidate` per the table
above. This correspondence is NOT machine-checked across the language boundary; it is backed
by shared accept/reject test vectors run in both `rust/verified-anchor/tests/behavior.rs` and
the Lean `#guard`s in `Codegen/ExampleGenerated.lean`.

**Known permissiveness difference (account count).** The generated Rust guards with
`accounts.len() < n`, so it *accepts surplus accounts* (it only inspects the declared prefix
`0..n`). The Lean model `genValidate` — and the M1 contract `WellFormed` it is proven equal
to — instead require an *exact* count (`c.length = s.fields.length`). So on a slice with more
accounts than the struct declares, Rust returns `Ok` while the contract/model would reject.
This is a transcription difference, not a soundness defect: `genValidate_sound` relates the
model to the contract (both exact), and the Rust is strictly *more* permissive only on the
surplus dimension. The Rust behavior is pinned by the `accepts_surplus_accounts` test; a
future milestone can tighten the generated guard to `!= n` if exact-count parity is desired.

## What is out of scope

rustc/LLVM/sBPF code generation fidelity — that the compiled binary faithfully executes the
Rust source. This is the standard boundary of source-level verification (cf. CompCert), and
is not addressed by Verified Anchor at any milestone.
```
