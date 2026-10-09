//! `Execute`: settle the two holders a transfer touches, using [`crate::rule`].

use hook_kit::{execute_prelude, KitError};
use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey};

use super::common::{load_global, load_optional_record, now};
use crate::{
    error::HolderRewardsError,
    state::{global_address, record_address, Record, VALIDATION_LIST},
};

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let ctx = execute_prelude(program_id, accounts, data, &VALIDATION_LIST)?;
    let [global_account, source_record, destination_record] = ctx.extras else {
        return Err(KitError::WrongAccountCount.into());
    };
    // The optional records may still be system-owned. Their contents cannot identify them, so
    // verify the addresses before treating an empty account as an unregistered holder.
    if global_account.key != &global_address(ctx.mint.key, program_id).0 {
        return Err(HolderRewardsError::InvalidGlobal.into());
    }
    if source_record.key != &record_address(ctx.source.key, program_id).0
        || destination_record.key != &record_address(ctx.destination.key, program_id).0
    {
        return Err(HolderRewardsError::InvalidRecord.into());
    }
    // Moving tokens within one account changes nothing.
    if ctx.source.key == ctx.destination.key {
        return Ok(());
    }

    // A record exists once the account has registered; an unregistered account is not counted.
    let source = load_optional_record(program_id, ctx.source.key, source_record)?;
    let destination = load_optional_record(program_id, ctx.destination.key, destination_record)?;
    if source.is_none() && destination.is_none() {
        return Ok(());
    }

    let mut global = load_global(program_id, ctx.mint.key, global_account)?;
    global.stream.advance(now()?)?;

    // Token-2022 moves the tokens before the hook runs, so the balances read are the ones after.
    let amount = ctx.amount;
    if let Some(mut record) = source {
        let after = ctx.source_view.amount;
        let before = after
            .checked_add(amount)
            .ok_or(HolderRewardsError::MathOverflow)?;
        record
            .holder
            .on_balance_change(&mut global.stream, before, after)?;
        write_record(&record, source_record)?;
    }
    if let Some(mut record) = destination {
        let after = ctx.destination_view.amount;
        let before = after
            .checked_sub(amount)
            .ok_or(HolderRewardsError::MathOverflow)?;
        record
            .holder
            .on_balance_change(&mut global.stream, before, after)?;
        write_record(&record, destination_record)?;
    }
    global.encode_into(&mut global_account.try_borrow_mut_data()?)?;
    Ok(())
}

fn write_record(record: &Record, account: &AccountInfo) -> ProgramResult {
    record.encode_into(&mut account.try_borrow_mut_data()?)?;
    Ok(())
}
