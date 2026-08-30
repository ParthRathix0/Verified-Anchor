//! Build the Lean library and check the generated obligations file.
use std::path::{Path, PathBuf};
use std::process::Command;

/// The public repository the pinned Lean proof library is fetched from when not found locally.
const REPO_URL: &str = "https://github.com/ParthRathix0/Verified-Anchor.git";

/// The expected git tree object of `lean/` for the release this crate was cut from.
///
/// WHY THIS EXISTS. `fetch_pinned_lean` clones the tag `v<CARGO_PKG_VERSION>`, and **a git tag is
/// mutable**. Anyone able to move it changes the proof library that every already-installed copy
/// of this tool downloads. That library is the thing that decides whether a user's obligations
/// are discharged, so trusting it unconditionally would make this tool's central claim
/// unfalsifiable: a vacuous `M10Subset` discharges everything and prints a green check.
///
/// A git tree object is CONTENT-ADDRESSED, so pinning it detects a moved tag, a tampered cache,
/// and any edit to `lean/` whatsoever — without adding a dependency (this crate is deliberately
/// std-only) and without hand-rolling a hash function into a security-critical path. `git` is
/// already a hard requirement of the fetch path, so this costs nothing new.
///
/// HOW TO UPDATE (see `docs/publish-checklist.md`): `git rev-parse HEAD:lean`. The value is
/// stable across every commit that does not touch `lean/`, so writing it here, committing,
/// tagging and publishing all keep it valid. There is deliberately no chicken-and-egg with the
/// release commit's own SHA. `expected_lean_tree_matches_this_checkout` fails the build if this
/// constant and `lean/` ever drift apart.
///
/// TRUST NOTE: git tree objects are SHA-1. That is adequate for detecting a moved tag or a
/// poisoned cache, and it is recorded in `docs/verified-anchor-bridge.md` alongside the
/// project's other named trust boundaries rather than left implicit.
const EXPECTED_LEAN_TREE: &str = "9bbddf53758c0d0b8185d56c511353b48bfe2574";

/// Find the Lean project dir, in order:
/// 1. explicit `--lean-dir`,
/// 2. `$VERIFIED_ANCHOR_LEAN_DIR`,
/// 3. a sibling `lean/` walking up from the current dir (in-repo development),
/// 4. **auto-fetch**: a shallow `git clone` of this crate's version tag into a cache dir, so a
///    `cargo install`-ed tool needs no manual clone and no `--lean-dir`.
///
/// Only route 4 is content-verified. Routes 1–3 are directories the user chose and owns; they
/// are the explicit opt-out for anyone who wants to point the tool at their own checkout.
pub fn locate_lean_dir(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Ok(p) = std::env::var("VERIFIED_ANCHOR_LEAN_DIR") {
        return Ok(PathBuf::from(p));
    }
    let mut dir = std::env::current_dir().map_err(|e| e.to_string())?;
    loop {
        let cand = dir.join("lean");
        if cand.join("lakefile.toml").exists() {
            return Ok(cand);
        }
        if !dir.pop() {
            break;
        }
    }
    fetch_pinned_lean()
}

/// Cache base dir, std-only (no `dirs` crate so the published crate stays dependency-free).
///
/// Split from `cache_base` so the refusal path is testable without mutating process-global
/// environment variables underneath concurrently running tests.
fn cache_base_from(
    explicit: Option<String>,
    xdg: Option<String>,
    home: Option<String>,
) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return Ok(PathBuf::from(p));
    }
    if let Some(p) = xdg {
        return Ok(PathBuf::from(p).join("verified-anchor"));
    }
    if let Some(home) = home {
        return Ok(PathBuf::from(home).join(".cache").join("verified-anchor"));
    }
    // DELIBERATELY NOT `std::env::temp_dir()`. On a multi-user box that resolves to
    // `/tmp/verified-anchor`, inside a world-writable directory, and the consequences are worse
    // than a spoiled cache: `discharge` runs `lake build` in there, a lakefile can execute
    // arbitrary code during a build, and the attacker also controls the Lean definitions every
    // obligation is checked against. A security tool must not silently downgrade to that.
    Err(
        "no cache directory available: VERIFIED_ANCHOR_CACHE, XDG_CACHE_HOME and HOME are all \
         unset.\n\
         Refusing to fall back to a world-writable temp directory — on a shared machine another \
         user could pre-create the cache and control the Lean definitions your proof obligations \
         are checked against.\n\
         Fixes: set VERIFIED_ANCHOR_CACHE to a directory you own, or pass `--lean-dir <path>`, \
         or set VERIFIED_ANCHOR_LEAN_DIR to a local checkout of `lean/`."
            .to_string(),
    )
}

