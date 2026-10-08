//! Checks and helpers the instructions share.

use solana_program::{
    account_info::AccountInfo, clock::Clock, program_error::ProgramError, pubkey::Pubkey,
    sysvar::Sysvar,
};
use spl_token_2022::{
    extension::StateWithExtensions,
    state::{Account as TokenAccount, Mint},
};

use crate::{
    error::HolderRewardsError,
    state::{global_address, record_address, Global, Record},
};

pub(super) fn now() -> Result<i64, ProgramError> {
    Ok(Clock::get()?.unix_timestamp)
}

/// The global account of `mint`: owned by this program, at its address, for this mint.
pub(super) fn load_global(
    program_id: &Pubkey,
    mint: &Pubkey,
    account: &AccountInfo,
) -> Result<Global, ProgramError> {
    if account.owner != program_id || account.key != &global_address(mint, program_id).0 {
        return Err(HolderRewardsError::InvalidGlobal.into());
    }
    let global = Global::decode(&account.try_borrow_data()?)?;
    if global.mint != *mint {
        return Err(HolderRewardsError::InvalidGlobal.into());
    }
    Ok(global)
}

/// A record account that exists: owned by this program, at the address for `token_account`.
pub(super) fn load_record(
    program_id: &Pubkey,
    token_account: &Pubkey,
    account: &AccountInfo,
) -> Result<Record, ProgramError> {
    if account.owner != program_id || account.key != &record_address(token_account, program_id).0 {
        return Err(HolderRewardsError::InvalidRecord.into());
    }
    let record = Record::decode(&account.try_borrow_data()?)?;
    if record.token_account != *token_account {
        return Err(HolderRewardsError::InvalidRecord.into());
    }
    Ok(record)
}

/// The reward mint's token program must be one of the two SPL token programs, and own the mint.
pub(super) fn require_token_program(
    token_program: &AccountInfo,
    mint: &AccountInfo,
) -> Result<(), ProgramError> {
    let known = token_program.key == &spl_token_2022::id() || token_program.key == &spl_token::id();
    if !known || mint.owner != token_program.key {
        return Err(HolderRewardsError::RewardAccountMismatch.into());
    }
    Ok(())
}

/// The decimals of a reward mint of either token program.
pub(super) fn mint_decimals(mint: &AccountInfo) -> Result<u8, ProgramError> {
    let data = mint.try_borrow_data()?;
    Ok(StateWithExtensions::<Mint>::unpack(&data)?.base.decimals)
}

/// `(mint, owner, amount)` of a token account of either token program.
pub(super) fn token_fields(account: &AccountInfo) -> Result<(Pubkey, Pubkey, u64), ProgramError> {
    let data = account.try_borrow_data()?;
    let state = StateWithExtensions::<TokenAccount>::unpack(&data)?;
    Ok((state.base.mint, state.base.owner, state.base.amount))
}
