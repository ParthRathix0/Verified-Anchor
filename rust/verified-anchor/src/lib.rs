//! Verified Anchor runtime support (Milestone 2).
use solana_program::account_info::AccountInfo;
use solana_program::pubkey::Pubkey;

pub mod account_data;
pub use account_data::{AccountData, ProgramId, System};

pub mod account;
pub use account::{Account, Signer, Program, SystemAccount, UncheckedAccount};

pub mod context;
pub use context::Context;

pub mod prelude;

pub use verified_anchor_macros::VerifiedAccounts;
pub use verified_anchor_macros::AccountData as AccountData;
pub use verified_anchor_macros::account;

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
    WrongPda { field: &'static str },
    WrongBump { field: &'static str },
    WrongDiscriminator { field: &'static str },
    BorshFailed { field: &'static str },
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
            VAError::WrongPda { field } => write!(f, "account `{field}` is not the expected PDA"),
            VAError::WrongBump { field } => write!(f, "account `{field}` has a non-canonical bump"),
            VAError::WrongDiscriminator { field } => write!(f, "account `{field}` has the wrong 8-byte discriminator"),
            VAError::BorshFailed { field } => write!(f, "Borsh deserialization failed for `{field}`"),
        }
    }
}

impl std::error::Error for VAError {}

/// Implemented by `#[derive(VerifiedAccounts)]`. Validation is positional over the
/// runtime account slice (index = field declaration order), matching the Lean `Ctx`.
pub trait Validate {
    fn validate(
        accounts: &[AccountInfo],
        instr_data: &[u8],
        program_id: &Pubkey,
    ) -> Result<(), VAError>;
}

/// THE DEVELOPER SURFACE (M7a). `try_accounts` calls `Self::validate`
/// (the proven layer) and Borsh-deserialises each `Account<T>` field.
pub trait Accounts<'info>: Sized {
    type Bumps;
    fn try_accounts(
        program_id: &Pubkey,
        accounts: &'info [AccountInfo<'info>],
        instr_data: &[u8],
    ) -> Result<Self, VAError>;
}

// The spec-collection machinery uses `inventory`, whose `#[used]` link-section statics
// corrupt the Solana SBF ELF (invalid PT_DYNAMIC -> loader rejects with InvalidAccountData).
// It is host-only: gate ALL of it out of the `target_os = "solana"` (BPF) build, the same way
// Anchor gates host-only code. Native builds (the example crate + `cargo verified-anchor check`,
// which runs `cargo test --lib` natively) keep it.

/// Re-exported so the derive macro can emit `::verified_anchor::inventory::submit!`.
#[cfg(not(target_os = "solana"))]
pub use inventory;

/// One registered `#[derive(VerifiedAccounts)]` struct.
#[cfg(not(target_os = "solana"))]
pub struct SpecEntry {
    pub name: &'static str,
    /// The Milestone-1 `AccountsStruct` literal (Lean source).
    pub lean_spec: fn() -> String,
    /// True if any field carries an `init`/`close` constraint (selects the obligation kind).
    pub has_lifecycle: bool,
}

#[cfg(not(target_os = "solana"))]
inventory::collect!(SpecEntry);

/// All registered structs in the current compilation artifact.
#[cfg(not(target_os = "solana"))]
pub fn collect_specs() -> Vec<&'static SpecEntry> {
    inventory::iter::<SpecEntry>.into_iter().collect()
}

/// Write one spec file per registered struct into `dir`. Filename is `<name>.<kind>` where
/// kind is `lifecycle` or `validation`; the file content is the `lean_spec()` literal.
/// (No JSON — the literal is the whole content, so there's nothing to escape.)
#[cfg(not(target_os = "solana"))]
pub fn write_spec_files(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for e in collect_specs() {
        let kind = if e.has_lifecycle { "lifecycle" } else { "validation" };
        std::fs::write(dir.join(format!("{}.{}", e.name, kind)), (e.lean_spec)())?;
    }
    Ok(())
}

/// Drop ONE call in your crate's lib (e.g. bottom of `src/lib.rs`). Expands to a test that,
/// when `VERIFIED_ANCHOR_SPEC_DIR` is set (by `cargo verified-anchor check`), writes spec
/// files for every derived struct in this crate. Placing it in the lib is REQUIRED: the
/// emitter must be same-crate as the `inventory::submit!`s (cross-crate harnesses dead-strip).
#[macro_export]
macro_rules! emit_specs {
    () => {
        #[cfg(test)]
        #[test]
        fn __verified_anchor_emit_specs() {
            if let Ok(dir) = ::std::env::var("VERIFIED_ANCHOR_SPEC_DIR") {
                ::verified_anchor::write_spec_files(::std::path::Path::new(&dir)).unwrap();
            }
        }
    };
}

#[cfg(test)]
mod spec_collection_tests {
    use super::*;

    // A manually-registered entry (same crate → inventory sees it).
    inventory::submit! { SpecEntry { name: "FakeStruct", lean_spec: || "FAKE-SPEC".to_string(), has_lifecycle: false } }

    #[test]
    fn write_spec_files_emits_one_file_per_entry() {
        let dir = std::env::temp_dir().join("va-m1-spec-test");
        let _ = std::fs::remove_dir_all(&dir);
        write_spec_files(&dir).unwrap();
        let f = dir.join("FakeStruct.validation");
        assert!(f.exists(), "expected {f:?}");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "FAKE-SPEC");
    }
}
