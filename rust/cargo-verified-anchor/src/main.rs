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
    // Pipeline lands in C3; stub for now so the crate builds.
    let _ = args;
    Ok(String::new())
}
