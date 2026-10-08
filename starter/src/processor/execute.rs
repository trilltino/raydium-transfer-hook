//! `Execute`: what Token-2022 calls on every transfer of a hooked mint.

use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
};
use spl_tlv_account_resolution::state::ExtraAccountMetaList;
use spl_token_2022::{
    extension::{transfer_hook::TransferHookAccount, BaseStateWithExtensions, StateWithExtensions},
    state::Account as TokenAccount,
};
use spl_transfer_hook_interface::instruction::ExecuteInstruction;

use super::common::*;
use crate::{
    config::HookConfig, constants::*, error::HookError, instruction::ix_array, rule,
    TransferContext,
};

/// The token account must belong to `mint` and be mid-transfer (flag set by Token-2022 only).
fn require_transferring(account: &AccountInfo, mint: &Pubkey) -> ProgramResult {
    if account.owner != &spl_token_2022::id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    let data = account.try_borrow_data()?;
    let state = StateWithExtensions::<TokenAccount>::unpack(&data)?;
    if state.base.mint != *mint {
        return Err(HookError::AccountOrderMismatch.into());
    }
    let transferring = state
        .get_extension::<TransferHookAccount>()
        .map(|extension| bool::from(extension.transferring))
        .unwrap_or(false);
    if !transferring {
        return Err(HookError::NotDirectInvocation.into());
    }
    Ok(())
}

/// Validate the list by hand so a corrupt list returns an error instead of panicking inside
/// `ExtraAccountMetaList::check_account_infos` (which unwraps and subtracts unchecked).
pub(crate) fn validate_list_layout(data: &[u8]) -> ProgramResult {
    if data.len() != VALIDATION_LIST_LEN
        || data[..8] != EXECUTE_DISCRIMINATOR
        || data[8..12] != ((4 + 35) as u32).to_le_bytes()
        || data[12..16] != 1u32.to_le_bytes()
    {
        return Err(HookError::InvalidValidationList.into());
    }
    Ok(())
}

pub(super) fn process_execute(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.len() != 16 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let amount = u64::from_le_bytes(ix_array(instruction_data, 8)?);
    if accounts.len() != EXECUTE_ACCOUNT_COUNT {
        return Err(HookError::WrongAccountCount.into());
    }
    let [source, mint, destination, owner, validation_list, config] = accounts else {
        return Err(HookError::WrongAccountCount.into());
    };
    // Token-2022 builds the Execute CPI with every account read-only, so none of them may be
    // writable. The owner/authority slot is exempt: on a top-level (forged) call its writability
    // is the transaction-level flag, which is true for the fee payer, and such calls are rejected
    // by the transferring-flag guard below anyway.
    if [source, mint, destination, validation_list, config]
        .iter()
        .any(|account| account.is_writable)
    {
        return Err(HookError::AccountOrderMismatch.into());
    }

    let mint_info = read_mint(mint)?;
    require_hook_program(&mint_info, program_id)?;
    // Both sides must carry the transferring flag, which only Token-2022 sets during a transfer.
    require_transferring(source, mint.key)?;
    require_transferring(destination, mint.key)?;

    if config.owner != program_id {
        return Err(HookError::InvalidConfigOwner.into());
    }
    let state = HookConfig::decode(&config.try_borrow_data()?)?;
    state.verify_address(program_id, mint.key, config.key)?;

    let expected_list = Pubkey::create_program_address(
        &[VALIDATION_LIST_SEED, mint.key.as_ref(), &[state.list_bump]],
        program_id,
    )
    .map_err(|_| HookError::InvalidValidationList)?;
    if validation_list.key != &expected_list || validation_list.owner != program_id {
        return Err(HookError::InvalidValidationList.into());
    }
    {
        let list_data = validation_list.try_borrow_data()?;
        validate_list_layout(&list_data)?;
        ExtraAccountMetaList::check_account_infos::<ExecuteInstruction>(
            accounts,
            instruction_data,
            program_id,
            &list_data,
        )
        .map_err(|_| HookError::AccountOrderMismatch)?;
    }

    match state.version {
        1 => {
            let context = TransferContext {
                amount,
                source: *source.key,
                destination: *destination.key,
                mint: *mint.key,
                authority: *owner.key,
            };
            rule::check_transfer(&state, &context).map_err(Into::into)
        }
        _ => Err(HookError::UnsupportedVersion.into()),
    }
}
