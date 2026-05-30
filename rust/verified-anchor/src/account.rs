//! The six typed wrappers the M7a macro recognises. Each is a thin marker over
//! `&'info AccountInfo<'info>`; `Account<'info, T>` additionally carries the
//! Borsh-deserialised T (the macro fills it in `try_accounts`).

use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use solana_program::account_info::AccountInfo;

use crate::account_data::{AccountData, ProgramId};

/// `Account<'info, T>` — Anchor-style typed account. The macro auto-implies
/// owner=crate::ID + discriminator=T::DISCRIMINATOR in validate, and
/// Borsh-deserialises T in try_accounts (skipping the 8-byte discriminator).
pub struct Account<'info, T: AccountData> {
    pub info: &'info AccountInfo<'info>,
    pub data: T,
}

impl<'info, T: AccountData> Deref for Account<'info, T> {
    type Target = T;
    fn deref(&self) -> &T { &self.data }
}
impl<'info, T: AccountData> DerefMut for Account<'info, T> {
    fn deref_mut(&mut self) -> &mut T { &mut self.data }
}

/// `Signer<'info>` — auto-implies `is_signer == true`.
pub struct Signer<'info> {
    pub info: &'info AccountInfo<'info>,
}

/// `Program<'info, P>` — auto-implies `executable == true` AND `info.key == P::ID`.
pub struct Program<'info, P: ProgramId> {
    pub info: &'info AccountInfo<'info>,
    _phantom: PhantomData<P>,
}
impl<'info, P: ProgramId> Program<'info, P> {
    /// Constructed by the macro after the wrapper checks pass.
    pub fn new(info: &'info AccountInfo<'info>) -> Self {
        Self { info, _phantom: PhantomData }
    }
}

/// `SystemAccount<'info>` — auto-implies `info.owner == system_program::ID`.
pub struct SystemAccount<'info> {
    pub info: &'info AccountInfo<'info>,
}

/// `UncheckedAccount<'info>` — escape hatch; no implied checks (explicit
/// `#[account(...)]` attributes still apply).
pub struct UncheckedAccount<'info> {
    pub info: &'info AccountInfo<'info>,
}

// `AccountInfo<'info>` is the raw Solana type — re-exported from prelude as-is
// (Task L3); no wrapper struct here.
