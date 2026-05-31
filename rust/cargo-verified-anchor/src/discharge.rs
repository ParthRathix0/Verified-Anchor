//! Build the Lean library and check the generated obligations file.
use std::path::{Path, PathBuf};
use std::process::Command;

/// The public repository the pinned Lean proof library is fetched from when not found locally.
const REPO_URL: &str = "https://github.com/ParthRathix0/Verified-Anchor.git";

/// Find the Lean project dir, in order:
/// 1. explicit `--lean-dir`,
/// 2. `$VERIFIED_ANCHOR_LEAN_DIR`,
/// 3. a sibling `lean/` walking up from the current dir (in-repo development),
/// 4. **auto-fetch**: a shallow `git clone` of this crate's version tag into a cache dir, so a
///    `cargo install`-ed tool needs no manual clone and no `--lean-dir`.
pub fn locate_lean_dir(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = explicit { return Ok(p.to_path_buf()); }
    if let Ok(p) = std::env::var("VERIFIED_ANCHOR_LEAN_DIR") { return Ok(PathBuf::from(p)); }
    let mut dir = std::env::current_dir().map_err(|e| e.to_string())?;
    loop {
        let cand = dir.join("lean");
        if cand.join("lakefile.toml").exists() { return Ok(cand); }
        if !dir.pop() { break; }
    }
    fetch_pinned_lean()
}

/// Cache base dir, std-only (no `dirs` crate so the published crate stays dependency-free):
/// `$VERIFIED_ANCHOR_CACHE`, else `$XDG_CACHE_HOME/verified-anchor`, else
/// `$HOME/.cache/verified-anchor`, else a temp dir.
fn cache_base() -> PathBuf {
    if let Ok(p) = std::env::var("VERIFIED_ANCHOR_CACHE") { return PathBuf::from(p); }
    if let Ok(p) = std::env::var("XDG_CACHE_HOME") { return PathBuf::from(p).join("verified-anchor"); }
    if let Ok(home) = std::env::var("HOME") { return PathBuf::from(home).join(".cache").join("verified-anchor"); }
    std::env::temp_dir().join("verified-anchor")
}

/// Shallow-clone the Lean proof library pinned to this crate's version tag (`v<version>`) into
/// the cache and return its `lean/` directory. Idempotent: a populated cache is reused, so the
/// network/git cost is paid only once per version.
fn fetch_pinned_lean() -> Result<PathBuf, String> {
    let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    let repo_dir = cache_base().join(format!("repo-{tag}"));
    let lean_dir = repo_dir.join("lean");
    if lean_dir.join("lakefile.toml").exists() {
        return Ok(lean_dir); // cache hit
    }
    std::fs::create_dir_all(repo_dir.parent().unwrap())
        .map_err(|e| format!("creating cache dir: {e}"))?;
    let _ = std::fs::remove_dir_all(&repo_dir); // clear any partial/failed clone
    eprintln!("verified-anchor: fetching the pinned Lean proof library ({tag}) — one-time, into {repo_dir:?}");
    let out = Command::new("git")
        .args(["clone", "--depth", "1", "--branch", &tag, REPO_URL])
        .arg(&repo_dir)
        .output()
        .map_err(|e| format!("running `git clone` (is git installed?): {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "could not fetch the Lean proof library (tag {tag}):\n{}\n\
             Fixes: ensure git + network are available, or pass `--lean-dir <path>`, or set \
             VERIFIED_ANCHOR_LEAN_DIR to a local checkout of the `lean/` directory.",
            String::from_utf8_lossy(&out.stderr)));
    }
    if !lean_dir.join("lakefile.toml").exists() {
        return Err(format!("fetched {tag} but {lean_dir:?} has no lakefile.toml"));
    }
    Ok(lean_dir)
}

/// `lake build` (cached) then `lake env lean <check_file>`. Returns the lean output on failure.
pub fn discharge(lean_dir: &Path, check_file: &Path) -> Result<(), String> {
    let build = Command::new("lake").arg("build").current_dir(lean_dir).output()
        .map_err(|e| format!("running `lake build` (is elan/lake on PATH?): {e}"))?;
    if !build.status.success() {
        return Err(format!("lake build failed:\n{}", String::from_utf8_lossy(&build.stderr)));
    }
    let chk = Command::new("lake").arg("env").arg("lean").arg(check_file)
        .current_dir(lean_dir).output()
        .map_err(|e| format!("running `lake env lean`: {e}"))?;
    if !chk.status.success() {
        return Err(format!("proof obligations NOT discharged:\n{}{}",
            String::from_utf8_lossy(&chk.stdout), String::from_utf8_lossy(&chk.stderr)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_lean_dir() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // rust/cargo-verified-anchor
        p.pop(); // rust/
        p.pop(); // repo root
        p.push("lean");
        p
    }
    fn lake_available() -> bool {
        Command::new("lake").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    /// The load-bearing property: `discharge` must FAIL when an obligation is false, otherwise
    /// the whole `check` is vacuous. A validation-kind `M4Subset` obligation over a struct with
    /// an `init` constraint is false (`init` is not an M4 constraint), so `by decide` errors.
    #[test]
    fn discharge_rejects_a_false_obligation() {
        if !lake_available() { eprintln!("SKIP: lake not on PATH"); return; }
        let bad = "import VerifiedAnchor\nopen VerifiedAnchor\n\n\
example : M4Subset ({ programId := Pubkey.zero, fields := \
[ { name := \"x\", ty := AccountType.uncheckedAccount, \
constraints := [Constraint.init \"p\" 0 Pubkey.zero] } ] }) := by decide\n";
        let f = std::env::temp_dir().join("va-false-obligation-check.lean");
        std::fs::write(&f, bad).unwrap();
        let r = discharge(&repo_lean_dir(), &f);
        assert!(r.is_err(), "discharge accepted a FALSE obligation — the checker is vacuous");
    }

    /// A populated cache is reused without invoking git/network (the common path after the
    /// one-time fetch). Exercises the auto-fetch resolver's cache-hit branch.
    #[test]
    fn fetch_pinned_lean_reuses_cache_without_cloning() {
        let tmp = std::env::temp_dir().join(format!("va-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
        let lean = tmp.join(format!("repo-{tag}")).join("lean");
        std::fs::create_dir_all(&lean).unwrap();
        std::fs::write(lean.join("lakefile.toml"), "-- test marker\n").unwrap();
        std::env::set_var("VERIFIED_ANCHOR_CACHE", &tmp);
        let got = fetch_pinned_lean().expect("cache hit must succeed without cloning");
        std::env::remove_var("VERIFIED_ANCHOR_CACHE");
        assert_eq!(got, lean);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
