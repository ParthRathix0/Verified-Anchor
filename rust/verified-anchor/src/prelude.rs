//! Recommended `use verified_anchor::prelude::*;` — gives users the typed
//! wrappers, traits, `Context`, and the derive macros in one import.

pub use crate::{
    account, Account, Accounts, AccountData, Context, Program, ProgramId,
    Signer, System, SystemAccount, UncheckedAccount, VAError, Validate, VerifiedAccounts,
};
pub use solana_program::account_info::AccountInfo;
pub use solana_program::declare_id;
pub use solana_program::entrypoint::ProgramResult;
pub use solana_program::pubkey::Pubkey;
