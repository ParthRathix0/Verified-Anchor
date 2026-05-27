# Verified Anchor

*Machine-checked correctness for the foundation every Solana program is built on.*

---

## One-Liner

A formally verified subset of Anchor's account validation macros, where each `#[derive(Accounts)]` expansion ships with a Lean 4 proof that the generated validation code satisfies a precisely specified safety contract — giving every Solana program built on it a class of bugs that is provably absent rather than just untested.

---

## One-Paragraph Summary

Anchor is the framework underpinning nearly every Solana program in production. Its `#[derive(Accounts)]` macro expands into the account validation logic that gates almost every instruction — checking signers, ownership, mutability, PDA derivation, and constraint relationships between accounts. That expansion is unverified procedural-macro code. A bug in the macro is a bug in every program that uses it, and historical Anchor-related vulnerabilities show this is not hypothetical. Verified Anchor is a drop-in re-implementation of Anchor's most-used account constraints where each macro expansion is paired with a machine-checked Lean 4 proof that the generated Rust code satisfies a formally specified validation contract. Combined with proof-of-business-logic tooling like QEDGen, this completes a verification chain from `.rs` source down to validator behavior — and unlike approaches that require developers to author proofs, Verified Anchor's guarantees come essentially free with existing `#[derive(Accounts)]` syntax.

---

## 1. The Problem

Solana's correctness depends on a chain of trust:

1. **Validator runtime** — assumed correct (Agave, Firedancer).
2. **sBPF execution** — assumed faithful to source semantics (compiler, BPF VM).
3. **Anchor macro expansion** — assumed to correctly enforce account constraints declared in source. ← **this layer**
4. **Program business logic** — verified per-program (QEDGen, manual review, fuzzing).

Layers 1, 2, and 4 receive substantial attention. Layer 3 does not, and it is where universal assumptions are silently embedded into every program in the ecosystem.

When a developer writes:

```rust
#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut, has_one = authority)]
    pub vault: Account<'info, Vault>,
    pub authority: Signer<'info>,
}
```

…they are trusting that the macro expansion produces validation code that actually enforces:

- `vault` is writable in the runtime sense,
- `vault.authority == authority.key()`,
- `authority` carries a signature,
- the account types deserialize cleanly,
- no account aliases another in ways that bypass these checks.

The macro does produce code. Whether that code correctly enforces those properties — for every combination of constraint annotations, for every account-layout permutation, under all the edge cases that interact with PDA derivation, lifetime annotations, and `Box<Account<...>>` wrapping — is a property currently maintained by tests and reviewer attention. Not by proof.

Historical Anchor-related vulnerabilities (account confusion, missing `has_one` checks elided by macro reordering, lifetime-driven aliasing bugs) all live in this layer. The class of bug is structural: as long as the macro is unverified code, this class cannot be ruled out.

**Verified Anchor's central thesis:** for the constraint subset that covers the dominant use cases, this entire bug class is eliminable by formal proof. The cost is paid once, in the macro implementation. The benefit accrues to every program that uses it.

---

## 2. Approach

Three pillars.

### Pillar 1 — A formal validation contract in Lean 4

Define, in Lean 4, what it means for an account context to satisfy the constraints declared in an Anchor accounts struct. This is the specification side. It does not depend on any code; it is the standalone statement of what Anchor *ought* to enforce.

Concretely: a function `validates : AccountsStruct → AccountInfoVector → Prop` such that `validates spec accounts` holds iff every constraint in `spec` is satisfied by `accounts`. This includes:

- signer requirements,
- ownership checks against expected program IDs,
- writability propagation,
- PDA derivation matching declared seeds and bump,
- `has_one` relational constraints between fields,
- discriminator-based type checks,
- account aliasing prohibitions where required,
- rent and lamport preconditions when declared.

The contract is the artifact a senior reviewer can read and either agree or disagree with. It pins down what verified-Anchor is claiming, in machine-checkable form.

### Pillar 2 — Proof-producing macro expansions

For each supported constraint annotation, re-implement the macro such that expansion produces *two* artifacts:

1. The Rust validation code (drop-in compatible with what stock Anchor produces).
2. A Lean theorem stating that the emitted Rust code, when executed, implements `validates spec accounts` for the specific `spec` that was annotated.

The theorem is then proved — once per constraint kind, parameterized over the user's annotation. The user does not write proofs. The user writes the same `#[derive(Accounts)]` they already write. The proof is checked at build time as a side effect of compilation.

