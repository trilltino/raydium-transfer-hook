//! `Execute`: the shared checks, then the rule from [`crate::rule`].

use hook_kit::execute_prelude;
use solana_program::{
    account_info::AccountInfo,
    clock::Clock,
    entrypoint::ProgramResult,
    program_error::ProgramError,
    pubkey,
    pubkey::Pubkey,
    sysvar::{self, instructions::load_instruction_at_checked, Sysvar},
};

use crate::{
    config::{config_address, counter_address, Config, Counter},
    error::FairLaunchError,
    rule::{buys_in_slot_after, check_buy, is_buy, priority_price, Buy},
};

/// Accounts of `Execute` before the extras: source, mint, destination, owner, validation list.
const FIXED_ACCOUNTS: usize = 5;

const COMPUTE_BUDGET_PROGRAM: Pubkey = pubkey!("ComputeBudget111111111111111111111111111111");

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    // The hook declares two extra accounts, the config and the slot counter, and a third, the
    // instructions sysvar, only when the priority-fee check is on (`Initialize` builds the list from
    // the same parameters). A launch without the fee check does not pay for the account.
    let extras = accounts.len().saturating_sub(FIXED_ACCOUNTS);
    let ctx = execute_prelude(program_id, accounts, data, extras)?;
    let (config_account, counter_account, instructions_sysvar) = match ctx.extras {
        [config, counter] => (config, counter, None),
        [config, counter, sysvar] => (config, counter, Some(sysvar)),
        _ => return Err(FairLaunchError::InvalidConfig.into()),
    };

    if config_account.owner != program_id
        || config_account.key != &config_address(ctx.mint.key, program_id).0
    {
        return Err(FairLaunchError::InvalidConfig.into());
    }
    let config = Config::decode(&config_account.try_borrow_data()?)?;

    let clock = Clock::get()?;
    // Outside the window, and for anything that is not a buy, there is nothing to check.
    if !config.params.in_window(clock.unix_timestamp) || !is_buy(ctx.source.key, config.venues()) {
        return Ok(());
    }

    if counter_account.owner != program_id
        || !counter_account.is_writable
        || counter_account.key != &counter_address(ctx.mint.key, program_id).0
    {
        return Err(FairLaunchError::InvalidCounter.into());
    }
    let counter = Counter::decode(&counter_account.try_borrow_data()?)?;
    let updated = Counter {
        slot: clock.slot,
        buys: buys_in_slot_after(counter.slot, counter.buys, clock.slot),
        ..counter
    };

    let priority_micro_lamports = if config.params.max_priority_micro_lamports > 0 {
        let sysvar = instructions_sysvar.ok_or(FairLaunchError::InvalidSysvar)?;
        declared_priority_price(sysvar)?
    } else {
        None
    };
    check_buy(
        &config.params,
        &Buy {
            amount: ctx.amount,
            wallet_balance_after: ctx.destination_view.amount,
            buys_in_slot: updated.buys,
            priority_micro_lamports,
        },
    )?;

    updated.encode_into(&mut counter_account.try_borrow_mut_data()?)?;
    Ok(())
}

/// The price declared by the transaction's `SetComputeUnitPrice` instruction, if it has one.
///
/// Reads the transaction's top-level instructions through the instructions sysvar.
fn declared_priority_price(instructions_sysvar: &AccountInfo) -> Result<Option<u64>, ProgramError> {
    if instructions_sysvar.key != &sysvar::instructions::id() {
        return Err(FairLaunchError::InvalidSysvar.into());
    }
    let mut index = 0;
    // `load_instruction_at_checked` fails once `index` runs past the last instruction.
    while let Ok(instruction) = load_instruction_at_checked(index, instructions_sysvar) {
        if instruction.program_id == COMPUTE_BUDGET_PROGRAM {
            if let Some(price) = priority_price(&instruction.data) {
                return Ok(Some(price));
            }
        }
        index += 1;
    }
    Ok(None)
}
