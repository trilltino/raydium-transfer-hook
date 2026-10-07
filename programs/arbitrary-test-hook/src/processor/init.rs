//! The init instruction: create the policy, stats and validation-list accounts atomically.

use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
};
use spl_tlv_account_resolution::state::ExtraAccountMetaList;
use spl_transfer_hook_interface::instruction::ExecuteInstruction;

use super::common::*;
use crate::{
    constants::*,
    error::ArbError,
    pda::*,
    state::{Policy, Stats},
};

pub(super) fn process_init(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.len() != 8 + 4 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let max_per_slot = u32::from_le_bytes(
        instruction_data[8..12]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    if max_per_slot == 0 {
        return Err(ArbError::InvalidParams.into());
    }
    let [payer, mint, authority, policy, stats, validation_list, system_program] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if system_program.key != &solana_program::system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !payer.is_signer || !payer.is_writable || !authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let info = read_mint(mint)?;
    if info.hook_program != Some(*program_id) {
        return Err(ArbError::MintHookMismatch.into());
    }
    if info.extension_authority != Some(*authority.key) {
        return Err(ArbError::AuthorityMismatch.into());
    }
    let (policy_key, policy_bump) = policy_address(mint.key, program_id);
    let (stats_key, stats_bump) = stats_address(mint.key, program_id);
    let (list_key, list_bump) = validation_list_address(mint.key, program_id);
    if policy.key != &policy_key {
        return Err(ArbError::InvalidPolicy.into());
    }
    if stats.key != &stats_key {
        return Err(ArbError::InvalidStats.into());
    }
    if validation_list.key != &list_key {
        return Err(ArbError::InvalidValidationList.into());
    }
    if ![policy, stats, validation_list]
        .iter()
        .all(|a| a.is_writable)
    {
        return Err(ProgramError::InvalidArgument);
    }

    create_pda(
        payer,
        policy,
        system_program,
        program_id,
        POLICY_LEN,
        &[POLICY_SEED, mint.key.as_ref(), &[policy_bump]],
    )?;
    create_pda(
        payer,
        stats,
        system_program,
        program_id,
        STATS_LEN,
        &[STATS_SEED, mint.key.as_ref(), &[stats_bump]],
    )?;
    let list_len = ExtraAccountMetaList::size_of(EXTRA_ACCOUNTS)?;
    create_pda(
        payer,
        validation_list,
        system_program,
        program_id,
        list_len,
        &[VALIDATION_LIST_SEED, mint.key.as_ref(), &[list_bump]],
    )?;

    Policy {
        bump: policy_bump,
        mint: *mint.key,
        max_per_slot,
    }
    .encode_into(&mut policy.try_borrow_mut_data()?)?;
    Stats {
        bump: stats_bump,
        slot: 0,
        count: 0,
        total: 0,
    }
    .encode_into(&mut stats.try_borrow_mut_data()?)?;
    ExtraAccountMetaList::init::<ExecuteInstruction>(
        &mut validation_list.try_borrow_mut_data()?,
        &extra_account_metas()?,
    )
}
