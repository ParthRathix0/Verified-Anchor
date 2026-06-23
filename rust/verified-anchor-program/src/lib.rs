#![allow(dead_code)]
use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult,
    program_error::ProgramError, pubkey::Pubkey,
};
use verified_anchor::{Validate, VerifiedAccounts};

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

entrypoint!(process);
pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    match data.first() {
        Some(0) => {
            InitOne::validate(accounts, &[], program_id).map_err(|_| ProgramError::InvalidArgument)?;
            // rent-exempt-ish lamports for 8 bytes; the test funds the payer generously
            InitOne::execute_lifecycle(accounts, program_id, 1_000_000)
                .map_err(|_| ProgramError::InvalidArgument)?;
            Ok(())
        }
        Some(1) => {
            CloseOne::validate(accounts, &[], program_id).map_err(|_| ProgramError::InvalidArgument)?;
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
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
