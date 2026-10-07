//! On-chain instruction processing, one module per operation.

mod common;
mod execute;
mod initialize;
mod mutate;

#[cfg(test)]
pub(crate) use execute::validate_list_layout;

use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
};

use crate::constants::*;
use crate::error::HookError;

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
    } else if discriminator == INITIALIZE_HOOK_DISCRIMINATOR {
        initialize::process_initialize_hook(program_id, accounts, instruction_data)
    } else if discriminator == UPDATE_CONFIG_DISCRIMINATOR {
        mutate::process_update_config(program_id, accounts, instruction_data)
    } else if discriminator == SET_CONFIG_AUTHORITY_DISCRIMINATOR {
        mutate::process_set_config_authority(program_id, accounts, instruction_data)
    } else if discriminator == SPL_INITIALIZE_LIST_DISCRIMINATOR
        || discriminator == SPL_UPDATE_LIST_DISCRIMINATOR
    {
        Err(HookError::SplInterfaceUnsupported.into())
    } else {
        Err(ProgramError::InvalidInstructionData)
    }
}
