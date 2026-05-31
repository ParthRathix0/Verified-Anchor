//! Build the Lean library and check the generated obligations file.
use std::path::{Path, PathBuf};
use std::process::Command;

/// Find the Lean project dir: explicit `--lean-dir`, else $VERIFIED_ANCHOR_LEAN_DIR, else a
/// sibling `lean/` walking up from the current dir.
pub fn locate_lean_dir(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = explicit { return Ok(p.to_path_buf()); }
    if let Ok(p) = std::env::var("VERIFIED_ANCHOR_LEAN_DIR") { return Ok(PathBuf::from(p)); }
    let mut dir = std::env::current_dir().map_err(|e| e.to_string())?;
    loop {
        let cand = dir.join("lean");
        if cand.join("lakefile.toml").exists() { return Ok(cand); }
        if !dir.pop() { return Err("could not locate the verified-anchor Lean project (pass --lean-dir)".into()); }
    }
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
}
