//! A worked verified-anchor user crate. `cargo build` compiles it; `cargo verified-anchor
//! check -p verified-anchor-example` discharges every struct's proof obligation via Lean.
use verified_anchor::VerifiedAccounts;

/// Validation: a PDA account derived from a literal + an instruction-arg seed.
#[derive(VerifiedAccounts)]
pub struct CheckPda {
    #[account(seeds = [b"vault", arg(0, 4)], bump)]
    pub pda: u8,
}

/// Validation: signer + writable.
#[derive(VerifiedAccounts)]
pub struct Transfer {
    #[account(mut)]
    pub vault: u8,
    #[account(signer)]
    pub authority: u8,
}

/// Lifecycle: init a new account, and close one to a destination.
#[derive(VerifiedAccounts)]
pub struct Lifecycle {
    #[account(init, payer = payer, space = 0)]
    pub new_acct: u8,
    #[account(mut, signer)]
    pub payer: u8,
    #[account(close = payer)]
    pub old_acct: u8,
}

verified_anchor::emit_specs!();
