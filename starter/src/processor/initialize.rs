//! `InitializeHook`: atomically create the config and the validation list of one mint.

use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
};
use spl_tlv_account_resolution::state::ExtraAccountMetaList;
use spl_transfer_hook_interface::instruction::ExecuteInstruction;

use super::common::{
    create_pda_account, read_mint, require_hook_program, require_signer, require_system_program,
    require_uninitialized, require_writable,
};
use crate::{
    config::HookConfig,
    constants::{CONFIG_SEED, VALIDATION_LIST_LEN, VALIDATION_LIST_SEED},
    error::HookError,
    instruction::InitializeHookArgs,
    pda::{config_address, config_extra_account_meta, validation_list_address},
    rule,
};

pub(super) fn process_initialize_hook(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let args = InitializeHookArgs::unpack(instruction_data)?;
    let [config, validation_list, mint, authority, payer, system_program] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    require_system_program(system_program)?;
    require_signer(payer)?;
    require_writable(payer)?;
    require_signer(authority)?;
    require_writable(config)?;
    require_writable(validation_list)?;

    let mint_info = read_mint(mint)?;
    require_hook_program(&mint_info, program_id)?;

    // Only the mint's live Transfer Hook authority may set the hook up, and only once.
    let required_authority = mint_info
        .extension_authority
        .ok_or(HookError::AuthorityUnavailable)?;
    if required_authority != *authority.key {
        return Err(HookError::AuthorityMismatch.into());
    }
    // `unpack` already bounded the params length.
    rule::validate_params(&args.params)?;

    let (expected_config, bump) = config_address(mint.key, program_id);
    if config.key != &expected_config {
        return Err(HookError::InvalidConfigPda.into());
    }
    let (expected_list, list_bump) = validation_list_address(mint.key, program_id);
    if validation_list.key != &expected_list {
        return Err(HookError::InvalidValidationList.into());
    }
    require_uninitialized(config, program_id, HookError::InvalidConfigOwner)?;
    require_uninitialized(
        validation_list,
        program_id,
        HookError::InvalidValidationList,
    )?;

    let state = HookConfig::new(bump, list_bump, *mint.key, &args.params)?;
    let list_meta = config_extra_account_meta()?;

    let bump_seed = [bump];
    create_pda_account(
        payer,
        config,
        system_program,
        program_id,
        state.account_len(),
        &[CONFIG_SEED, mint.key.as_ref(), &bump_seed],
    )?;
    let list_bump_seed = [list_bump];
    create_pda_account(
        payer,
        validation_list,
        system_program,
        program_id,
        VALIDATION_LIST_LEN,
        &[VALIDATION_LIST_SEED, mint.key.as_ref(), &list_bump_seed],
    )?;

    state.encode_into(&mut config.try_borrow_mut_data()?)?;
    ExtraAccountMetaList::init::<ExecuteInstruction>(
        &mut validation_list.try_borrow_mut_data()?,
        &[list_meta],
    )
}
