use verified_anchor::VerifiedAccounts;

// `has_one` needs the target's Borsh offset, which comes from `T::LAYOUT`. An untyped wrapper
// has no layout, so the macro must fail closed rather than fall back to a hardcoded offset.
#[derive(VerifiedAccounts)]
struct BadHasOne<'info> {
    #[account(has_one = authority)]
    vault: verified_anchor::UncheckedAccount<'info>,
    authority: verified_anchor::UncheckedAccount<'info>,
}

fn main() {}
