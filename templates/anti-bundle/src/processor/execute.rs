//! `Execute`: the shared checks, then the rule from [`crate::rule`].

use hook_kit::execute_prelude;
use solana_program::{
    account_info::AccountInfo, clock::Clock, entrypoint::ProgramResult, pubkey::Pubkey,
    sysvar::Sysvar,
};

use crate::{
    error::AntiBundleError,
    rule::{buys_in_slot_after, check_buy, is_buy},
    state::{config_address, counter_address, Config, Counter},
};

/// The hook declares two extra accounts: the config and the slot counter.
const EXTRA_ACCOUNTS: usize = 2;

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let ctx = execute_prelude(program_id, accounts, data, EXTRA_ACCOUNTS)?;
    let [config_account, counter_account] = ctx.extras else {
        return Err(AntiBundleError::InvalidConfig.into());
    };

    if config_account.owner != program_id
        || config_account.key != &config_address(ctx.mint.key, program_id).0
    {
        return Err(AntiBundleError::InvalidConfig.into());
    }
    let config = Config::decode(&config_account.try_borrow_data()?)?;

    let clock = Clock::get()?;
    // Past the end, and for anything that is not a buy, there is nothing to check.
    if !config.params.is_active(clock.unix_timestamp) || !is_buy(ctx.source.key, &config.venues) {
        return Ok(());
    }

    if counter_account.owner != program_id
        || !counter_account.is_writable
        || counter_account.key != &counter_address(ctx.mint.key, program_id).0
    {
        return Err(AntiBundleError::InvalidState.into());
    }
    let counter = Counter::decode(&counter_account.try_borrow_data()?)?;
    let updated = Counter {
        slot: clock.slot,
        buys: buys_in_slot_after(counter.slot, counter.buys, clock.slot),
        ..counter
    };
    check_buy(&config.params, updated.buys)?;

    updated.encode_into(&mut counter_account.try_borrow_mut_data()?)?;
    Ok(())
}
