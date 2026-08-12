//! The six typed wrappers the M7a macro recognises. Each is a thin marker over
//! `&'info AccountInfo<'info>`; `Account<'info, T>` additionally carries the
//! Borsh-deserialised T (the macro fills it in `try_accounts`).

use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use solana_program::account_info::AccountInfo;
use solana_program::pubkey::Pubkey;

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

// ── Anchor-parity surface for the escape hatch (M10 Task 13) ───────────────────────────────
//
// An out-of-sublanguage `constraint = <expr>` runs VERBATIM as Rust against these wrappers, so
// the idioms real Anchor code is written in have to resolve here or a valid Anchor program
// would stop compiling — the one thing the prime directive forbids. Anchor gives every wrapper
// `Deref<Target = AccountInfo>` (so `a.owner`, `a.is_signer`, `a.lamports()` read the account
// meta) and a `key()` method through its `Key` trait; these impls are that surface, nothing
// more. `Account<'info, T>` deliberately keeps its `Deref<Target = T>` above: on a TYPED
// account `vault.owner` means `T::owner` under real Anchor, and diverging from that would make
// the hatch check a different thing than the developer wrote.

impl<'info> Deref for Signer<'info> {
    type Target = AccountInfo<'info>;
    fn deref(&self) -> &AccountInfo<'info> { self.info }
}
impl<'info> Deref for SystemAccount<'info> {
    type Target = AccountInfo<'info>;
    fn deref(&self) -> &AccountInfo<'info> { self.info }
}
impl<'info> Deref for UncheckedAccount<'info> {
    type Target = AccountInfo<'info>;
    fn deref(&self) -> &AccountInfo<'info> { self.info }
}
impl<'info, P: ProgramId> Deref for Program<'info, P> {
    type Target = AccountInfo<'info>;
    fn deref(&self) -> &AccountInfo<'info> { self.info }
}

/// `a.key()` — Anchor's `Key` trait, the single most common spelling in real constraints
/// (`a.key() == crate::ID`). Mirrors the `Operand::Key` the proven sublanguage compiles the
/// same source to, so a check that moves between the two paths reads the same value.
pub trait Key {
    fn key(&self) -> Pubkey;
}

impl Key for AccountInfo<'_> {
    fn key(&self) -> Pubkey { *self.key }
}
impl<T: AccountData> Key for Account<'_, T> {
    fn key(&self) -> Pubkey { *self.info.key }
}
impl Key for Signer<'_> {
    fn key(&self) -> Pubkey { *self.info.key }
}
impl Key for SystemAccount<'_> {
    fn key(&self) -> Pubkey { *self.info.key }
}
impl Key for UncheckedAccount<'_> {
    fn key(&self) -> Pubkey { *self.info.key }
}
impl<P: ProgramId> Key for Program<'_, P> {
    fn key(&self) -> Pubkey { *self.info.key }
}
