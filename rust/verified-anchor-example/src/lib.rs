//! A worked verified-anchor user crate. `cargo build` compiles it; `cargo verified-anchor
//! check -p verified-anchor-example` discharges every struct's proof obligation via Lean.
use verified_anchor::VerifiedAccounts;

/// Validation: a PDA account derived from a literal + an instruction-arg seed.
#[derive(VerifiedAccounts)]
pub struct CheckPda<'info> {
    #[account(seeds = [b"vault", arg(0, 4)], bump)]
    pub pda: verified_anchor::UncheckedAccount<'info>,
}

/// Validation: signer + writable.
#[derive(VerifiedAccounts)]
pub struct Transfer<'info> {
    #[account(mut)]
    pub vault: verified_anchor::UncheckedAccount<'info>,
    pub authority: verified_anchor::Signer<'info>,
}

/// Lifecycle: init a new account, and close one to a destination.
#[derive(VerifiedAccounts)]
pub struct Lifecycle<'info> {
    #[account(init, payer = payer, space = 0)]
    pub new_acct: verified_anchor::UncheckedAccount<'info>,
    #[account(mut)]
    pub payer: verified_anchor::Signer<'info>,
    #[account(close = payer)]
    pub old_acct: verified_anchor::UncheckedAccount<'info>,
}

verified_anchor::emit_specs!();
