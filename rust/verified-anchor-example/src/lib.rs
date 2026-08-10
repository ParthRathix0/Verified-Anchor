//! A worked verified-anchor user crate. `cargo build` compiles it; `cargo verified-anchor
//! check -p verified-anchor-example` discharges every struct's proof obligation via Lean.
use verified_anchor::VerifiedAccounts;

// Required so that Account<'info, T> (which implies owner = crate::ID) compiles.
solana_program::declare_id!("VAExamp111111111111111111111111111111111111");

/// Minimal typed account used by the M9 init_if_needed example struct.
/// Discriminator = sha256(b"account:ExampleData")[..8].
#[verified_anchor::account]
pub struct ExampleData {
    pub value: u64,
}

/// Validation: a PDA account derived from a literal + an instruction-arg seed.
#[derive(VerifiedAccounts)]
pub struct CheckPda<'info> {
    #[account(seeds = [b"vault", arg(0, 4)], bump)]
    pub pda: verified_anchor::UncheckedAccount<'info>,
}

/// Validation: signer + writable.
#[derive(VerifiedAccounts)]
pub struct Transfer<'info> {
    #[account(mut)]
    pub vault: verified_anchor::UncheckedAccount<'info>,
    pub authority: verified_anchor::Signer<'info>,
}

/// Lifecycle: init a new account, and close one to a destination.
#[derive(VerifiedAccounts)]
pub struct Lifecycle<'info> {
    #[account(init, payer = payer, space = 0)]
    pub new_acct: verified_anchor::UncheckedAccount<'info>,
    #[account(mut)]
    pub payer: verified_anchor::Signer<'info>,
    #[account(close = payer)]
    pub old_acct: verified_anchor::UncheckedAccount<'info>,
}

// ── M9: zero / realloc / init_if_needed ──────────────────────────────────────

/// Validation (M9): `zero` reinit guard — account discriminator must be all-zero.
/// Obligation kind: M4Subset (zero ∈ isM4Constraint).
#[derive(VerifiedAccounts)]
pub struct ZeroCheck<'info> {
    #[account(zero)]
    pub data: verified_anchor::UncheckedAccount<'info>,
}

/// Lifecycle (M9): realloc — resize the account data to 64 bytes.
/// Obligation kind: StructLifecycleWF.
#[derive(VerifiedAccounts)]
pub struct ReallocData<'info> {
    #[account(mut, realloc = 64, realloc::payer = payer)]
    pub vault: verified_anchor::UncheckedAccount<'info>,
    #[account(mut)]
    pub payer: verified_anchor::Signer<'info>,
}

/// Lifecycle (M9): init_if_needed on a typed seeded PDA — the real drop-in pattern.
/// Obligation kind: StructLifecycleWF.
/// The Task 6 guard requires a typed Account<T>; seeds identify the PDA.
#[derive(VerifiedAccounts)]
pub struct InitIfNeededPda<'info> {
    #[account(init_if_needed, payer = payer, space = 8, seeds = [b"example"], bump)]
    pub data: verified_anchor::Account<'info, ExampleData>,
    #[account(mut)]
    pub payer: verified_anchor::Signer<'info>,
}

verified_anchor::emit_specs!();
