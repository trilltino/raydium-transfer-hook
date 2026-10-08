//! `Initialize`: authorise, validate, then create the config, the counter and the validation list.

use hook_kit::{
    create_pda, create_validation_list, read_hook_mint, read_token_account,
    require_extension_authority, require_hook_program,
};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvar,
};
use spl_tlv_account_resolution::{account::ExtraAccountMeta, seeds::Seed};

use crate::{
    config::{config_address, counter_address, Config, Counter, CONFIG_LEN, COUNTER_LEN},
    error::FairLaunchError,
    rule::Params,
};

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], params: Params) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let payer = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;
    let mint = next_account_info(accounts_iter)?;
    let config_account = next_account_info(accounts_iter)?;
    let counter_account = next_account_info(accounts_iter)?;
    let validation_list = next_account_info(accounts_iter)?;
    let system_program = next_account_info(accounts_iter)?;
    // Everything after the fixed accounts is a venue: a pool vault of the hooked mint.
    let venue_accounts: Vec<&AccountInfo> = accounts_iter.collect();

    if !payer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if system_program.key != &solana_program::system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    params.validate()?;

    let hook_mint = read_hook_mint(mint)?;
    require_hook_program(&hook_mint, program_id)?;
    require_extension_authority(&hook_mint, authority)?;

    let venues: Vec<Pubkey> = venue_accounts.iter().map(|venue| *venue.key).collect();
    for venue in &venue_accounts {
        if read_token_account(venue)?.mint != *mint.key {
            return Err(FairLaunchError::PoolVaultMismatch.into());
        }
    }

    let (expected_config, config_bump) = config_address(mint.key, program_id);
    let (expected_counter, counter_bump) = counter_address(mint.key, program_id);
    if config_account.key != &expected_config || counter_account.key != &expected_counter {
        return Err(FairLaunchError::InvalidConfig.into());
    }
    create_pda(
        payer,
        config_account,
        system_program,
        program_id,
        CONFIG_LEN,
        &[b"config", mint.key.as_ref(), &[config_bump]],
    )?;
    Config::new(config_bump, *mint.key, &venues, params)?
        .encode_into(&mut config_account.try_borrow_mut_data()?)?;
    create_pda(
        payer,
        counter_account,
        system_program,
        program_id,
        COUNTER_LEN,
        &[b"counter", mint.key.as_ref(), &[counter_bump]],
    )?;
    Counter {
        bump: counter_bump,
        slot: 0,
        buys: 0,
    }
    .encode_into(&mut counter_account.try_borrow_mut_data()?)?;

    // The extra accounts every transfer carries, in this order (account 1 is the mint): the config,
    // the writable slot counter, and, only if the priority-fee check is on, the instructions sysvar
    // (to read the fee).
    let mut metas = vec![
        ExtraAccountMeta::new_with_seeds(
            &[
                Seed::Literal {
                    bytes: b"config".to_vec(),
                },
                Seed::AccountKey { index: 1 },
            ],
            false,
            false,
        )?,
        ExtraAccountMeta::new_with_seeds(
            &[
                Seed::Literal {
                    bytes: b"counter".to_vec(),
                },
                Seed::AccountKey { index: 1 },
            ],
            false,
            true,
        )?,
    ];
    if params.max_priority_micro_lamports > 0 {
        metas.push(ExtraAccountMeta::new_with_pubkey(
            &sysvar::instructions::id(),
            false,
            false,
        )?);
    }
    create_validation_list(
        payer,
        validation_list,
        system_program,
        mint.key,
        program_id,
        &metas,
    )
}
