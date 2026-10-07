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
    rule::{buys_in_slot_after, check_buy, priority_price, Buy},
};

/// The hook declares three extra accounts: the config, the slot counter, the instructions sysvar.
const EXTRA_ACCOUNTS: usize = 3;

const COMPUTE_BUDGET_PROGRAM: Pubkey = pubkey!("ComputeBudget111111111111111111111111111111");

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let ctx = execute_prelude(program_id, accounts, data, EXTRA_ACCOUNTS)?;
    let [config_account, counter_account, instructions_sysvar] = ctx.extras else {
        return Err(FairLaunchError::InvalidConfig.into());
    };

    if config_account.owner != program_id
        || config_account.key != &config_address(ctx.mint.key, program_id).0
    {
        return Err(FairLaunchError::InvalidConfig.into());
    }
    let config = Config::decode(&config_account.try_borrow_data()?)?;

    let clock = Clock::get()?;
    // Outside the window, and for anything that is not a buy, there is nothing to check.
    if !config.params.in_window(clock.unix_timestamp) || ctx.source.key != &config.pool_vault {
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
        declared_priority_price(instructions_sysvar)?
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
