//! Recommended `use verified_anchor::prelude::*;` — gives users the typed
//! wrappers, traits, `Context`, and the derive macros in one import.

pub use crate::{
    Account, Accounts, AccountData, Context, Program, ProgramId,
    Signer, System, SystemAccount, UncheckedAccount, VAError, Validate, VerifiedAccounts,
};
pub use solana_program::account_info::AccountInfo;
