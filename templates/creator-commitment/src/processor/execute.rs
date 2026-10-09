//! `Execute`: the shared checks, then the rule from [`crate::rule`].

use hook_kit::execute_prelude;
use solana_program::{
    account_info::AccountInfo, clock::Clock, entrypoint::ProgramResult, pubkey::Pubkey,
    sysvar::Sysvar,
};

use crate::{
    config::{Config, VALIDATION_LIST},
    error::CommitmentError,
    rule::check_outgoing,
};

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let ctx = execute_prelude(program_id, accounts, data, &VALIDATION_LIST)?;

    // The only program-owned account `Initialize` ever creates for a mint is its config, at the
    // address Token-2022 resolved from the validation list, so ownership and the stored mint
    // identify it; no address needs deriving here.
    let config_account = &ctx.extras[0];
    if config_account.owner != program_id {
        return Err(CommitmentError::InvalidConfig.into());
    }
    let config = Config::decode(&config_account.try_borrow_data()?)?;
    if config.mint != *ctx.mint.key {
        return Err(CommitmentError::InvalidConfig.into());
    }

    // The rule only concerns tokens leaving the locked account.
    if ctx.source.key != &config.creator_account {
        return Ok(());
    }
    let now = Clock::get()?.unix_timestamp;
    check_outgoing(&config.schedule, now, ctx.source_view.amount)?;
    Ok(())
}
