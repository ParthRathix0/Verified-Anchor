use verified_anchor::VerifiedAccounts;

#[derive(VerifiedAccounts)]
struct Bad<'info> {
    #[account(realloc = 8)]
    vault: verified_anchor::UncheckedAccount<'info>,
}

fn main() {}
