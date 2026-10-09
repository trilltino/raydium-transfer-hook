//! Checks shared by the processors: mint and authority reads, account guards, PDA creation.

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

use crate::error::HookError;

/// Read `N` bytes of instruction data at `offset`, or fail with `InvalidInstructionData`.
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

pub(crate) struct MintHookInfo {
    pub(crate) hook_program: Option<Pubkey>,
    pub(crate) extension_authority: Option<Pubkey>,
}

/// Mint must be Token-2022 owned and carry a TransferHook extension.
pub(crate) fn read_mint(mint: &AccountInfo) -> Result<MintHookInfo, ProgramError> {
    if mint.owner != &spl_token_2022::id() {
        return Err(HookError::MintOwnerNotToken2022.into());
    }
    let data = mint.try_borrow_data()?;
    let state = StateWithExtensions::<Mint>::unpack(&data)?;
    let extension = state
        .get_extension::<TransferHook>()
        .map_err(|_| HookError::MintHookExtensionMissing)?;
    Ok(MintHookInfo {
        hook_program: get_program_id(&state),
        extension_authority: Option::<Pubkey>::from(extension.authority),
    })
}

pub(crate) fn require_hook_program(info: &MintHookInfo, program_id: &Pubkey) -> ProgramResult {
    if info.hook_program != Some(*program_id) {
        return Err(HookError::MintHookProgramMismatch.into());
    }
    Ok(())
}

pub(crate) fn require_signer(account: &AccountInfo) -> ProgramResult {
    if !account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    Ok(())
}

pub(crate) fn require_writable(account: &AccountInfo) -> ProgramResult {
    if !account.is_writable {
        return Err(ProgramError::Immutable);
    }
    Ok(())
}

pub(crate) fn require_system_program(account: &AccountInfo) -> ProgramResult {
    if account.key != &solana_program::system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    Ok(())
}

/// A PDA that is about to be created must be unowned: system owned and empty.
pub(crate) fn require_uninitialized(
    account: &AccountInfo,
    program_id: &Pubkey,
    foreign_owner_error: HookError,
) -> ProgramResult {
    if account.owner == program_id {
        return Err(HookError::AlreadyInitialized.into());
    }
    if account.owner != &solana_program::system_program::id() || !account.data_is_empty() {
        return Err(foreign_owner_error.into());
    }
    Ok(())
}

/// Create a program-owned PDA account. Survives a pre-funded PDA (anyone can send lamports to a
/// deterministic address): in that case top up to rent exemption, then allocate and assign.
pub(crate) fn create_pda_account<'a>(
    payer: &AccountInfo<'a>,
    account: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    program_id: &Pubkey,
    space: usize,
    signer_seeds: &[&[u8]],
) -> ProgramResult {
    if account.owner != &solana_program::system_program::id() || !account.data_is_empty() {
        return Err(HookError::AlreadyInitialized.into());
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
            &[signer_seeds],
        )
    } else {
        let missing = required.saturating_sub(current);
        if missing > 0 {
            invoke(
                &system_instruction::transfer(payer.key, account.key, missing),
                &[payer.clone(), account.clone(), system_program.clone()],
            )?;
        }
        invoke_signed(
            &system_instruction::allocate(account.key, space_u64),
            &[account.clone(), system_program.clone()],
            &[signer_seeds],
        )?;
        invoke_signed(
            &system_instruction::assign(account.key, program_id),
            &[account.clone(), system_program.clone()],
            &[signer_seeds],
        )
    }
}
