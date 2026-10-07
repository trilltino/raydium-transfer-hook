//! Client-side instruction encoding: arguments and builders for every instruction the hook
//! accepts. The on-chain decoding lives in `processor`.

use solana_program::{
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
};

use crate::{
    authority::AuthorityMode, config::max_transfer_params, constants::*, error::HookError, pda::*,
};

/// Arguments of `InitializeHook`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitializeHookArgs {
    pub authority_mode: u8,
    pub template_id: [u8; 32],
    pub template_version: u32,
    pub flags: u64,
    /// Must be non-zero in mode 2 and zero otherwise.
    pub config_authority: Pubkey,
    pub params: Vec<u8>,
}

impl InitializeHookArgs {
    /// A `max-transfer-v1` hook with the given limit.
    pub fn max_transfer(mode: AuthorityMode, limit: u64, config_authority: Pubkey) -> Self {
        InitializeHookArgs {
            authority_mode: mode as u8,
            template_id: TEMPLATE_MAX_TRANSFER_V1,
            template_version: MAX_TRANSFER_TEMPLATE_VERSION,
            flags: 0,
            config_authority,
            params: max_transfer_params(limit).to_vec(),
        }
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(INITIALIZE_HOOK_FIXED_LEN + self.params.len());
        data.extend_from_slice(&INITIALIZE_HOOK_DISCRIMINATOR);
        data.push(self.authority_mode);
        data.extend_from_slice(&self.template_version.to_le_bytes());
        data.extend_from_slice(&self.flags.to_le_bytes());
        data.extend_from_slice(&self.template_id);
        data.extend_from_slice(self.config_authority.as_ref());
        data.extend_from_slice(&(self.params.len() as u16).to_le_bytes());
        data.extend_from_slice(&self.params);
        data
    }

    pub(crate) fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        if data.len() < INITIALIZE_HOOK_FIXED_LEN {
            return Err(ProgramError::InvalidInstructionData);
        }
        let params_len = usize::from(u16::from_le_bytes([data[85], data[86]]));
        if params_len > MAX_PARAMS_LEN {
            return Err(HookError::ParamsTooLarge.into());
        }
        if data.len() != INITIALIZE_HOOK_FIXED_LEN + params_len {
            return Err(ProgramError::InvalidInstructionData);
        }
        Ok(InitializeHookArgs {
            authority_mode: data[8],
            template_version: u32::from_le_bytes(ix_array(data, 9)?),
            flags: u64::from_le_bytes(ix_array(data, 13)?),
            template_id: ix_array(data, 21)?,
            config_authority: Pubkey::new_from_array(ix_array(data, 53)?),
            params: data[INITIALIZE_HOOK_FIXED_LEN..].to_vec(),
        })
    }
}

pub(crate) fn ix_array<const N: usize>(
    data: &[u8],
    offset: usize,
) -> Result<[u8; N], ProgramError> {
    offset
        .checked_add(N)
        .and_then(|end| data.get(offset..end))
        .and_then(|slice| slice.try_into().ok())
        .ok_or(ProgramError::InvalidInstructionData)
}

/// Atomically create the config and validation list of `mint`.
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

pub fn update_config_instruction(
    program_id: Pubkey,
    mint: Pubkey,
    authority: Pubkey,
    expected_seq: u64,
    template_version: u32,
    flags: u64,
    params: &[u8],
) -> Instruction {
    let mut data = Vec::with_capacity(UPDATE_CONFIG_FIXED_LEN + params.len());
    data.extend_from_slice(&UPDATE_CONFIG_DISCRIMINATOR);
    data.extend_from_slice(&expected_seq.to_le_bytes());
    data.extend_from_slice(&template_version.to_le_bytes());
    data.extend_from_slice(&flags.to_le_bytes());
    data.extend_from_slice(&(params.len() as u16).to_le_bytes());
    data.extend_from_slice(params);
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(config_address(&mint, &program_id).0, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(authority, true),
        ],
        data,
    }
}

pub fn set_config_authority_instruction(
    program_id: Pubkey,
    mint: Pubkey,
    authority: Pubkey,
    new_authority: Pubkey,
) -> Instruction {
    let mut data = Vec::with_capacity(40);
    data.extend_from_slice(&SET_CONFIG_AUTHORITY_DISCRIMINATOR);
    data.extend_from_slice(new_authority.as_ref());
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(config_address(&mint, &program_id).0, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(authority, true),
        ],
        data,
    }
}
