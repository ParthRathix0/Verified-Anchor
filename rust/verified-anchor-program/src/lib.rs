#![allow(dead_code)]
use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
};
use verified_anchor::{Accounts, Key, Validate, VerifiedAccounts};

// Needed so `Account<'info, T>` (which implies `owner = crate::ID`) compiles in the derive macro.
// Tests that exercise typed accounts (init_if_needed, tag 10) load the program under this id.
pub const ID: solana_program::pubkey::Pubkey =
    solana_program::pubkey::Pubkey::new_from_array([0x0Bu8; 32]);

/// init a new account. Accounts: [new, payer, system_program].
#[derive(VerifiedAccounts)]
struct InitOne<'info> {
    #[account(init, payer = payer, space = 0)]
    new: verified_anchor::UncheckedAccount<'info>,
    #[account(mut)]
    payer: verified_anchor::Signer<'info>,
    system_program: verified_anchor::Program<'info, verified_anchor::System>,
}

/// close an account. Accounts: [target, dest].
#[derive(VerifiedAccounts)]
struct CloseOne<'info> {
    #[account(close = dest)]
    target: verified_anchor::UncheckedAccount<'info>,
    #[account(mut)]
    dest: verified_anchor::UncheckedAccount<'info>,
}

/// validate a PDA. Accounts: [pda]. Instruction data: [2, arg0, arg1, arg2, arg3].
#[derive(VerifiedAccounts)]
struct CheckPda<'info> {
    #[account(seeds = [b"vault", arg(0, 4)], bump)]
    pda: verified_anchor::UncheckedAccount<'info>,
}

/// validate a PDA with an opt-in stored (non-canonical) bump read from instr data byte 0.
/// Accounts: [pda]. Instruction data: [3, stored_bump].
#[derive(VerifiedAccounts)]
struct CheckStoredBump<'info> {
    #[account(seeds = [b"vault"], bump = arg(0))]
    pda: verified_anchor::UncheckedAccount<'info>,
}

/// A fixed FOREIGN program id the PDA derives against, regardless of which program runs this.
const FOREIGN_PROGRAM: Pubkey = Pubkey::new_from_array([9u8; 32]);

/// validate a PDA derived against a FOREIGN program id via `seeds::program`.
/// Accounts: [pda]. Instruction data: [4].
#[derive(VerifiedAccounts)]
struct CheckForeignPda<'info> {
    #[account(seeds = [b"vault"], seeds::program = FOREIGN_PROGRAM, bump)]
    pda: verified_anchor::UncheckedAccount<'info>,
}

/// validate that an account is rent-exempt. Accounts: [vault]. Instruction data: [5].
/// `rent_exempt = enforce` — rejects under-funded accounts on-chain.
#[derive(VerifiedAccounts)]
struct CheckRentExempt<'info> {
    #[account(rent_exempt = enforce)]
    vault: verified_anchor::UncheckedAccount<'info>,
}

/// validate an account with `rent_exempt = skip` — the opt-out.
/// Under-funded accounts pass through. Accounts: [vault]. Instruction data: [6].
#[derive(VerifiedAccounts)]
struct CheckRentSkip<'info> {
    #[account(rent_exempt = skip)]
    vault: verified_anchor::UncheckedAccount<'info>,
}

// ── M9: realloc / zero / init_if_needed ─────────────────────────────────────

/// Typed account used by init_if_needed (tag 10) and realloc (tags 7/8).
/// Discriminator = sha256(b"account:Counter")[..8].
#[verified_anchor::account]
pub struct Counter {
    pub value: u64,
}

/// Grow an account to 80 bytes.
/// Accounts: [vault (mut), payer (mut signer), system_program].
/// Instruction data: [7].
#[derive(VerifiedAccounts)]
struct ReallocGrow<'info> {
    #[account(mut, realloc = 80, realloc::payer = payer)]
    vault: verified_anchor::UncheckedAccount<'info>,
    #[account(mut)]
    payer: verified_anchor::Signer<'info>,
    system_program: verified_anchor::Program<'info, verified_anchor::System>,
}

