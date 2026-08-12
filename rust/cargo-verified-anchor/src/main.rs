mod collect;
mod discharge;
mod generate;

use std::path::PathBuf;
use std::process::exit;

struct Args {
    crate_name: Option<String>,
    lean_dir: Option<PathBuf>,
    json: bool,
    deny_unproven: bool,
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
    let mut args = Args {
        crate_name: None,
        lean_dir: None,
        json: false,
        deny_unproven: false,
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "-p" | "--package" => args.crate_name = it.next(),
            "--lean-dir" => args.lean_dir = it.next().map(PathBuf::from),
            "--json" => args.json = true,
            "--deny-unproven" => args.deny_unproven = true,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(args)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("cargo-verified-anchor: {e}");
            exit(2);
        }
    };
    match run(args) {
        Ok((report, fail)) => {
            print!("{report}");
            if fail {
                exit(1);
            }
        }
        Err(e) => {
            eprintln!("cargo-verified-anchor: {e}");
            exit(1);
        }
    }
}

fn run(args: Args) -> Result<(String, bool), String> {
    let spec_dir = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join("target")
        .join("verified-anchor")
        .join("specs");
    let specs = collect::collect(args.crate_name.as_deref(), &spec_dir)?;
    if specs.is_empty() {
        return Err("no #[derive(VerifiedAccounts)] structs found — did you add `verified_anchor::emit_specs!();` to your lib?".into());
    }
    let check_lean = generate::generate_check_lean(&specs);
    let check_file = spec_dir.join("check.lean");
    std::fs::write(&check_file, &check_lean).map_err(|e| format!("write {check_file:?}: {e}"))?;

    let lean_dir = discharge::locate_lean_dir(args.lean_dir.as_deref())?;
    discharge::discharge(&lean_dir, &check_file)?;

    let fail = should_fail(&specs, args.deny_unproven);
    let report = render_report(&specs, args.json, args.deny_unproven);
    Ok((report, fail))
}

/// Render the human or `--json` report for a set of discharged specs. Every obligation in
/// `specs` discharged (an earlier `discharge::discharge` error would have short-circuited
/// `run` before this is called) — what varies is only whether each struct's proof covers its
/// FULL constraint surface or defers some of it to the Task 13 escape hatch.
///
/// `"ok"` in the `--json` output mirrors the process exit code (`should_fail`), not blanket
/// success: with `--deny-unproven`, a struct carrying unproven checks makes `ok` false, same
/// as the non-zero exit. A CI consumer keying off `ok` must never see it disagree with the
/// exit status.
fn render_report(specs: &[generate::Spec], json: bool, deny_unproven: bool) -> String {
    let total_unproven: usize = specs.iter().map(|s| s.unproven.len()).sum();
    let ok = !should_fail(specs, deny_unproven);
    let mut out = String::new();
    if json {
        out.push_str(&format!("{{\"ok\":{ok},\"structs\":["));
        for (i, s) in specs.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let k = match s.kind {
                generate::Kind::Validation => "validation",
                generate::Kind::Lifecycle => "lifecycle",
            };
            let ups: Vec<String> = s
                .unproven
                .iter()
                .map(|u| format!("\"{}\"", u.replace('\\', "\\\\").replace('"', "\\\"")))
                .collect();
            out.push_str(&format!(
                "{{\"name\":\"{}\",\"kind\":\"{}\",\"unproven\":[{}]}}",
                s.name,
                k,
                ups.join(",")
            ));
        }
        out.push_str("]}\n");
        return out;
    }
    for s in specs {
        let k = match s.kind {
            generate::Kind::Validation => "validation",
            generate::Kind::Lifecycle => "lifecycle",
        };
        out.push_str(&format!("  \u{2713} {} ({})\n", s.name, k));
        if !s.unproven.is_empty() {
            out.push_str(&format!(
                "  \u{26a0} {} \u{2014} {} unproven check(s) run outside the proof:\n",
                s.name,
                s.unproven.len()
            ));
            for u in &s.unproven {
                out.push_str(&format!("      constraint = {u}\n"));
            }
        }
    }
    out.push_str(&format!(
        "\n{} struct(s), {} proof obligation(s) discharged, {} unproven.\n",
        specs.len(),
        specs.len(),
        total_unproven
    ));
    if total_unproven > 0 {
        out.push_str(
            "Unproven checks still run at runtime; they can only reject more, never less.\n",
        );
    }
    out
}

/// `--deny-unproven`: fail CI when ANY struct defers a check past the proof, even though the
/// default (exit 0) is intentional — see the module doc / Task 14 brief for why the honest
/// default is a green check plus an inventory, not a failure.
fn should_fail(specs: &[generate::Spec], deny_unproven: bool) -> bool {
    deny_unproven && specs.iter().any(|s| !s.unproven.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::{Kind, Spec};

    fn spec(name: &str, unproven: &[&str]) -> Spec {
        Spec {
            name: name.into(),
            kind: Kind::Validation,
            lean_spec: "SPEC".into(),
            unproven: unproven.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn reports_proven_structs_plainly() {
        let r = render_report(&[spec("Deposit", &[])], false, false);
        assert!(r.contains("\u{2713} Deposit (validation)"));
        assert!(!r.contains("\u{26a0}"));
    }

    #[test]
    fn reports_unproven_checks_prominently() {
        let r = render_report(
            &[spec("Deposit", &["custom_check(vault, clock)"])],
            false,
            false,
        );
        assert!(r.contains("\u{26a0}"), "report was: {r}");
        assert!(r.contains("custom_check(vault, clock)"), "report was: {r}");
        assert!(r.contains("1 unproven"), "report was: {r}");
    }

    #[test]
    fn deny_unproven_makes_it_an_error() {
        assert!(!should_fail(&[spec("A", &[])], true));
        assert!(should_fail(&[spec("A", &["f(x)"])], true));
        assert!(!should_fail(&[spec("A", &["f(x)"])], false));
    }

    #[test]
    fn json_report_lists_unproven() {
        let r = render_report(&[spec("A", &["f(x)"])], true, false);
        assert!(r.contains("\"unproven\":[\"f(x)\"]"), "report was: {r}");
    }

    #[test]
    fn json_ok_field_matches_the_exit_condition() {
        // Without --deny-unproven, unproven checks don't fail the process, so ok stays true.
        let r = render_report(&[spec("A", &["f(x)"])], true, false);
        assert!(r.contains("\"ok\":true"), "report was: {r}");

        // With --deny-unproven and an unproven check present, the process exits non-zero —
        // "ok" must say the same thing, not unconditionally claim success.
        let r = render_report(&[spec("A", &["f(x)"])], true, true);
        assert!(r.contains("\"ok\":false"), "report was: {r}");

        // With --deny-unproven but nothing unproven, both still agree on success.
        let r = render_report(&[spec("A", &[])], true, true);
        assert!(r.contains("\"ok\":true"), "report was: {r}");
    }
}
