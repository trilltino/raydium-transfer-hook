//! `Register`: start counting a token account in the stream.

use hook_kit::{create_pda, read_token_account, KitError};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    program_error::ProgramError,
    pubkey::Pubkey,
};

use super::common::{load_global, now};
use crate::{
    error::HolderRewardsError,
    rule::Holder,
    state::{record_address, Record, HOLDER_SEED, RECORD_LEN},
};

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let payer = next_account_info(accounts_iter)?;
    let mint = next_account_info(accounts_iter)?;
    let token_account = next_account_info(accounts_iter)?;
    let record_account = next_account_info(accounts_iter)?;
    let global_account = next_account_info(accounts_iter)?;
    let system_program = next_account_info(accounts_iter)?;

    if !payer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if system_program.key != &solana_program::system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    let mut global = load_global(program_id, mint.key, global_account)?;
    let token = read_token_account(token_account)?;
    if token.mint != *mint.key {
        return Err(KitError::TokenAccountMismatch.into());
    }
    // The pool's own vault must not collect the holders' rewards.
    if token_account.key == &global.pool_vault {
        return Err(HolderRewardsError::ExcludedAccount.into());
    }

    let (expected, bump) = record_address(token_account.key, program_id);
    if record_account.key != &expected {
        return Err(HolderRewardsError::InvalidRecord.into());
    }
    // Fails with `KitError::AlreadyInitialized` if the account is already registered.
    create_pda(
        payer,
        record_account,
        system_program,
        program_id,
        RECORD_LEN,
        &[HOLDER_SEED, token_account.key.as_ref(), &[bump]],
    )?;

    global.stream.advance(now()?)?;
    let holder = Holder::register(&mut global.stream, token.amount)?;
    Record {
        bump,
        token_account: *token_account.key,
        mint: *mint.key,
        holder,
    }
    .encode_into(&mut record_account.try_borrow_mut_data()?)?;
    global.encode_into(&mut global_account.try_borrow_mut_data()?)?;
    Ok(())
}
