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
    Command::new("lake").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

#[test]
fn check_discharges_example_obligations() {
    if !lake_available() {
        eprintln!("SKIP: lake not on PATH");
        return;
    }
    let bin = env!("CARGO_BIN_EXE_cargo-verified-anchor");
    let out = Command::new(bin)
        .args(["verified-anchor", "check", "-p", "verified-anchor-example",
               "--lean-dir", lean_dir().to_str().unwrap()])
        .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap()) // rust/
        .output()
        .expect("run cargo-verified-anchor");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "check failed:\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}");
    assert!(stdout.contains("CheckPda (validation)"), "missing CheckPda: {stdout}");
    assert!(stdout.contains("Lifecycle (lifecycle)"), "missing Lifecycle: {stdout}");
    assert!(stdout.contains("discharged"), "missing summary: {stdout}");
}
