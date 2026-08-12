// The residual `unexpected_cfgs` warning comes from the `inventory::submit!` item inside the
// `VerifiedAccounts` expansion, where `#[allow]` does not reach. Silenced here so the expected
// stderr does not carry rustc's version-dependent list of known `target_os` values.
#![allow(unexpected_cfgs)]

use verified_anchor::VerifiedAccounts;

verified_anchor::solana_program::declare_id!("VAiinArg11111111111111111111111111111111111");

#[verified_anchor::account]
pub struct VaultAccount {
    pub value: u64,
}

// VALID Anchor source that verified-anchor cannot support: the account is created inside
// `execute_lifecycle`, which receives no instruction data and therefore cannot rebuild the
// PDA's signer seeds. It must say so with a span, not panic inside the codegen.
#[derive(VerifiedAccounts)]
#[instruction(name: String)]
struct InitIfNeededArgSeed<'info> {
    #[account(init_if_needed, payer = payer, space = 64, seeds = [b"vault", name.as_bytes()], bump)]
    data: verified_anchor::Account<'info, VaultAccount>,
    #[account(mut)]
    payer: verified_anchor::UncheckedAccount<'info>,
}

fn main() {}
