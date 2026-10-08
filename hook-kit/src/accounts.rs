//! Creating program-derived accounts and the canonical validation list.

use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    sysvar::Sysvar,
};
use spl_tlv_account_resolution::{account::ExtraAccountMeta, state::ExtraAccountMetaList};
use spl_transfer_hook_interface::instruction::ExecuteInstruction;

use crate::error::KitError;

/// The canonical validation-list address of `mint` under `program_id`.
pub fn validation_list_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    spl_transfer_hook_interface::get_extra_account_metas_address_and_bump_seed(mint, program_id)
}

/// Create the PDA at `account` (seeds `seeds`, including the bump) owned by `program_id`.
///
/// Survives a pre-funded address (anyone can send lamports to a deterministic PDA): it then tops
/// up, allocates and assigns instead of failing.
pub fn create_pda<'a>(
    payer: &AccountInfo<'a>,
    account: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    program_id: &Pubkey,
    space: usize,
    seeds: &[&[u8]],
) -> ProgramResult {
    if account.owner != &solana_program::system_program::id() || !account.data_is_empty() {
        return Err(KitError::AlreadyInitialized.into());
    }
    let required = Rent::get()?.minimum_balance(space);
    let space_u64 = u64::try_from(space).map_err(|_| ProgramError::InvalidArgument)?;
    let current = account.lamports();
    if current == 0 {
        invoke_signed(
            &system_instruction::create_account(
                payer.key,
                account.key,
                required,
                space_u64,
                program_id,
            ),
            &[payer.clone(), account.clone(), system_program.clone()],
            &[seeds],
        )
    } else {
        if let Some(missing) = required.checked_sub(current).filter(|m| *m > 0) {
            invoke(
                &system_instruction::transfer(payer.key, account.key, missing),
                &[payer.clone(), account.clone(), system_program.clone()],
            )?;
        }
        invoke_signed(
            &system_instruction::allocate(account.key, space_u64),
            &[account.clone(), system_program.clone()],
            &[seeds],
        )?;
        invoke_signed(
            &system_instruction::assign(account.key, program_id),
            &[account.clone(), system_program.clone()],
            &[seeds],
        )
    }
}

/// Create the validation list at its canonical address and write `metas` into it.
pub fn create_validation_list<'a>(
    payer: &AccountInfo<'a>,
    list: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    mint: &Pubkey,
    program_id: &Pubkey,
    metas: &[ExtraAccountMeta],
) -> ProgramResult {
    let (expected, bump) = validation_list_address(mint, program_id);
    if list.key != &expected {
        return Err(KitError::InvalidValidationList.into());
    }
    let size = ExtraAccountMetaList::size_of(metas.len())?;
    create_pda(
        payer,
        list,
        system_program,
        program_id,
        size,
        &[b"extra-account-metas", mint.as_ref(), &[bump]],
    )?;
    ExtraAccountMetaList::init::<ExecuteInstruction>(&mut list.try_borrow_mut_data()?, metas)
}
