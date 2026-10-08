//! `Execute`: settle the two holders a transfer touches, using [`crate::rule`].

use hook_kit::execute_prelude;
use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey};

use super::common::{load_global, load_record, now};
use crate::{
    error::HolderRewardsError,
    state::{record_address, Record},
};

/// The hook declares three extra accounts: the global, the source's record, the destination's.
const EXTRA_ACCOUNTS: usize = 3;

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let ctx = execute_prelude(program_id, accounts, data, EXTRA_ACCOUNTS)?;
    let [global_account, source_record, destination_record] = ctx.extras else {
        return Err(HolderRewardsError::InvalidGlobal.into());
    };
    // Moving tokens within one account changes nothing.
    if ctx.source.key == ctx.destination.key {
        return Ok(());
    }

    // A record exists once the account has registered; an unregistered account is not counted.
    let registered = |account: &AccountInfo, token_account: &Pubkey| -> bool {
        account.owner == program_id && account.key == &record_address(token_account, program_id).0
    };
    let source_registered = registered(source_record, ctx.source.key);
    let destination_registered = registered(destination_record, ctx.destination.key);
    if !source_registered && !destination_registered {
        return Ok(());
    }

    let mut global = load_global(program_id, ctx.mint.key, global_account)?;
    global.stream.advance(now()?)?;

    // Token-2022 moves the tokens before the hook runs, so the balances read are the ones after.
    let amount = ctx.amount;
    if source_registered {
        let mut record = load_record(program_id, ctx.source.key, source_record)?;
        let after = ctx.source_view.amount;
        let before = after
            .checked_add(amount)
            .ok_or(HolderRewardsError::MathOverflow)?;
        record
            .holder
            .on_balance_change(&mut global.stream, before, after)?;
        write_record(&record, source_record)?;
    }
    if destination_registered {
        let mut record = load_record(program_id, ctx.destination.key, destination_record)?;
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
