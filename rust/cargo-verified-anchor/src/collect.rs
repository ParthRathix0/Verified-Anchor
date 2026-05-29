//! Run the target crate's `emit_specs!()` test and read the resulting spec files.
use crate::generate::{Kind, Spec};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run `cargo test --lib __verified_anchor_emit_specs` with VERIFIED_ANCHOR_SPEC_DIR set,
/// then read every `<name>.{validation,lifecycle}` file written into `spec_dir`.
pub fn collect(crate_name: Option<&str>, spec_dir: &Path) -> Result<Vec<Spec>, String> {
    let _ = std::fs::remove_dir_all(spec_dir);
    std::fs::create_dir_all(spec_dir).map_err(|e| format!("mkdir {spec_dir:?}: {e}"))?;

    let mut cmd = Command::new("cargo");
    cmd.arg("test");
    if let Some(c) = crate_name { cmd.args(["-p", c]); }
    cmd.args(["--lib", "__verified_anchor_emit_specs"]);
    cmd.env("VERIFIED_ANCHOR_SPEC_DIR", spec_dir);
    let out = cmd.output().map_err(|e| format!("running cargo test: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo test (spec emitter) failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }

    let mut specs = Vec::new();
    for entry in std::fs::read_dir(spec_dir).map_err(|e| format!("read {spec_dir:?}: {e}"))? {
        let path: PathBuf = entry.map_err(|e| e.to_string())?.path();
        let kind = match path.extension().and_then(|s| s.to_str()) {
            Some("validation") => Kind::Validation,
            Some("lifecycle") => Kind::Lifecycle,
            _ => continue,
        };
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string();
        let lean_spec = std::fs::read_to_string(&path).map_err(|e| format!("read {path:?}: {e}"))?;
        specs.push(Spec { name, kind, lean_spec });
    }
    Ok(specs)
}
