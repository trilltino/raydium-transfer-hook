//! `UpdateConfig` and `SetConfigAuthority`: the only instructions that change an existing config.

use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
};

use super::common::*;
use crate::{
    authority::AuthorityMode, config::HookConfig, constants::*, error::HookError,
    instruction::ix_array,
};

/// Ordered checks shared by every mutating instruction: config owner, discriminator and version
/// (decode), mint match, bump-derived address, mode, signer, and that the mint still points to
/// this program. Returns the decoded config.
fn authorize_mutation(
    program_id: &Pubkey,
    config: &AccountInfo,
    mint: &AccountInfo,
    authority: &AccountInfo,
    allow_modes: fn(AuthorityMode) -> Result<(), HookError>,
) -> Result<HookConfig, ProgramError> {
    require_writable(config)?;
    if config.owner != program_id {
        return Err(HookError::InvalidConfigOwner.into());
    }
    let state = HookConfig::decode(&config.try_borrow_data()?)?;
    state.verify_address(program_id, mint.key, config.key)?;
    if state.authority_mode == AuthorityMode::Immutable {
        return Err(HookError::ConfigImmutable.into());
    }
    allow_modes(state.authority_mode)?;
    require_signer(authority)?;
    let mint_info = read_mint(mint)?;
    let required = match state.authority_mode {
        AuthorityMode::ExtensionAuthority => mint_info.extension_authority,
        AuthorityMode::MintAuthority => mint_info.mint_authority,
        AuthorityMode::Explicit => Some(state.config_authority),
        AuthorityMode::Immutable => return Err(HookError::ConfigImmutable.into()),
    }
    .ok_or(HookError::AuthorityUnavailable)?;
    if required != *authority.key {
        return Err(HookError::AuthorityMismatch.into());
    }
    require_hook_program(&mint_info, program_id)?;
    Ok(state)
}

pub(super) fn process_update_config(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.len() < UPDATE_CONFIG_FIXED_LEN {
        return Err(ProgramError::InvalidInstructionData);
    }
    let params_len = usize::from(u16::from_le_bytes([
        instruction_data[28],
        instruction_data[29],
    ]));
    if params_len > MAX_PARAMS_LEN {
        return Err(HookError::ParamsTooLarge.into());
    }
    if instruction_data.len() != UPDATE_CONFIG_FIXED_LEN + params_len {
        return Err(ProgramError::InvalidInstructionData);
    }
    let expected_seq = u64::from_le_bytes(ix_array(instruction_data, 8)?);
    let template_version = u32::from_le_bytes(ix_array(instruction_data, 16)?);
    let flags = u64::from_le_bytes(ix_array(instruction_data, 20)?);
    let params = &instruction_data[UPDATE_CONFIG_FIXED_LEN..];

    let [config, mint, authority] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let current = authorize_mutation(program_id, config, mint, authority, |_| Ok(()))?;
    if current.config_seq != expected_seq {
        return Err(HookError::StaleConfigSeq.into());
    }
    validate_template(&current.template_id, template_version, flags, params)?;
    // Config size is fixed per template: `max-transfer-v1` params are always 8 bytes.
    if params.len() != current.params().len() {
        return Err(HookError::InvalidParams.into());
    }

    let mut next = HookConfig::new(
        current.bump,
        current.list_bump,
        current.authority_mode,
        current.template_id,
        template_version,
        current.mint,
        current.config_authority,
        flags,
        params,
    )?;
    next.config_seq = current
        .config_seq
        .checked_add(1)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    next.encode_into(&mut config.try_borrow_mut_data()?)?;
    Ok(())
}

pub(super) fn process_set_config_authority(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.len() != 40 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let new_authority = Pubkey::new_from_array(ix_array(instruction_data, 8)?);
    let [config, mint, authority] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let current = authorize_mutation(program_id, config, mint, authority, |mode| match mode {
        AuthorityMode::Explicit => Ok(()),
        _ => Err(HookError::UnsupportedMode),
    })?;
    let mut next = current;
    if new_authority == Pubkey::default() {
        // One-way transition to Immutable.
        next.authority_mode = AuthorityMode::Immutable;
        next.config_authority = Pubkey::default();
    } else {
        next.config_authority = new_authority;
    }
    next.config_seq = current
        .config_seq
        .checked_add(1)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    next.encode_into(&mut config.try_borrow_mut_data()?)?;
    Ok(())
}
