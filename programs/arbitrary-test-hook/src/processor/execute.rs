//! `Execute`: what Token-2022 calls on every transfer of the hooked mint.

use solana_program::{
    account_info::AccountInfo, clock::Clock, entrypoint::ProgramResult,
    program_error::ProgramError, pubkey::Pubkey, sysvar::Sysvar,
};
use spl_tlv_account_resolution::state::ExtraAccountMetaList;
use spl_token_2022::{
    extension::{transfer_hook::TransferHookAccount, BaseStateWithExtensions, StateWithExtensions},
    state::Account as TokenAccount,
};
use spl_transfer_hook_interface::instruction::ExecuteInstruction;

use super::common::*;
use crate::{
    constants::*,
    error::ArbError,
    pda::*,
    state::{Policy, Stats},
};

fn require_transferring(account: &AccountInfo, mint: &Pubkey) -> ProgramResult {
    if account.owner != &spl_token_2022::id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    let data = account.try_borrow_data()?;
    let state = StateWithExtensions::<TokenAccount>::unpack(&data)?;
    if state.base.mint != *mint {
        return Err(ArbError::WrongAccountCount.into());
    }
    let transferring = state
        .get_extension::<TransferHookAccount>()
        .map(|extension| bool::from(extension.transferring))
        .unwrap_or(false);
    if !transferring {
        return Err(ArbError::NotDirectInvocation.into());
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
    let amount = u64::from_le_bytes(
        instruction_data[8..16]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    if accounts.len() != EXECUTE_ACCOUNT_COUNT {
        return Err(ArbError::WrongAccountCount.into());
    }
    let [source, mint, destination, _owner, validation_list, policy, stats] = accounts else {
        return Err(ArbError::WrongAccountCount.into());
    };
    let info = read_mint(mint)?;
    if info.hook_program != Some(*program_id) {
        return Err(ArbError::MintHookMismatch.into());
    }
    // Only Token-2022 sets the transferring flag, and only for the duration of a transfer.
    require_transferring(source, mint.key)?;
    require_transferring(destination, mint.key)?;

    let (list_key, _) = validation_list_address(mint.key, program_id);
    if validation_list.key != &list_key || validation_list.owner != program_id {
        return Err(ArbError::InvalidValidationList.into());
    }
    {
        let list_data = validation_list.try_borrow_data()?;
        if list_data.len() != ExtraAccountMetaList::size_of(EXTRA_ACCOUNTS)? {
            return Err(ArbError::InvalidValidationList.into());
        }
        ExtraAccountMetaList::check_account_infos::<ExecuteInstruction>(
            accounts,
            instruction_data,
            program_id,
            &list_data,
        )
        .map_err(|_| ArbError::WrongAccountCount)?;
    }

    if policy.owner != program_id || stats.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    let policy_state = Policy::decode(&policy.try_borrow_data()?)?;
    if policy_state.mint != *mint.key
        || Pubkey::create_program_address(
            &[POLICY_SEED, mint.key.as_ref(), &[policy_state.bump]],
            program_id,
        )
        .ok()
            != Some(*policy.key)
    {
        return Err(ArbError::InvalidPolicy.into());
    }
    if !stats.is_writable {
        return Err(ArbError::StatsNotWritable.into());
    }
    let mut stats_state = Stats::decode(&stats.try_borrow_data()?)?;
    if Pubkey::create_program_address(
        &[STATS_SEED, mint.key.as_ref(), &[stats_state.bump]],
        program_id,
    )
    .ok()
        != Some(*stats.key)
    {
        return Err(ArbError::InvalidStats.into());
    }

    let slot = Clock::get()?.slot;
    if stats_state.slot != slot {
        stats_state.slot = slot;
        stats_state.count = 0;
    }
    stats_state.count = stats_state.count.checked_add(1).ok_or(ArbError::Overflow)?;
    if stats_state.count > policy_state.max_per_slot {
        return Err(ArbError::SlotLimitExceeded.into());
    }
    stats_state.total = stats_state
        .total
        .checked_add(amount)
        .ok_or(ArbError::Overflow)?;
    stats_state.encode_into(&mut stats.try_borrow_mut_data()?)?;
    Ok(())
}
