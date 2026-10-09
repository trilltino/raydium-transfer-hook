//! `Fund`: pay reward tokens into the vault and start (or extend) the stream.

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    program::invoke,
    program_error::ProgramError,
    pubkey::Pubkey,
};
use spl_token_2022::instruction::transfer_checked;

use super::common::{load_any_global, mint_decimals, now, require_token_program};
use crate::{error::HolderRewardsError, rule::check_funding};

pub fn process(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
    duration: u32,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let funder = next_account_info(accounts_iter)?;
    let funder_account = next_account_info(accounts_iter)?;
    let reward_vault = next_account_info(accounts_iter)?;
    let global_account = next_account_info(accounts_iter)?;
    let reward_mint = next_account_info(accounts_iter)?;
    let token_program = next_account_info(accounts_iter)?;

    if !funder.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let mut global = load_any_global(program_id, global_account)?;
    if reward_vault.key != &global.reward_vault || reward_mint.key != &global.reward_mint {
        return Err(HolderRewardsError::RewardAccountMismatch.into());
    }
    require_token_program(token_program, reward_mint)?;
    // A one-time allocation (a spin-off) is funded once; an ongoing programme can be topped up.
    check_funding(global.one_time, global.stream.rate)?;

    // Validates the amount and duration, and updates the rate, before any tokens move.
    global.stream.fund(now()?, amount, duration)?;

    invoke(
        &transfer_checked(
            token_program.key,
            funder_account.key,
            reward_mint.key,
            reward_vault.key,
            funder.key,
            &[],
            amount,
            mint_decimals(reward_mint)?,
        )?,
        &[
            funder_account.clone(),
            reward_mint.clone(),
            reward_vault.clone(),
            funder.clone(),
        ],
    )?;
    global.encode_into(&mut global_account.try_borrow_mut_data()?)?;
    Ok(())
}
