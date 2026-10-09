//! Instruction encoding: arguments, pack/unpack and the client-side builder for the one
//! instruction the hook has besides `Execute`. The handlers live in `processor`.

use solana_program::{
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
};

use crate::{config::max_transfer_params, constants::*, error::HookError, pda::*};

/// Arguments of `InitializeHook`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitializeHookArgs {
    /// Your rule's settings, at most [`MAX_PARAMS_LEN`] bytes. `rule::validate_params` checks them.
    pub params: Vec<u8>,
}

impl InitializeHookArgs {
    /// The default rule with the given limit.
    pub fn max_transfer(limit: u64) -> Self {
        InitializeHookArgs {
            params: max_transfer_params(limit).to_vec(),
        }
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(INITIALIZE_HOOK_FIXED_LEN + self.params.len());
        data.extend_from_slice(&INITIALIZE_HOOK_DISCRIMINATOR);
        // Saturate instead of wrapping: an oversized `params` then fails on-chain with
        // `ParamsTooLarge` rather than being re-read as a shorter, different instruction.
        let params_len = u16::try_from(self.params.len()).unwrap_or(u16::MAX);
        data.extend_from_slice(&params_len.to_le_bytes());
        data.extend_from_slice(&self.params);
        data
    }

    pub(crate) fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        if data.len() < INITIALIZE_HOOK_FIXED_LEN {
            return Err(ProgramError::InvalidInstructionData);
        }
        let params_len = usize::from(u16::from_le_bytes([data[8], data[9]]));
        if params_len > MAX_PARAMS_LEN {
            return Err(HookError::ParamsTooLarge.into());
        }
        if data.len() != INITIALIZE_HOOK_FIXED_LEN + params_len {
            return Err(ProgramError::InvalidInstructionData);
        }
        Ok(InitializeHookArgs {
            params: data[INITIALIZE_HOOK_FIXED_LEN..].to_vec(),
        })
    }
}

/// Atomically create the config and validation list of `mint`. `authority` must be the mint's
/// live Transfer Hook extension authority.
pub fn initialize_hook_instruction(
    program_id: Pubkey,
    mint: Pubkey,
    authority: Pubkey,
    payer: Pubkey,
    args: &InitializeHookArgs,
) -> Instruction {
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(config_address(&mint, &program_id).0, false),
            AccountMeta::new(validation_list_address(&mint, &program_id).0, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(solana_program::system_program::id(), false),
        ],
        data: args.pack(),
    }
}
