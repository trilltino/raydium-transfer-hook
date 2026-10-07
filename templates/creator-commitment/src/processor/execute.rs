//! `Execute`: the shared checks, then the rule from [`crate::rule`].

use hook_kit::execute_prelude;
use solana_program::{
    account_info::AccountInfo, clock::Clock, entrypoint::ProgramResult, pubkey::Pubkey,
    sysvar::Sysvar,
};

use crate::{
    config::{config_address, Config},
    error::CommitmentError,
    rule::check_outgoing,
};

/// The hook declares one extra account: the mint's config.
const EXTRA_ACCOUNTS: usize = 1;

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let ctx = execute_prelude(program_id, accounts, data, EXTRA_ACCOUNTS)?;

    let config_account = &ctx.extras[0];
    if config_account.owner != program_id
        || config_account.key != &config_address(ctx.mint.key, program_id).0
    {
        return Err(CommitmentError::InvalidConfig.into());
    }
    let config = Config::decode(&config_account.try_borrow_data()?)?;

    // The rule only concerns tokens leaving the locked account.
    if ctx.source.key != &config.creator_account {
        return Ok(());
    }
    let now = Clock::get()?.unix_timestamp;
    check_outgoing(&config.schedule, now, ctx.source_view.amount)?;
    Ok(())
}