This is where the technical work concentrates. Macro-level reasoning requires bridging Rust's procedural macro system to Lean's logic. The pragmatic approach: macros emit a structured representation of the expansion alongside the Rust code, and a Lean checker validates that representation against the contract. This is conservative — it doesn't fully verify the Rust source-to-MIR pipeline — but it cleanly verifies the only step where Anchor itself is making semantic decisions.

### Pillar 3 — Drop-in cargo integration

Verified Anchor must work as a replacement, not a parallel universe. A developer running `cargo build` on a project using verified-Anchor sees:

- The same `#[derive(Accounts)]` syntax for the supported constraint subset.
- The same generated Rust API (so business logic compiles unchanged).
- An additional build artifact: a Lean proof obligation set, checked by `lake` as part of the build.
- A clear error if a constraint is used that falls outside the verified subset, with guidance on how to either use a stock-Anchor escape hatch or contribute the constraint upstream.

The point of integration is that adoption requires no proof-engineering skill from the user. The skill is concentrated in the library.

---

## 3. Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  User's program: #[derive(Accounts)] struct Foo { ... }     │
└──────────────────┬──────────────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────────────────┐
│  verified-anchor proc-macro                                 │
│  ├── Parses constraint annotations                          │
│  ├── Emits Rust validation code (Anchor-compatible API)     │
│  └── Emits Lean proof obligation: "this expansion           │
│      satisfies the validation contract for these specs"    │
└──────────────────┬──────────────────────────────────────────┘
                   │
       ┌───────────┴──────────────┐
       ▼                          ▼
┌──────────────┐         ┌────────────────────┐
│ cargo build  │         │ lake build         │
│ (Rust)       │         │ (Lean proof check) │
└──────────────┘         └────────────────────┘
                                   │
                                   ▼
                         ┌────────────────────┐
                         │ verified-anchor    │
                         │ Lean library:      │
                         │ ├── Account model  │
                         │ ├── Contract spec  │
                         │ └── Per-constraint │
                         │     soundness      │
                         │     theorems       │
                         └────────────────────┘
```

The Lean library is the durable artifact. The macro is the integration surface. The cargo plugin is the developer experience.

### Composition with existing tooling

Verified Anchor sits one layer below QEDGen. A program using both gets:

- **Verified Anchor**: proof that account validation does what the constraints declare.
- **QEDGen**: proof that the program's business logic, given validated accounts, satisfies its stated invariants.

Composed, these give a verification story for both halves of an instruction handler — the part Anchor generates and the part the developer writes. No other blockchain ecosystem currently offers this combination.

### Composition with the Solana runtime

The validation contract is stated in terms of a Lean model of the Solana account model — the same model QEDGen's `QEDGen.Solana` library exposes. Where verified-Anchor's contract requires reasoning about account fields and constraints, it lifts directly into types that QEDGen and other Lean-based Solana tools already use. The libraries are designed to interoperate; verified-Anchor can be a dependency of QEDGen specs, and vice versa.

---

## 4. Scope

### In scope

The constraint subset covering the dominant usage patterns. Based on a survey of mainnet Anchor programs, this is concentrated in:

- `init` (account initialization with rent payment and program-ownership setup)
- `mut` (writable propagation)
- `has_one` (relational field constraints)
- `seeds` and `bump` (PDA derivation)
- `signer` (signature requirement)
- `owner` (program-ownership check)
- `close` (rent reclamation and discriminator zeroing)

These cover, empirically, the large majority of `#[derive(Accounts)]` usage. The remaining constraint kinds are either niche, addressable through composition of the above, or have manual workarounds.

### Out of scope (for v1)

- Token-22 extension constraints (separate verification effort, likely a follow-on).
- Cross-program reference constraints requiring spec for the called program (composes with QEDGen).
- Custom constraint expressions inside `#[account(constraint = …)]` (these are arbitrary Rust expressions; verification reduces to standard program verification).
- The Anchor runtime's CPI helpers (out of `#[derive(Accounts)]` scope; addressable separately).

The out-of-scope list is explicit because verified-Anchor's value depends on being honest about what it does and doesn't cover. A developer hitting an unsupported constraint should see a clear, actionable error, not a silent gap.

---

## 5. Milestones

Milestones are ordered to maximize the value of partial completion. Each milestone produces an artifact that has standalone worth even if subsequent milestones are not reached. This is deliberate: ambitious formal-methods projects often deliver less than planned, and the value should be front-loaded.

### Milestone 1 — Formal contract for the Anchor account validation model

