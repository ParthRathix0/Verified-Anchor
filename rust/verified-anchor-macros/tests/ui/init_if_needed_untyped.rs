use verified_anchor::VerifiedAccounts;

#[derive(VerifiedAccounts)]
struct BadInit<'info> {
    #[account(init_if_needed, payer = payer, space = 64)]
    data: verified_anchor::UncheckedAccount<'info>,
    payer: verified_anchor::Signer<'info>,
}

fn main() {}
