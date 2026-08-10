use verified_anchor::VerifiedAccounts;

#[derive(VerifiedAccounts)]
struct Bad<'info> {
    #[account(mint)]
    vault: verified_anchor::UncheckedAccount<'info>,
}

fn main() {}