**Deliverable:** A Lean 4 library defining the account model, the constraint language, and the validation contract `validates : AccountsStruct → AccountInfoVector → Prop` for the in-scope constraint subset.

**Standalone value if no further milestones complete:** A published formal specification of what Anchor's account validation is supposed to mean. This is itself a contribution — there is currently no precise statement of Anchor's account-validation semantics anywhere, and the specification can ground future work by anyone, including stock Anchor's maintainers. Reviewable by senior devs as a Lean file; arguable on its merits independently of any implementation.

### Milestone 2 — Verified expansion for the foundational constraints

**Deliverable:** Proof-producing macro implementations for `mut`, `signer`, and `owner` — the three simplest in-scope constraints. Each expansion ships with a machine-checked Lean proof that the generated Rust validation code implements the contract from Milestone 1 for the relevant constraint subset.

**Standalone value if no further milestones complete:** Demonstrates that the verified-expansion approach works end-to-end on real Anchor-compatible constraints. The proof scaffolding (lemma libraries, tactic infrastructure, Rust-to-Lean bridging code) is reusable for all subsequent constraints. A senior reviewer can compile and run the proof of soundness for a non-trivial macro expansion — that alone is a result worth publishing.

### Milestone 3 — Verified expansion for the relational and lifecycle constraints

**Deliverable:** Verified macro implementations for `has_one`, `init`, and `close`. These are substantially harder than Milestone 2's constraints because they involve relationships between accounts and lifecycle state, not just per-account predicates.

**Standalone value if no further milestones complete:** Coverage of the constraints behind the largest historical Anchor bug class (relational constraint errors). At this point verified-Anchor has demonstrated that the approach scales beyond toy cases to constraints with real semantic content.

### Milestone 4 — Verified PDA derivation

**Deliverable:** Verified expansion for `seeds` and `bump`. PDA misuse is the source of a large fraction of Solana exploits, so verifying that the macro's PDA-derivation code matches the user's declared seeds is high-impact. Requires axiomatizing PDA derivation (treating `find_program_address` as an uninterpreted function with collision-resistance assumptions) and proving the expansion calls it with the right arguments.

**Standalone value if no further milestones complete:** Eliminates one of the largest single bug classes in the Solana ecosystem at the macro level. Programs using verified-Anchor for PDA accounts have a machine-checked guarantee that the validation code matches what the seeds annotation declares.

### Milestone 5 — Cargo integration and developer experience

**Deliverable:** A working cargo integration where projects using verified-anchor compile with `cargo build` and have their proof obligations checked by `lake build` in the same workflow. Clear error reporting when unsupported constraints are used. Migration documentation for stock-Anchor projects.

**Standalone value if no further milestones complete:** Makes the verification accessible. Up to this point the verified-Anchor work is a library that requires manual integration; this milestone makes it a tool a developer can actually use.

### Milestone 6 — Empirical validation against historical exploits

**Deliverable:** A case-study report identifying at least one historical Solana exploit whose root cause was in macro-level account validation, and demonstrating that the verified-Anchor version of the affected program would either fail to compile (because verified-Anchor caught the misuse) or carry a proof that the exploit's preconditions are unreachable.

**Standalone value if no further milestones complete:** Empirical evidence that verified-Anchor catches real bugs. This is the milestone that converts the technical argument into a publishable result.

### Milestone 7 — Release and ecosystem integration

**Deliverable:** Public open-source release. Integration with QEDGen demonstrated on a real program (verified-Anchor handles validation, QEDGen handles business logic, the combined proof is checked end-to-end). Documentation, examples, and the announcement blog post explaining the verification chain to a developer audience.

**Standalone value:** The project, as a thing other people can use and build on. The end of the v1 scope.

---

## 6. Why This Matters

### For senior developers

Formal verification on blockchains has been a research-leaning effort for years. Most of what ships in practice — verified individual proofs of specific protocols — is bespoke and expensive. Verified Anchor inverts the cost structure: the proof effort is paid once, by the library author, and amortized across every program that uses the library. This is the same economics that made verified compilers (CompCert) and verified runtimes (seL4) into landmark projects rather than curiosities.

A senior dev reviewing this proposal will recognize that the central technical bet — that macro-level validation is the right granularity to verify, because it's where reusable safety guarantees can be installed once for the whole ecosystem — is the kind of design choice that determines whether a formal-methods project has impact or remains academic.

### For the Solana ecosystem

Anchor is universal infrastructure. A class of bug eliminated at the Anchor level is a class of bug eliminated across the ecosystem. The verification chain this enables, combined with deductive proof tools like QEDGen, gives Solana a verification story that no other major blockchain currently has.

