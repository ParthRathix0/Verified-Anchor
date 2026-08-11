// See has_one_unlocatable.rs for why this is silenced.
#![allow(unexpected_cfgs)]

use verified_anchor::VerifiedAccounts;

verified_anchor::solana_program::declare_id!("VANotPk111111111111111111111111111111111111");

// `amount` IS locatable, but it is a `u64`. `read_val` would yield `Value::Nat`, never
// `Value::Key`, so the generated check would reject every account at runtime.
#[verified_anchor::account]
pub struct Vault {
    pub amount: u64,
}

#[derive(VerifiedAccounts)]
struct BadTarget<'info> {
    #[account(has_one = amount)]
    vault: verified_anchor::Account<'info, Vault>,
    amount: verified_anchor::UncheckedAccount<'info>,
}

fn main() {}
