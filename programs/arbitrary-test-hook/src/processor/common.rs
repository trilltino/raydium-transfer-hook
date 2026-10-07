//! Mint reads and PDA creation shared by the processors.

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
use spl_token_2022::{
    extension::{
        transfer_hook::{get_program_id, TransferHook},
        BaseStateWithExtensions, StateWithExtensions,
    },
    state::Mint,
};

use crate::error::ArbError;

pub(crate) struct MintInfo {
    pub(crate) hook_program: Option<Pubkey>,
    pub(crate) extension_authority: Option<Pubkey>,
}

pub(crate) fn read_mint(mint: &AccountInfo) -> Result<MintInfo, ProgramError> {
    if mint.owner != &spl_token_2022::id() {
        return Err(ArbError::MintNotToken2022.into());
    }
    let data = mint.try_borrow_data()?;
    let state = StateWithExtensions::<Mint>::unpack(&data)?;
    let extension = state
        .get_extension::<TransferHook>()
        .map_err(|_| ArbError::MintHookMismatch)?;
    Ok(MintInfo {
        hook_program: get_program_id(&state),
        extension_authority: Option::<Pubkey>::from(extension.authority),
    })
}

pub(crate) fn create_pda<'a>(
    payer: &AccountInfo<'a>,
    account: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    program_id: &Pubkey,
    space: usize,
    seeds: &[&[u8]],
) -> ProgramResult {
    if account.owner != &solana_program::system_program::id() || !account.data_is_empty() {
        return Err(ArbError::AlreadyInitialized.into());
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
        // Someone pre-funded the deterministic address: top up, allocate and assign instead.
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
