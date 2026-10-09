//! `Reconcile`: correct a record whose balance fell without a transfer (a burn, or a closed token
//! account). Permissionless: it only ever lowers a stale count, so anyone may call it.

use hook_kit::{read_token_account, KitError};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    program_error::ProgramError,
    pubkey::Pubkey,
};

use super::common::{load_global, now};
use crate::{error::HolderRewardsError, state::Record};

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let token_account = next_account_info(accounts_iter)?;
    let record_account = next_account_info(accounts_iter)?;
    let global_account = next_account_info(accounts_iter)?;

    if record_account.owner != program_id {
        return Err(HolderRewardsError::NotRegistered.into());
    }
    let mut record = Record::decode(&record_account.try_borrow_data()?)?;
    if record.token_account != *token_account.key {
        return Err(HolderRewardsError::InvalidRecord.into());
    }
    // The record names its mint, so the stream is found even when the token account is gone.
    let mut global = load_global(program_id, &record.mint, global_account)?;
    let balance_now = current_balance(token_account, &record.mint)?;

    global.stream.advance(now()?)?;
    record.holder.reconcile(&mut global.stream, balance_now)?;

    record.encode_into(&mut record_account.try_borrow_mut_data()?)?;
    global.encode_into(&mut global_account.try_borrow_mut_data()?)?;
    Ok(())
}

/// The token account's balance, or `0` if it has been closed (an empty, system-owned address).
/// A live account must still be a token account of `mint`.
fn current_balance(account: &AccountInfo, mint: &Pubkey) -> Result<u64, ProgramError> {
    if account.owner == &solana_program::system_program::id() && account.data_is_empty() {
        return Ok(0);
    }
    let view = read_token_account(account)?;
    if view.mint != *mint {
        return Err(KitError::TokenAccountMismatch.into());
    }
    Ok(view.amount)
}
