//! On-chain instruction processing.

mod common;
mod execute;
mod init;

use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
};

use crate::constants::*;

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let Some(discriminator) = instruction_data.get(..8) else {
        return Err(ProgramError::InvalidInstructionData);
    };
    if discriminator == EXECUTE_DISCRIMINATOR {
        execute::process_execute(program_id, accounts, instruction_data)
    } else if discriminator == INIT_DISCRIMINATOR {
        init::process_init(program_id, accounts, instruction_data)
    } else {
        Err(ProgramError::InvalidInstructionData)
    }
}
