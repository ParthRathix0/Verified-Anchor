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
