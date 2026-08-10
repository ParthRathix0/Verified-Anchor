use verified_anchor::VerifiedAccounts;

// Account<'info, T> - the T just needs to look like a type to the macro;
// type-checking errors are irrelevant since the proc-macro guard fires first.
struct Vault;

#[derive(VerifiedAccounts)]
struct NeedsMut<'info> {
    #[account(realloc = 64, realloc::payer = payer)]
    data: verified_anchor::Account<'info, Vault>,
    payer: verified_anchor::Signer<'info>,
}

fn main() {}
