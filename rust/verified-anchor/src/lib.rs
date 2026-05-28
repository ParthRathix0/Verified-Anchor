//! Verified Anchor runtime support (Milestone 2).
use solana_program::account_info::AccountInfo;

pub use verified_anchor_macros::VerifiedAccounts;

/// Why account validation failed. `field` is the struct field name that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VAError {
    MissingSigner { field: &'static str },
    NotWritable { field: &'static str },
    WrongOwner { field: &'static str },
    /// Fewer accounts were supplied than the struct declares.
    NotEnoughAccounts { expected: usize, got: usize },
    WrongHasOne { field: &'static str, target: &'static str },
    InitFailed { field: &'static str },
    CloseFailed { field: &'static str },
}

impl core::fmt::Display for VAError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VAError::MissingSigner { field } => write!(f, "account `{field}` must be a signer"),
            VAError::NotWritable { field } => write!(f, "account `{field}` must be writable"),
            VAError::WrongOwner { field } => write!(f, "account `{field}` has the wrong owner"),
            VAError::NotEnoughAccounts { expected, got } =>
                write!(f, "expected {expected} accounts, got {got}"),
            VAError::WrongHasOne { field, target } =>
                write!(f, "account `{field}` field does not match `{target}`"),
            VAError::InitFailed { field } => write!(f, "init failed for `{field}`"),
            VAError::CloseFailed { field } => write!(f, "close failed for `{field}`"),
        }
    }
}

impl std::error::Error for VAError {}

/// Implemented by `#[derive(VerifiedAccounts)]`. Validation is positional over the
/// runtime account slice (index = field declaration order), matching the Lean `Ctx`.
pub trait Validate {
    fn validate(accounts: &[AccountInfo]) -> Result<(), VAError>;
}
