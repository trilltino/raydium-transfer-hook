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
};
use spl_tlv_account_resolution::{account::ExtraAccountMeta, seeds::Seed};

use crate::{
    error::AntiBundleError,
    rule::{validate_venues, Params},
    state::{config_address, counter_address, Config, Counter, CONFIG_LEN, COUNTER_LEN},
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
    let venue_accounts = accounts_iter.as_slice();

    if !payer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if system_program.key != &solana_program::system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    params.validate()?;
    let venues: Vec<Pubkey> = venue_accounts.iter().map(|a| *a.key).collect();
    validate_venues(&venues)?;

    let hook_mint = read_hook_mint(mint)?;
    require_hook_program(&hook_mint, program_id)?;
    require_extension_authority(&hook_mint, authority)?;

    for venue in venue_accounts {
        if read_token_account(venue)?.mint != *mint.key {
            return Err(AntiBundleError::VenueMismatch.into());
        }
    }

    let (expected_config, config_bump) = config_address(mint.key, program_id);
    let (expected_counter, counter_bump) = counter_address(mint.key, program_id);
    if config_account.key != &expected_config || counter_account.key != &expected_counter {
        return Err(AntiBundleError::InvalidConfig.into());
    }
    create_pda(
        payer,
        config_account,
        system_program,
        program_id,
        CONFIG_LEN,
        &[b"config", mint.key.as_ref(), &[config_bump]],
    )?;
    Config {
        bump: config_bump,
        mint: *mint.key,
        params,
        venues,
    }
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

    // The extra accounts every transfer carries (account 1 is the mint): the config, and the
    // writable slot counter.
    let seeds_of = |literal: &[u8]| {
        [
            Seed::Literal {
                bytes: literal.to_vec(),
            },
            Seed::AccountKey { index: 1 },
        ]
    };
    let metas = [
        ExtraAccountMeta::new_with_seeds(&seeds_of(b"config"), false, false)?,
        ExtraAccountMeta::new_with_seeds(&seeds_of(b"counter"), false, true)?,
    ];
    create_validation_list(
        payer,
        validation_list,
        system_program,
        mint.key,
        program_id,
        &metas,
    )
}
