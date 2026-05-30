//! `Context<'a, 'b, 'c, 'info, T>` — mirrors stock Anchor's signature so a
//! verified-anchor instruction handler is type-identical to a stock-Anchor one.

use core::marker::PhantomData;
use solana_program::{account_info::AccountInfo, pubkey::Pubkey};

use crate::Accounts;

pub struct Context<'a, 'b, 'c, 'info, T: Accounts<'info>> {
    pub accounts: T,
    pub program_id: &'a Pubkey,
    pub remaining_accounts: &'c [AccountInfo<'info>],
    pub bumps: T::Bumps,
    _phantom: PhantomData<&'b ()>,
}

impl<'a, 'b, 'c, 'info, T: Accounts<'info>> Context<'a, 'b, 'c, 'info, T> {
    pub fn new(
        program_id: &'a Pubkey,
        accounts: T,
        remaining_accounts: &'c [AccountInfo<'info>],
        bumps: T::Bumps,
    ) -> Self {
        Self { accounts, program_id, remaining_accounts, bumps, _phantom: PhantomData }
    }
}
