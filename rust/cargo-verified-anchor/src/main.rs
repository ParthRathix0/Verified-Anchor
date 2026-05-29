mod collect;
mod generate;
mod discharge;

use std::path::PathBuf;
use std::process::exit;

struct Args {
    crate_name: Option<String>,
    lean_dir: Option<PathBuf>,
    json: bool,
}

fn parse_args() -> Result<Args, String> {
    // Invoked as `cargo verified-anchor check ...` => argv: [bin, "verified-anchor", "check", ...]
    let mut it = std::env::args().skip(1).peekable();
    if it.peek().map(|s| s == "verified-anchor").unwrap_or(false) {
        it.next();
    }
    match it.next().as_deref() {
        Some("check") => {}
        other => return Err(format!("expected subcommand `check`, got {other:?}")),
    }
    let mut args = Args { crate_name: None, lean_dir: None, json: false };
    while let Some(a) = it.next() {
        match a.as_str() {
            "-p" | "--package" => args.crate_name = it.next(),
            "--lean-dir" => args.lean_dir = it.next().map(PathBuf::from),
            "--json" => args.json = true,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(args)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => { eprintln!("cargo-verified-anchor: {e}"); exit(2); }
    };
    match run(args) {
        Ok(report) => { print!("{report}"); }
        Err(e) => { eprintln!("cargo-verified-anchor: {e}"); exit(1); }
    }
}

fn run(args: Args) -> Result<String, String> {
    let spec_dir = std::env::current_dir().map_err(|e| e.to_string())?
        .join("target").join("verified-anchor").join("specs");
    let specs = collect::collect(args.crate_name.as_deref(), &spec_dir)?;
    if specs.is_empty() {
        return Err("no #[derive(VerifiedAccounts)] structs found — did you add `verified_anchor::emit_specs!();` to your lib?".into());
    }
    let check_lean = generate::generate_check_lean(&specs);
    let check_file = spec_dir.join("check.lean");
    std::fs::write(&check_file, &check_lean).map_err(|e| format!("write {check_file:?}: {e}"))?;

    let lean_dir = discharge::locate_lean_dir(args.lean_dir.as_deref())?;
    discharge::discharge(&lean_dir, &check_file)?;

    let mut report = String::new();
    if args.json {
        report.push_str("{\"ok\":true,\"structs\":[");
        for (i, s) in specs.iter().enumerate() {
            if i > 0 { report.push(','); }
            let k = match s.kind { generate::Kind::Validation => "validation", generate::Kind::Lifecycle => "lifecycle" };
            report.push_str(&format!("{{\"name\":\"{}\",\"kind\":\"{}\"}}", s.name, k));
        }
        report.push_str("]}\n");
    } else {
        for s in &specs {
            let k = match s.kind { generate::Kind::Validation => "validation", generate::Kind::Lifecycle => "lifecycle" };
            report.push_str(&format!("  \u{2713} {} ({})\n", s.name, k));
        }
        report.push_str(&format!("All {} proof obligation(s) discharged.\n", specs.len()));
    }
    Ok(report)
}
