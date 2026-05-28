use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult,
    program_error::ProgramError, pubkey::Pubkey,
};
use verified_anchor::{Validate, VerifiedAccounts};

/// init a new account. Accounts: [new, payer, system_program].
#[derive(VerifiedAccounts)]
struct InitOne {
    #[account(init, payer = payer, space = 0)]
    new: u8,
    #[account(mut, signer)]
    payer: u8,
    system_program: u8,
}

/// close an account. Accounts: [target, dest].
#[derive(VerifiedAccounts)]
struct CloseOne {
    #[account(close = dest)]
    target: u8,
    #[account(mut)]
    dest: u8,
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
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
