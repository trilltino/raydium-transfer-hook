//! `Claim`: pay a token account's earnings to its owner.

use hook_kit::{read_token_account, KitError};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
};
use spl_token_2022::instruction::transfer_checked;

use super::common::{
    load_any_global, load_record, mint_decimals, now, require_token_program, token_fields,
};
use crate::{error::HolderRewardsError, state::REWARDS_SEED};

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let owner = next_account_info(accounts_iter)?;
    let token_account = next_account_info(accounts_iter)?;
    let record_account = next_account_info(accounts_iter)?;
    let global_account = next_account_info(accounts_iter)?;
    let reward_vault = next_account_info(accounts_iter)?;
    let owner_reward_account = next_account_info(accounts_iter)?;
    let reward_mint = next_account_info(accounts_iter)?;
    let token_program = next_account_info(accounts_iter)?;

    if !owner.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let mut global = load_any_global(program_id, global_account)?;
    if reward_vault.key != &global.reward_vault || reward_mint.key != &global.reward_mint {
        return Err(HolderRewardsError::RewardAccountMismatch.into());
    }
    require_token_program(token_program, reward_mint)?;

    let token = read_token_account(token_account)?;
    if token.mint != global.mint {
        return Err(KitError::TokenAccountMismatch.into());
    }
    if token.owner != *owner.key {
        return Err(HolderRewardsError::WrongOwner.into());
    }
    if record_account.owner != program_id {
        return Err(HolderRewardsError::NotRegistered.into());
    }
    let mut record = load_record(program_id, token_account.key, record_account)?;

    // The payout goes to an account of the reward mint that the owner controls.
    if owner_reward_account.owner != token_program.key {
        return Err(HolderRewardsError::RewardAccountMismatch.into());
    }
    let (reward_account_mint, reward_account_owner, _) = token_fields(owner_reward_account)?;
    if reward_account_mint != global.reward_mint || reward_account_owner != *owner.key {
        return Err(HolderRewardsError::RewardAccountMismatch.into());
    }

    global.stream.advance(now()?)?;
    let payout = record.holder.claim(&mut global.stream, token.amount)?;
    if payout == 0 {
        return Err(HolderRewardsError::NothingToClaim.into());
    }

    // The global PDA owns the vault, so it signs the payout.
    invoke_signed(
        &transfer_checked(
            token_program.key,
            reward_vault.key,
            reward_mint.key,
            owner_reward_account.key,
            global_account.key,
            &[],
            payout,
            mint_decimals(reward_mint)?,
        )?,
        &[
            reward_vault.clone(),
            reward_mint.clone(),
            owner_reward_account.clone(),
            global_account.clone(),
        ],
        &[&[REWARDS_SEED, global.mint.as_ref(), &[global.bump]]],
    )?;

    record.encode_into(&mut record_account.try_borrow_mut_data()?)?;
    global.encode_into(&mut global_account.try_borrow_mut_data()?)?;
    Ok(())
}
