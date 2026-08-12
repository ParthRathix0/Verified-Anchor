#[test]
fn unsupported_constraints_are_rejected() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}

/// The other half of the contract, and the more important one: verified-anchor must NEVER
/// refuse to build a program real Anchor accepts. `tests/ui/pass/` holds the fixtures that
/// would fail if an out-of-sublanguage `constraint = <expr>` ever became a compile error
/// again. They live in a subdirectory because the glob above claims `tests/ui/*.rs` for the
/// rejection fixtures.
#[test]
fn valid_anchor_constraints_always_compile() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass/*.rs");
}