fn cache_base() -> Result<PathBuf, String> {
    cache_base_from(
        std::env::var("VERIFIED_ANCHOR_CACHE").ok(),
        std::env::var("XDG_CACHE_HOME").ok(),
        std::env::var("HOME").ok(),
    )
}

/// The git tree object of `lean/` inside a checkout, or an error if `repo_dir` is not a git
/// repository. `--git-dir` is passed explicitly so git cannot walk UP out of `repo_dir` and
/// answer from some unrelated repository that happens to contain it.
fn lean_tree_of(repo_dir: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .arg("--git-dir")
        .arg(repo_dir.join(".git"))
        .args(["rev-parse", "HEAD:lean"])
        .output()
        .map_err(|e| format!("running `git rev-parse` (is git installed?): {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{} is not a verifiable checkout of the Lean proof library: {}",
            repo_dir.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Fail closed unless `repo_dir` carries exactly the `lean/` tree this crate was built against.
fn verify_lean_tree(repo_dir: &Path) -> Result<(), String> {
    let got = lean_tree_of(repo_dir)?;
    if got != EXPECTED_LEAN_TREE {
        return Err(format!(
            "the Lean proof library at {} is NOT the one this tool was built against.\n  \
             expected lean/ tree {EXPECTED_LEAN_TREE}\n  \
             found    lean/ tree {got}\n\
             The version tag may have been moved, or the cache may have been tampered with. \
             Refusing to discharge proof obligations against an unverified proof library.\n\
             Fixes: delete the cache directory and re-run to re-fetch, or pass \
             `--lean-dir <path>` to point at a checkout you trust.",
            repo_dir.display()
        ));
    }
    Ok(())
}

/// Shallow-clone the Lean proof library pinned to this crate's version tag (`v<version>`) into
/// the cache and return its `lean/` directory. Idempotent: a populated cache is reused, so the
/// network/git cost is paid only once per version. Both the freshly cloned tree and the reused
/// cache are content-verified against `EXPECTED_LEAN_TREE`.
fn fetch_pinned_lean() -> Result<PathBuf, String> {
    let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    let repo_dir = cache_base()?.join(format!("repo-{tag}"));
    let lean_dir = repo_dir.join("lean");
    // CACHE HIT. The previous test here was "does a file named lakefile.toml exist", which any
    // local user could satisfy by hand. Verify the content instead of the filename.
    if lean_dir.join("lakefile.toml").exists() {
        verify_lean_tree(&repo_dir)?;
        return Ok(lean_dir);
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
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    // A tag is mutable, so a successful clone proves nothing about WHAT was cloned.
    verify_lean_tree(&repo_dir)?;
    if !lean_dir.join("lakefile.toml").exists() {
        return Err(format!(
            "fetched {tag} but {lean_dir:?} has no lakefile.toml"
        ));
    }
    Ok(lean_dir)
}

/// `lake build` (cached) then `lake env lean <check_file>`. Returns the lean output on failure.
pub fn discharge(lean_dir: &Path, check_file: &Path) -> Result<(), String> {
    let build = Command::new("lake")
        .arg("build")
        .current_dir(lean_dir)
        .output()
        .map_err(|e| format!("running `lake build` (is elan/lake on PATH?): {e}"))?;
    if !build.status.success() {
        return Err(format!(
            "lake build failed:\n{}",
            String::from_utf8_lossy(&build.stderr)
        ));
    }
    let chk = Command::new("lake")
        .arg("env")
        .arg("lean")
        .arg(check_file)
        .current_dir(lean_dir)
        .output()
        .map_err(|e| format!("running `lake env lean`: {e}"))?;
    if !chk.status.success() {
        return Err(format!(
            "proof obligations NOT discharged:\n{}{}",
            String::from_utf8_lossy(&chk.stdout),
            String::from_utf8_lossy(&chk.stderr)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // rust/cargo-verified-anchor
        p.pop(); // rust/
        p.pop(); // repo root
        p
    }
    fn repo_lean_dir() -> PathBuf {
        repo_root().join("lean")
    }
    fn lake_available() -> bool {
        Command::new("lake")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// The load-bearing property: `discharge` must FAIL when an obligation is false, otherwise
    /// the whole `check` is vacuous. A validation-kind `M4Subset` obligation over a struct with
    /// an `init` constraint is false (`init` is not an M4 constraint), so `by decide` errors.
    #[test]
    fn discharge_rejects_a_false_obligation() {
        if !lake_available() {
            eprintln!("SKIP: lake not on PATH");
            return;
        }
        let bad = "import VerifiedAnchor\nopen VerifiedAnchor\n\n\
example : M4Subset ({ programId := Pubkey.zero, fields := \
[ { name := \"x\", ty := AccountType.uncheckedAccount, \
constraints := [Constraint.init \"p\" 0 Pubkey.zero] } ] }) := by decide\n";
        let f = std::env::temp_dir().join("va-false-obligation-check.lean");
        std::fs::write(&f, bad).unwrap();
        let r = discharge(&repo_lean_dir(), &f);
        assert!(
            r.is_err(),
            "discharge accepted a FALSE obligation — the checker is vacuous"
        );
    }

    /// Pins `EXPECTED_LEAN_TREE` to the `lean/` actually in this checkout. This is the release
    /// safety net: change anything under `lean/` without updating the constant and this fails,
    /// which is what stops a release shipping a constant that rejects its own proof library.
    #[test]
    fn expected_lean_tree_matches_this_checkout() {
        let got = match lean_tree_of(&repo_root()) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("SKIP: not a git checkout ({e})");
                return;
            }
        };
        assert_eq!(
            got, EXPECTED_LEAN_TREE,
            "lean/ changed but EXPECTED_LEAN_TREE in discharge.rs was not updated.\n\
             Run `git rev-parse HEAD:lean` and paste the result into the constant."
        );
    }

    /// H2: the cache-hit path used to accept any directory containing a file named
    /// `lakefile.toml`. On a shared machine that is a full compromise — `lake build` runs there
    /// and a lakefile executes arbitrary code, and the planted Lean definitions decide every
    /// obligation. Constructed entirely inside a temp dir; needs no shared machine to reproduce.
    #[test]
    fn poisoned_cache_is_rejected() {
        let tmp = std::env::temp_dir().join(format!("va-poisoned-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
        let lean = tmp.join(format!("repo-{tag}")).join("lean");
        std::fs::create_dir_all(&lean).unwrap();
        // Exactly what an attacker would plant: the filename the old check looked for.
        std::fs::write(lean.join("lakefile.toml"), "-- attacker-controlled\n").unwrap();

        std::env::set_var("VERIFIED_ANCHOR_CACHE", &tmp);
        let got = fetch_pinned_lean();
        std::env::remove_var("VERIFIED_ANCHOR_CACHE");
        let _ = std::fs::remove_dir_all(&tmp);

        let err = got.expect_err("a planted cache directory was ACCEPTED as the proof library");
        assert!(
            err.contains("not a verifiable checkout"),
            "rejected, but not for the right reason: {err}"
        );
    }

    /// A directory that is not a git checkout at all cannot yield a tree, and must not be
    /// silently treated as valid.
    #[test]
    fn non_git_directory_is_not_verifiable() {
        let tmp = std::env::temp_dir().join(format!("va-nongit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let r = lean_tree_of(&tmp);
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(r.is_err(), "a non-git directory was treated as verifiable");
    }

    /// H2: with no cache location available the tool must refuse rather than silently using a
    /// world-writable temp directory. Pure function, so no environment mutation is needed and
    /// this cannot race other tests.
    #[test]
    fn cache_base_refuses_the_world_writable_fallback() {
        let err = cache_base_from(None, None, None)
            .expect_err("fell back to a temp dir instead of refusing");
        assert!(err.contains("world-writable"), "unexpected message: {err}");

        // The legitimate routes still work.
        assert_eq!(
            cache_base_from(Some("/x".into()), None, None).unwrap(),
            PathBuf::from("/x")
        );
        assert_eq!(
            cache_base_from(None, Some("/x".into()), None).unwrap(),
            PathBuf::from("/x/verified-anchor")
        );
        assert_eq!(
            cache_base_from(None, None, Some("/h".into())).unwrap(),
            PathBuf::from("/h/.cache/verified-anchor")
        );
    }

    /// H1, and the sharpest case: a REAL git checkout whose `lean/` is not the pinned one. This
    /// is exactly what a moved version tag looks like from the client side. Without this test
    /// the tree COMPARISON is never exercised — only the cheaper "is this a git repo" gate — and
    /// a verification nobody has watched reject is not known to work.
    #[test]
    fn a_real_checkout_with_the_wrong_lean_tree_is_rejected() {
        let tmp = std::env::temp_dir().join(format!("va-wrongtree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let lean = tmp.join("lean");
        std::fs::create_dir_all(&lean).unwrap();
        std::fs::write(lean.join("lakefile.toml"), "-- attacker's proof library\n").unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&tmp)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.com")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.com")
                .output()
                .expect("git must be installed to run this test")
        };
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        git(&["commit", "-qm", "attacker tree"]);

        let r = verify_lean_tree(&tmp);
        let _ = std::fs::remove_dir_all(&tmp);

        let err = r.expect_err("a real git checkout with a DIFFERENT lean/ tree was ACCEPTED");
        assert!(
            err.contains("NOT the one this tool was built against"),
            "rejected, but not for the right reason: {err}"
        );
    }
}
