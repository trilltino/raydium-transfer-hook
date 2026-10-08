//! The `Execute` prelude: every check a hook must make before it looks at its own rule.

use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};
use spl_tlv_account_resolution::state::ExtraAccountMetaList;
use spl_transfer_hook_interface::instruction::ExecuteInstruction;

use crate::{
    accounts::validation_list_address,
    error::KitError,
    mint::{read_hook_mint, require_hook_program, HookMint},
    token::{read_token_account, TokenView},
};

/// The SPL `Execute` instruction discriminator.
pub const EXECUTE_DISCRIMINATOR: [u8; 8] = [105, 37, 101, 197, 75, 251, 102, 26];

/// The validated inputs of an `Execute` call.
pub struct ExecuteCtx<'a, 'info> {
    pub amount: u64,
    pub source: &'a AccountInfo<'info>,
    pub mint: &'a AccountInfo<'info>,
    pub destination: &'a AccountInfo<'info>,
    pub owner: &'a AccountInfo<'info>,
    /// The hook-specific accounts, in the order the validation list declares them.
    pub extras: &'a [AccountInfo<'info>],
    pub hook_mint: HookMint,
    /// Post-transfer views (Token-2022 moves the tokens before it calls the hook).
    pub source_view: TokenView,
    pub destination_view: TokenView,
}

/// Whether `instruction_data` is an `Execute` call, and its transfer amount.
pub fn parse_execute_amount(instruction_data: &[u8]) -> Result<u64, ProgramError> {
    if instruction_data.len() != 16 || instruction_data[..8] != EXECUTE_DISCRIMINATOR {
        return Err(ProgramError::InvalidInstructionData);
    }
    let bytes: [u8; 8] = instruction_data[8..16]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    Ok(u64::from_le_bytes(bytes))
}

/// Validate an `Execute` call for a hook that declares `extra_count` extra accounts.
///
/// Checks, in order: the instruction data and exact account count; the mint is Token-2022, carries
/// a TransferHook extension and points at `program_id`; both token accounts belong to the mint and
/// carry the `transferring` flag (only Token-2022 sets it, so a direct call is refused); the
/// validation list is the canonical address, owned by `program_id`, of the exact size, and the
/// account list matches what it declares.
///
/// Verifying the extra accounts themselves (addresses, owners, writability) is the caller's job.
pub fn execute_prelude<'a, 'info>(
    program_id: &Pubkey,
    accounts: &'a [AccountInfo<'info>],
    instruction_data: &[u8],
    extra_count: usize,
) -> Result<ExecuteCtx<'a, 'info>, ProgramError> {
    let amount = parse_execute_amount(instruction_data)?;
    if accounts.len() != 5 + extra_count {
        return Err(KitError::WrongAccountCount.into());
    }
    let (source, mint, destination, owner, validation_list) = (
        &accounts[0],
        &accounts[1],
        &accounts[2],
        &accounts[3],
        &accounts[4],
    );
    let hook_mint = read_hook_mint(mint)?;
    require_hook_program(&hook_mint, program_id)?;

    let source_view = read_token_account(source)?;
    let destination_view = read_token_account(destination)?;
    if source_view.mint != *mint.key || destination_view.mint != *mint.key {
        return Err(KitError::TokenAccountMismatch.into());
    }
    if !source_view.transferring || !destination_view.transferring {
        return Err(KitError::NotDirectInvocation.into());
    }

    let (expected_list, _) = validation_list_address(mint.key, program_id);
    if validation_list.key != &expected_list || validation_list.owner != program_id {
        return Err(KitError::InvalidValidationList.into());
    }
    {
        let list_data = validation_list.try_borrow_data()?;
        if list_data.len() != ExtraAccountMetaList::size_of(extra_count)? {
            return Err(KitError::InvalidValidationList.into());
        }
        ExtraAccountMetaList::check_account_infos::<ExecuteInstruction>(
            accounts,
            instruction_data,
            program_id,
            &list_data,
        )
        .map_err(|_| KitError::WrongAccountCount)?;
    }

    Ok(ExecuteCtx {
        amount,
        source,
        mint,
        destination,
        owner,
        extras: &accounts[5..],
        hook_mint,
        source_view,
        destination_view,
    })
}
