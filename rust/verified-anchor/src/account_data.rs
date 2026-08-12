//! Traits that user types carry into typed-account wrappers.

use solana_program::pubkey::Pubkey;

/// Anchor-compatible account-data trait. The derive `#[derive(AccountData)]`
/// implements this and the underlying Borsh traits; the `DISCRIMINATOR` is
/// `sha256(b"account:" ++ <TypeName>)[0..8]` — the real Anchor wire format.
pub trait AccountData: borsh::BorshDeserialize + borsh::BorshSerialize {
    const DISCRIMINATOR: [u8; 8];
    /// Borsh descriptor for this struct's fields, in declaration order. Walked by the
    /// generated locator so `has_one` and `constraint` read the field the developer named
    /// rather than a hardcoded offset.
    const LAYOUT: crate::layout::Ty;
    /// The same descriptor as Lean `Ty` source, spliced into `lean_spec()` at runtime.
    /// Host-only: keeping it out of the BPF build avoids embedding the string in the `.so`.
    #[cfg(not(target_os = "solana"))]
    const LAYOUT_LEAN: &'static str;
}

/// A marker for a Solana program, providing its on-chain id. Carried by
/// `Program<'info, P>` so the wrapper can check `accounts[i].key == &P::ID`.
pub trait ProgramId {
    const ID: Pubkey;
}

/// Marker for the System Program. Used as `Program<'info, System>` so the
/// wrapper auto-checks `accounts[i].key == solana_program::system_program::ID`.
pub struct System;
impl ProgramId for System {
    const ID: Pubkey = solana_program::system_program::ID;
}
