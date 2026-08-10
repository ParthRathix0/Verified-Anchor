use verified_anchor::VerifiedAccounts;

// Vault must implement AccountData so Account<'info, Vault> compiles without
// the secondary E0277 trait-bound error — leaving only the intended realloc-
// requires-mut compile_error from the macro guard.
#[verified_anchor::account]
pub struct Vault {
    pub balance: u64,
}

#[derive(VerifiedAccounts)]
struct NeedsMut<'info> {
    #[account(realloc = 64, realloc::payer = payer)]
    data: verified_anchor::Account<'info, Vault>,
    payer: verified_anchor::Signer<'info>,
}

fn main() {}
