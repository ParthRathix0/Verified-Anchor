use verified_anchor::VerifiedAccounts;

#[derive(VerifiedAccounts)]
struct Bad {
    #[account(realloc = 8)]
    vault: u8,
}

fn main() {}
