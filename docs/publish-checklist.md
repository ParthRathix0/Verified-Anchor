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
4. **Push the version tag FIRST.** `cargo-verified-anchor` auto-fetches the Lean proof library
   from the git tag `v<version>` (e.g. `v0.1.1`) on first run, so that tag must exist on the
   public repo before the published tool is used:
   ```bash
   git tag v0.1.1 && git push origin v0.1.1
   ```
5. **Publish in dependency order.** Modern cargo auto-waits for each upload to appear in the
   index before returning, so explicit sleeps are usually unnecessary; if the runtime publish
   can't find the just-published macros, wait ~60 s and re-run it:
   ```bash
   cd rust
   cargo publish -p verified-anchor-macros
   cargo publish -p verified-anchor
   cargo publish -p cargo-verified-anchor
   ```

After-publish housekeeping:

- Verify the crates.io page renders the README correctly (the `readme = "../../README.md"` path means it picks up the workspace-root README).
- Sanity-check the no-clone path: `cargo install cargo-verified-anchor` in a clean dir, then
  `cargo verified-anchor check -p <a crate using verified-anchor>` — the first run should
  fetch the pinned proofs into the cache and discharge.
- Announce.

What is NOT published (but IS reachable):

- `verified-anchor-program`, `verified-anchor-example`, `verified-anchor-exploits` are test fixtures (`publish = false`).
- The Lean source under `lean/` is the proof artefact; it is NOT a Rust crate and does not go to
  crates.io. Instead `cargo-verified-anchor` shallow-clones it from the pinned `v<version>` tag
  into a cache dir on first `check` (override with `--lean-dir` or `VERIFIED_ANCHOR_LEAN_DIR`).

What to verify before tagging v0.2.0 / v0.x.0:

- Lean axioms unchanged (`#print axioms verified_anchor::genValidate_sound` → `[propext, Quot.sound]`).
- `cargo test --workspace` green.
- Both `cargo verified-anchor check -p verified-anchor-example` and `-p verified-anchor-exploits` exit 0.
- Both SBF `.so`s rebuild without `PT_DYNAMIC` warnings.
