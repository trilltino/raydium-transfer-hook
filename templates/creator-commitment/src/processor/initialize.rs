//! `Initialize`: authorise, validate, then create the config and the validation list.

use hook_kit::{
    create_pda, create_validation_list, read_hook_mint, read_token_account,
    require_extension_authority, require_hook_program,
};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    program_error::ProgramError,
    pubkey::Pubkey,
};

use crate::{
    config::{config_address, Config, CONFIG_LEN, CONFIG_SEED, VALIDATION_LIST},
    error::CommitmentError,
    rule::Schedule,
};

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], schedule: Schedule) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let payer = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;
    let mint = next_account_info(accounts_iter)?;
    let creator_account = next_account_info(accounts_iter)?;
    let config_account = next_account_info(accounts_iter)?;
    let validation_list = next_account_info(accounts_iter)?;
    let system_program = next_account_info(accounts_iter)?;

    if !payer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if system_program.key != &solana_program::system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    schedule.validate()?;

    let hook_mint = read_hook_mint(mint)?;
    require_hook_program(&hook_mint, program_id)?;
    require_extension_authority(&hook_mint, authority)?;

    let creator = read_token_account(creator_account)?;
    if creator.mint != *mint.key {
        return Err(CommitmentError::CreatorAccountMismatch.into());
    }
    if creator.amount < schedule.locked_total {
        return Err(CommitmentError::InsufficientBalanceAtInit.into());
    }

    let (expected_config, bump) = config_address(mint.key, program_id);
    if config_account.key != &expected_config {
        return Err(CommitmentError::InvalidConfig.into());
    }
    create_pda(
        payer,
        config_account,
        system_program,
        program_id,
        CONFIG_LEN,
        &[CONFIG_SEED, mint.key.as_ref(), &[bump]],
    )?;
    Config {
        bump,
        mint: *mint.key,
        creator_account: *creator_account.key,
        schedule,
    }
    .encode_into(&mut config_account.try_borrow_mut_data()?)?;

    // The one extra account every transfer carries: the config, found from the mint (account 1).
    create_validation_list(
        payer,
        validation_list,
        system_program,
        mint.key,
        program_id,
        &VALIDATION_LIST,
    )
}