/// Shrink an account to 32 bytes (surplus lamports are preserved — not drained).
/// Accounts: [vault (mut), payer (mut signer), system_program].
/// Instruction data: [8].
#[derive(VerifiedAccounts)]
struct ReallocShrink<'info> {
    #[account(mut, realloc = 32, realloc::payer = payer)]
    vault: verified_anchor::UncheckedAccount<'info>,
    #[account(mut)]
    payer: verified_anchor::Signer<'info>,
    system_program: verified_anchor::Program<'info, verified_anchor::System>,
}

/// Validate that a `zero`-annotated account has an all-zero 8-byte discriminator.
/// Accounts: [data]. Instruction data: [9].
#[derive(VerifiedAccounts)]
struct ZeroCheck<'info> {
    #[account(zero)]
    data: verified_anchor::UncheckedAccount<'info>,
}

/// Conditionally initialise a typed SEEDED PDA — the real-world init_if_needed drop-in.
/// Accounts: [data (PDA Account<Counter>), payer (mut signer), system_program].
/// Instruction data: [10].
/// The `seeds`/`bump` identify the account instance and hold on BOTH a fresh (system-owned)
/// and an already-initialised account, so the normal `validate` + `execute_lifecycle` flow
/// works as a genuine drop-in: validate identifies the PDA (its wrapper owner/disc checks are
/// filtered out for the iin field), then execute_lifecycle inits-if-fresh / reinit-guards.
#[derive(VerifiedAccounts)]
struct InitIfNeeded<'info> {
    #[account(init_if_needed, payer = payer, space = 8, seeds = [b"counter"], bump)]
    data: verified_anchor::Account<'info, Counter>,
    #[account(mut)]
    payer: verified_anchor::Signer<'info>,
    system_program: verified_anchor::Program<'info, verified_anchor::System>,
}

// ── M10 Task 15: on-chain proof for the new constraint-expr sublanguage ─────────────────────
// Native tests build `AccountInfo`s by hand; only litesvm runs the generated code inside a
// real loader against real Borsh-encoded account data. These three arms exist ONLY so the
// litesvm suite (`runtime_constraints.rs`) can prove, on-chain, that (a) the length-prefix
// walk correctly locates a field after a `String`, (b) `#[instruction(...)]` argument decoding
// works against real instruction data, and (c) the escape hatch actually executes.

/// Typed account with a `String` field BEFORE the field the constraint reads. Discriminator =
/// sha256(b"account:DataVault")[..8]. Uses `crate::ID` for its `Account<'info, T>` owner check,
/// same as `Counter`.
#[verified_anchor::account]
pub struct DataVault {
    pub label: String,
    pub amount: u64,
}

/// `constraint = vault.amount >= 1000` over `amount`, which sits AFTER the variable-width
/// `label: String`. Proves the length-prefix walk that locates `amount` works on a real
/// Borsh-encoded on-chain account, not just against hand-built native fixtures.
/// Accounts: [vault (Account<DataVault>), out (mut)]. Instruction data: [11].
#[derive(VerifiedAccounts)]
struct CheckConstraintAfterString<'info> {
    #[account(constraint = vault.amount >= 1000)]
    vault: verified_anchor::Account<'info, DataVault>,
    #[account(mut)]
    out: verified_anchor::UncheckedAccount<'info>,
}

/// `#[instruction(name: String)]` seed using `name.as_bytes()` — proves the generated code
/// decodes a Borsh `String` argument out of real instruction data and derives the PDA from its
/// payload bytes (length prefix stripped), on-chain. Accounts: [pda, out (mut)].
/// Instruction data: [12, borsh-encoded `name: String`].
#[derive(VerifiedAccounts)]
#[instruction(name: String)]
struct CheckNamedSeed<'info> {
    #[account(seeds = [b"vault", name.as_bytes()], bump)]
    pda: verified_anchor::UncheckedAccount<'info>,
    #[account(mut)]
    out: verified_anchor::UncheckedAccount<'info>,
}

