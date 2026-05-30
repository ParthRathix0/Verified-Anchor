# Publishing verified-anchor to crates.io

Pre-publish steps (must be done before `cargo publish`):

1. **Confirm `repository` / `homepage` URLs.** All three publishable Cargo.tomls
   already point at <https://github.com/ParthRathix0/Verified-Anchor>. If the
   repo ever moves, update these three files:
   - `rust/verified-anchor/Cargo.toml`
   - `rust/verified-anchor-macros/Cargo.toml`
   - `rust/cargo-verified-anchor/Cargo.toml`
2. **`cargo login`** with a crates.io API token if you haven't already.
3. **Run dry-runs.** Order matters: macros first (everything else depends on it), then the runtime, then the cargo subcommand:
   ```bash
   cd rust
   cargo publish --dry-run -p verified-anchor-macros
   cargo publish --dry-run -p cargo-verified-anchor
   # verified-anchor's dry-run cannot succeed until verified-anchor-macros is
   # actually on crates.io (cargo resolves the dep against the live index even
   # in dry-run). It will succeed at the real-publish step below.
   ```
   The first two dry-runs must succeed before proceeding.
4. **Publish.** Sleep ~60 seconds between publishes so the registry has time to index each crate (without this, the next `cargo publish` can fail because the registry lookup of the newly-uploaded dep hasn't propagated yet):
   ```bash
   cd rust
   cargo publish -p verified-anchor-macros && sleep 60
   cargo publish -p verified-anchor && sleep 60
   cargo publish -p cargo-verified-anchor
   ```

After-publish housekeeping:

- Tag the commit: `git tag v0.1.0 && git push --tags`.
- Verify the crates.io page renders the README correctly (the `readme = "../../README.md"` path means it picks up the workspace-root README).
- Announce.

What is NOT published:

- `verified-anchor-program`, `verified-anchor-example`, `verified-anchor-exploits` are test fixtures (`publish = false`).
- The Lean source under `lean/` is the proof artefact; it is NOT a Rust crate and does not go to crates.io.

What to verify before tagging v0.2.0 / v0.x.0:

- Lean axioms unchanged (`#print axioms verified_anchor::genValidate_sound` → `[propext, Quot.sound]`).
- `cargo test --workspace` green.
- Both `cargo verified-anchor check -p verified-anchor-example` and `-p verified-anchor-exploits` exit 0.
- Both SBF `.so`s rebuild without `PT_DYNAMIC` warnings.