### For the formal methods community

Macro verification is a recognized hard problem with relatively little applied work outside of meta-programming research. Verified Anchor produces a working, deployed instance of proof-producing macro expansion in a high-stakes domain. That is a publishable contribution to formal methods literature independently of its ecosystem value.

---

## 7. Risks and Honest Limitations

**Macro verification is genuinely hard.** Bridging Rust's procedural macros to Lean is not a solved problem. The mitigation is the milestone structure: Milestone 1 produces value even if no macros get verified. Milestone 2 demonstrates feasibility on simple constraints before tackling harder ones. Each milestone is independently deliverable.

**The verified subset will be a strict subset of stock Anchor.** Adoption depends on the subset covering enough of real usage. The constraint selection above is informed by mainnet program surveys, but the empirical adoption story is unproven until Milestone 7.

**Proofs cover the macro expansion, not the Rust compiler.** A bug in `rustc` codegen could still cause the deployed sBPF to behave differently from the verified source-level expansion. This is a real limitation, shared with every source-level formal verification effort on every language. The chain of trust is improved, not closed.

**The Lean ecosystem for program verification is maturing, not mature.** Tactic libraries, automation, and ergonomics are all improving but uneven. Some proofs that ought to be easy will be tedious; some that ought to be hard may be tractable. The milestone structure assumes proof effort scales reasonably; if it doesn't, scope can be trimmed without invalidating the prior milestones' artifacts.

**This work composes with QEDGen but is not dependent on it.** If QEDGen's trajectory changes, verified-Anchor stands alone as a library. The reverse is also true. This is a deliberate architectural choice — neither project should be a single point of failure for the other.

---

## 8. Tech Stack

**Lean 4** for the contract specification, the per-constraint soundness theorems, and the proof infrastructure. Chosen for: active program-verification ecosystem, strong metaprogramming, alignment with existing Solana formal-methods work (QEDGen, the `lean_solana` library).

**Rust** for the procedural macro implementation and the cargo integration. The macros emit both Anchor-compatible Rust and structured representations consumed by the Lean proof side.

**Lake** (Lean's build system) and **cargo** (Rust's) integrated so a single user-facing build flow produces both the Rust artifact and the proof check.

**Anchor compatibility layer** for the public API surface, ensuring user programs need no business-logic changes when migrating.

**Claude / Claude Code** as the primary development environment for the proof engineering. Inner-loop tactic suggestions, drafting of routine lemmas, scaffolding of constraint expansions. The harder design work — the contract itself, the proof architecture, the macro-to-Lean bridge — is human-led. This is the right division of labor for formal verification work, where LLMs are useful collaborators on syntax-heavy tasks but cannot validate soundness.

**QEDGen as a downstream dependency** in the demonstration phase. Showing the two libraries composing on a real program is a Milestone 7 deliverable; the development of verified-Anchor itself does not depend on QEDGen.

---

## 9. What This Is Not

To preempt misreadings:

This is not an audit tool. It does not analyze existing programs for bugs. It provides a verified library that, when used, eliminates a class of bug by construction.

This is not a static analyzer or a linter. The guarantees are deductive, not heuristic. A program that compiles against verified-Anchor has a machine-checked proof of the validation contract, not a probabilistic confidence rating.

This is not a replacement for QEDGen or any business-logic verification tool. It verifies one specific layer — account validation — and explicitly composes with tools that verify other layers.

This is not a research prototype. The deliverable target is production-usable for the supported constraint subset. Research outputs (the formal contract, the macro verification methodology) are byproducts of building production infrastructure, not the primary goal.

---

## 10. Summary

Verified Anchor places a machine-checked correctness guarantee at the layer of the Solana stack where it has the most leverage: the macro that generates the account validation logic for nearly every program in the ecosystem. The work is technically deep (formal contract specification in Lean 4, proof-producing macro expansion, Rust-to-Lean bridging), ecosystem-relevant (every Anchor program in production is a potential beneficiary), composable with existing tooling (sits cleanly under QEDGen's business-logic proofs), and structured as a series of independently valuable milestones rather than a monolithic deliverable.

The end state is a Solana ecosystem in which a developer can write idiomatic Anchor code and, as a free side effect of building, ship a program with a machine-checked proof that its account validation does what its annotations declare. The bug class addressed is universal, the verification chain it enables is one no other major blockchain currently offers, and the artifacts produced — the formal contract, the proof methodology, the verified macros, the cargo integration — each contribute independently to the state of practice.
