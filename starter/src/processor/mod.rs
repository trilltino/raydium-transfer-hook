//! On-chain instruction processing, one module per operation.

mod common;
mod execute;
mod initialize;

use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
};

use crate::{
    constants::{
        EXECUTE_DISCRIMINATOR, INITIALIZE_HOOK_DISCRIMINATOR, SPL_INITIALIZE_LIST_DISCRIMINATOR,
        SPL_UPDATE_LIST_DISCRIMINATOR,
    },
    error::HookError,
};

/// Program entrypoint: route by the 8-byte discriminator.
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let discriminator: [u8; 8] = instruction_data
        .get(..8)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(ProgramError::InvalidInstructionData)?;
    match discriminator {
        EXECUTE_DISCRIMINATOR => execute::process_execute(program_id, accounts, instruction_data),
        INITIALIZE_HOOK_DISCRIMINATOR => {
            initialize::process_initialize_hook(program_id, accounts, instruction_data)
        }
        SPL_INITIALIZE_LIST_DISCRIMINATOR | SPL_UPDATE_LIST_DISCRIMINATOR => {
            Err(HookError::SplInterfaceUnsupported.into())
        }
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
