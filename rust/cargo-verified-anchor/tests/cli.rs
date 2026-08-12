//! End-to-end: run the built `cargo-verified-anchor` binary against verified-anchor-example
//! and assert every obligation is discharged. Gated on the Lean toolchain being present.
use std::path::PathBuf;
use std::process::Command;

fn lean_dir() -> PathBuf {
    // rust/cargo-verified-anchor -> repo root -> lean/
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // rust/
    p.pop(); // repo root
    p.push("lean");
    p
}

fn lake_available() -> bool {
    Command::new("lake")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn check_discharges_example_obligations() {
    if !lake_available() {
        eprintln!("SKIP: lake not on PATH");
        return;
    }
    let bin = env!("CARGO_BIN_EXE_cargo-verified-anchor");
    let out = Command::new(bin)
        .args([
            "verified-anchor",
            "check",
            "-p",
            "verified-anchor-example",
            "--lean-dir",
            lean_dir().to_str().unwrap(),
        ])
        .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap()) // rust/
        .output()
        .expect("run cargo-verified-anchor");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "check failed:\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}"
    );
    assert!(
        stdout.contains("CheckPda (validation)"),
        "missing CheckPda: {stdout}"
    );
    assert!(
        stdout.contains("Lifecycle (lifecycle)"),
        "missing Lifecycle: {stdout}"
    );
    assert!(stdout.contains("discharged"), "missing summary: {stdout}");

    // `CheckAuthority` carries a `constraint = authority.key() == crate::ID`, which lands
    // outside the proven sublanguage (Task 13's escape hatch) — the plain check still exits 0
    // (default: honest inventory, not a failure) but must call it out with a `⚠` line naming
    // the unproven expression, plus the "still run at runtime" guarantee.
    assert!(
        stdout.contains('\u{2713}'),
        "missing proven-check marker: {stdout}"
    );
    assert!(
        stdout.contains('\u{26a0}'),
        "missing unproven-check marker: {stdout}"
    );
    assert!(
        stdout.contains("CheckAuthority"),
        "missing CheckAuthority: {stdout}"
    );
    assert!(
        stdout.contains("authority.key() == crate::ID"),
        "missing unproven expr: {stdout}"
    );
    assert!(
        stdout.contains(
            "Unproven checks still run at runtime; they can only reject more, never less."
        ),
        "missing runtime guarantee line: {stdout}"
    );
}

/// `--deny-unproven` turns the SAME unproven surface into a non-zero exit, for teams that want
/// strict CI. Default (above) is exit 0; this is the opt-in strict mode.
#[test]
fn deny_unproven_fails_when_a_struct_has_an_unproven_check() {
    if !lake_available() {
        eprintln!("SKIP: lake not on PATH");
        return;
    }
    let bin = env!("CARGO_BIN_EXE_cargo-verified-anchor");
    let out = Command::new(bin)
        .args([
            "verified-anchor",
            "check",
            "-p",
            "verified-anchor-example",
            "--lean-dir",
            lean_dir().to_str().unwrap(),
            "--deny-unproven",
        ])
        .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap()) // rust/
        .output()
        .expect("run cargo-verified-anchor");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !out.status.success(),
        "expected --deny-unproven to fail:\nSTDOUT:\n{stdout}"
    );
    assert_eq!(out.status.code(), Some(1));
}