/// An escape-hatch `constraint = <expr>` (`a.key() == crate::ID` is outside the proven
/// sublanguage — a module-qualified path). The escape hatch runs ONLY inside `try_accounts`,
/// never inside `Validate::validate` alone, so this arm deliberately calls `try_accounts`
/// rather than the `validate` + `execute_lifecycle` pattern every other arm uses.
/// Accounts: [a, out (mut)]. Instruction data: [13].
#[derive(VerifiedAccounts)]
struct CheckEscapeHatchOnChain<'info> {
    #[account(constraint = a.key() == crate::ID)]
    a: verified_anchor::UncheckedAccount<'info>,
    #[account(mut)]
    out: verified_anchor::UncheckedAccount<'info>,
}

entrypoint!(process);
// `try_accounts` (tag 13) needs `accounts: &'a [AccountInfo<'a>]` — `AccountInfo` is invariant
// over its lifetime, so the elided-lifetime signature every other arm was fine with does not
// unify. Name the lifetime explicitly instead of relaxing the trait.
pub fn process<'a>(
    program_id: &Pubkey,
    accounts: &'a [AccountInfo<'a>],
    data: &[u8],
) -> ProgramResult {
    match data.first() {
        Some(0) => {
            InitOne::validate(accounts, &[], program_id)
                .map_err(|_| ProgramError::InvalidArgument)?;
            // rent-exempt-ish lamports for 8 bytes; the test funds the payer generously
            InitOne::execute_lifecycle(accounts, program_id, 1_000_000)
                .map_err(|_| ProgramError::InvalidArgument)?;
            Ok(())
        }
        Some(1) => {
            CloseOne::validate(accounts, &[], program_id)
                .map_err(|_| ProgramError::InvalidArgument)?;
            CloseOne::execute_lifecycle(accounts, program_id, 0)
                .map_err(|_| ProgramError::InvalidArgument)?;
            Ok(())
        }
        Some(2) => {
            // instr_data after the 1-byte tag carries the 4-byte seed arg
            CheckPda::validate(accounts, &data[1..], program_id)
                .map_err(|_| ProgramError::InvalidArgument)?;
            Ok(())
        }
        Some(3) => {
            // instr_data after the 1-byte tag carries the stored bump byte at offset 0
            CheckStoredBump::validate(accounts, &data[1..], program_id)
                .map_err(|_| ProgramError::InvalidArgument)?;
            Ok(())
        }
        Some(4) => {
            // PDA derived against the FOREIGN program id (seeds::program), not `program_id`.
            CheckForeignPda::validate(accounts, &data[1..], program_id)
                .map_err(|_| ProgramError::InvalidArgument)?;
            Ok(())
        }
        Some(5) => {
            // rent_exempt = enforce: rejects account that is not rent-exempt.
            CheckRentExempt::validate(accounts, &data[1..], program_id)
                .map_err(ProgramError::from)?;
            Ok(())
        }
        Some(6) => {
            // rent_exempt = skip: any account passes (opt-out, no check).
            CheckRentSkip::validate(accounts, &data[1..], program_id)
                .map_err(|_| ProgramError::InvalidArgument)?;
            Ok(())
        }
        Some(7) => {
            // realloc grow: vault → 80 bytes; payer tops up rent if needed.
            ReallocGrow::validate(accounts, &[], program_id)
                .map_err(|_| ProgramError::InvalidArgument)?;
            ReallocGrow::execute_lifecycle(accounts, program_id, 0)
                .map_err(|_| ProgramError::InvalidArgument)?;
            Ok(())
        }
        Some(8) => {
            // realloc shrink: vault → 32 bytes; surplus lamports are preserved.
            ReallocShrink::validate(accounts, &[], program_id)
                .map_err(|_| ProgramError::InvalidArgument)?;
            ReallocShrink::execute_lifecycle(accounts, program_id, 0)
                .map_err(|_| ProgramError::InvalidArgument)?;
            Ok(())
        }
        Some(9) => {
            // zero: accept an all-zero discriminator; reject a non-zeroed one.
            ZeroCheck::validate(accounts, &[], program_id)
                .map_err(|_| ProgramError::InvalidArgument)?;
            Ok(())
        }
        Some(10) => {
            // init_if_needed DROP-IN: the normal validate + execute_lifecycle flow, no manual
            // disc stamp, no validate-skip. `validate` identifies the seeded PDA (the iin
            // field's wrapper owner/disc checks are filtered out in codegen); then
            // `execute_lifecycle` inits the fresh PDA (stamping Counter::DISCRIMINATOR itself)
            // or, on an already-initialised account, reinit-guards owner+size (rejecting a
            // wrong-owner/undersized existing account).
            // 2_000_000 lamports covers the rent-exempt minimum for space=8+8=16 bytes.
            InitIfNeeded::validate(accounts, &[], program_id)
                .map_err(|_| ProgramError::InvalidArgument)?;
            InitIfNeeded::execute_lifecycle(accounts, program_id, 2_000_000)
                .map_err(|_| ProgramError::InvalidArgument)?;
            Ok(())
        }
        Some(11) => {
            // constraint = vault.amount >= 1000, where `amount` sits AFTER a `String` field.
            // Proves the length-prefix walk that locates `amount` on-chain, not just in a
            // hand-built native fixture.
            CheckConstraintAfterString::validate(accounts, &[], program_id)
                .map_err(|_| ProgramError::InvalidArgument)?;
            // Observable effect: echo the `amount` that passed the constraint into `out`, read
            // by walking the SAME length-prefixed layout the proven check just located.
            let vault_data = accounts[0]
                .try_borrow_data()
                .map_err(|_| ProgramError::InvalidAccountData)?;
            let label_len = u32::from_le_bytes(vault_data[8..12].try_into().unwrap()) as usize;
            let amount_off = 12 + label_len;
            let amount =
                u64::from_le_bytes(vault_data[amount_off..amount_off + 8].try_into().unwrap());
            drop(vault_data);
            let mut out_data = accounts[1]
                .try_borrow_mut_data()
                .map_err(|_| ProgramError::InvalidAccountData)?;
            out_data[..8].copy_from_slice(&amount.to_le_bytes());
            Ok(())
        }
        Some(12) => {
            // #[instruction(name: String)] seed: `data[1..]` is the Borsh-encoded `name` arg,
            // exactly what a real client sends. Proves argument decoding + `name.as_bytes()`
            // seed derivation on-chain.
            CheckNamedSeed::validate(accounts, &data[1..], program_id)
                .map_err(|_| ProgramError::InvalidArgument)?;
            // Observable effect: echo the accepted PDA's key into `out`.
            let mut out_data = accounts[1]
                .try_borrow_mut_data()
                .map_err(|_| ProgramError::InvalidAccountData)?;
            out_data[..32].copy_from_slice(&accounts[0].key.to_bytes());
            Ok(())
        }
        Some(13) => {
            // Escape-hatch constraint (`a.key() == crate::ID`, outside the proven sublanguage).
            // It runs ONLY inside `try_accounts` — `Validate::validate` alone never sees it — so
            // this arm deliberately goes through `try_accounts`, unlike every other arm above.
            CheckEscapeHatchOnChain::try_accounts(program_id, accounts, &data[1..])
                .map_err(|_| ProgramError::InvalidArgument)?;
            // Observable effect: flip a byte in `out` once the hatch has run and accepted.
            let mut out_data = accounts[1]
                .try_borrow_mut_data()
                .map_err(|_| ProgramError::InvalidAccountData)?;
            out_data[0] = 1;
            Ok(())
        }
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
