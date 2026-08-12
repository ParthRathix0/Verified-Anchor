## What this changes

<!-- What does this do, and why? Explain the reasoning, not just the diff. -->

## Linked issue

<!-- Closes #123. If there is no issue, say why this did not need discussion first. -->

## Soundness impact

<!--
Does this change what the generated validator ACCEPTS?

- If no: say "No change to accepted contexts."
- If yes: which Lean definition or theorem changed to match? A widened runtime check with no
  matching Lean change means the soundness theorem no longer covers it.
-->

## Gate

Run the gate from [CONTRIBUTING.md](../CONTRIBUTING.md) before requesting review. Tick what you
ran; if you could not run something, leave it unticked and say so — an honest gap is fine, a
silently skipped step is not.

- [ ] `lake build` is clean
- [ ] `grep -rn "sorry\|admit" lean/VerifiedAnchor/` prints nothing
- [ ] `./scripts/audit-axioms.sh` prints `AXIOM AUDIT PASSED`
- [ ] Both SBF `.so` files rebuilt with `cargo-build-sbf --no-rustup-override`
- [ ] `cargo test --workspace` passes (with SBF tools **and** elan on `PATH`)
- [ ] `cargo verified-anchor check` exits 0 for `verified-anchor-example` and `verified-anchor-exploits`
- [ ] I have read [CONTRIBUTING.md](../CONTRIBUTING.md) and signed the [CLA](../CLA.md)

<!-- Anything you could not run: -->
