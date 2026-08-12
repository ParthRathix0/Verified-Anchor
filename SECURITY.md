# Security Policy

Verified Anchor is a safety tool. A flaw in it can silently remove a protection a program's
authors believed they had, so security reports are taken seriously and handled privately.

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.4.x   | Yes       |
| < 0.4.0 | No — upgrade first |

Fixes land on the latest minor. There are no backports to earlier versions.

## What counts as a security issue here

This is broader than "the code crashes", because the product is a *proof*. Any of the following
is a security issue:

1. **The generated validator accepts an account context that the declarative contract rejects.**
   This is a soundness hole, and it is the direction that matters. A program author reads the
   contract and believes they are protected; the generated code disagrees and lets an attacker
   through.

2. **A check runs at runtime with no Lean counterpart.** The project claims every check in the
   proven core is proven. A runtime check that no theorem covers makes that claim false for that
   check, even if the check itself behaves correctly today.

3. **A headline theorem's `#print axioms` reports anything beyond `[propext, Quot.sound]`.**
   Any additional axiom is unverified trust that was not disclosed. Run `./scripts/audit-axioms.sh`.

4. **A `sorry` or `admit` reachable from a headline theorem.** The proof is incomplete and the
   guarantee does not hold.

5. **A named trust boundary diverges from the real behaviour it stands for.** There are exactly
   four: `sha256`, `isOnCurve`, `rentExemptMinimum`, and the Borsh layout model. These are
   modelled opaquely and cross-checked empirically rather than proven. A demonstrated
   discrepancy between one of them and the real Solana or `borsh` behaviour breaks everything
   proven on top of it.

## What is not a security issue

Please open an ordinary issue for these — they are still worth reporting:

* **The validator rejects too much.** A false rejection is a completeness bug. It costs
  usability, not safety, because rejecting more can never let an attacker through.
* **A documented unproven escape hatch behaving as documented.** A `constraint = <expr>` outside
  the proven sublanguage runs verbatim and is reported in `UNPROVEN_CHECKS`. Unproven checks are
  additional conjuncts on the proven core, so they can only reject more.
* **A drop-in gap that surfaces as a build error.** Real Anchor code that will not compile is a
  defect worth fixing — use the drop-in gap issue template — but a build error is loud and
  cannot cause a bad on-chain acceptance.
* **Anything outside account validation.** Instruction logic, arithmetic, and CPI safety are
  explicitly out of scope for the current releases. The tool does not claim to verify them.

## Reporting

**Do not open a public issue for an exploitable finding.**

* **Preferred:** [GitHub Security Advisories](https://github.com/ParthRathix0/Verified-Anchor/security/advisories/new)
  — the `Security` tab → `Report a vulnerability`. This creates a private thread.
* **Fallback:** email <rathiparth931@gmail.com>.

Helpful to include: the affected version, the `#[derive(VerifiedAccounts)]` struct, the account
context that is wrongly accepted or rejected, and the `cargo verified-anchor check` output. A
failing litesvm test is the gold standard, but a clear written description is entirely enough to
start.

## What to expect

| Stage | Timeline |
|---|---|
| Acknowledgement that the report was received | within 5 days |
| Initial assessment — confirmed, not reproducible, or out of scope | within 14 days |
| Fix and coordinated disclosure | agreed with you, based on severity |

Disclosure is coordinated: a fix ships before details are made public. You will be credited in
the advisory and the release notes unless you prefer otherwise.

If you do not hear back within the times above, please chase — a missed notification is far more
likely than disinterest.
