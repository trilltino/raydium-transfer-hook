//! `Initialize`: authorise, validate, then create the global, the reward vault and the validation
//! list.

use hook_kit::{
    create_pda, create_validation_list, read_hook_mint, read_token_account,
    require_extension_authority, require_hook_program, require_mint_authority_revoked,
};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    program::invoke,
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
};
use spl_tlv_account_resolution::{account::ExtraAccountMeta, seeds::Seed};
use spl_token_2022::{instruction::initialize_account3, state::Account as TokenAccount};

use super::common::require_token_program;
use crate::{
    error::HolderRewardsError,
    rule::Stream,
    state::{global_address, reward_vault_address, Global, GLOBAL_LEN},
};

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], one_time: bool) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let payer = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;
    let mint = next_account_info(accounts_iter)?;
    let pool_vault = next_account_info(accounts_iter)?;
    let reward_mint = next_account_info(accounts_iter)?;
    let global_account = next_account_info(accounts_iter)?;
    let reward_vault = next_account_info(accounts_iter)?;
    let validation_list = next_account_info(accounts_iter)?;
    let token_program = next_account_info(accounts_iter)?;
    let system_program = next_account_info(accounts_iter)?;

    if !payer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if system_program.key != &solana_program::system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    let hook_mint = read_hook_mint(mint)?;
    require_hook_program(&hook_mint, program_id)?;
    require_extension_authority(&hook_mint, authority)?;
    // Minting is not a transfer, so the hook would never see it: the rule needs a fixed supply.
    require_mint_authority_revoked(&hook_mint)?;

    if read_token_account(pool_vault)?.mint != *mint.key {
        return Err(HolderRewardsError::PoolVaultMismatch.into());
    }

    require_token_program(token_program, reward_mint)?;
    // A reward mint with a hook of its own would need extra accounts on every claim and fund.
    if reward_mint.owner == &spl_token_2022::id() && read_hook_mint(reward_mint).is_ok() {
        return Err(HolderRewardsError::RewardMintHasHook.into());
    }

    let (expected_global, global_bump) = global_address(mint.key, program_id);
    let (expected_vault, vault_bump) = reward_vault_address(mint.key, program_id);
    if global_account.key != &expected_global || reward_vault.key != &expected_vault {
        return Err(HolderRewardsError::InvalidGlobal.into());
    }
    create_pda(
        payer,
        global_account,
        system_program,
        program_id,
        GLOBAL_LEN,
        &[b"rewards", mint.key.as_ref(), &[global_bump]],
    )?;
    Global {
        bump: global_bump,
        mint: *mint.key,
        reward_mint: *reward_mint.key,
        reward_vault: *reward_vault.key,
        pool_vault: *pool_vault.key,
        stream: Stream::default(),
        one_time,
    }
    .encode_into(&mut global_account.try_borrow_mut_data()?)?;

    // The reward vault is a token account at a PDA, owned (as a token account) by the global PDA,
    // so only this program can move rewards out of it.
    create_pda(
        payer,
        reward_vault,
        system_program,
        token_program.key,
        TokenAccount::LEN,
        &[b"reward-vault", mint.key.as_ref(), &[vault_bump]],
    )?;
    invoke(
        &initialize_account3(
            token_program.key,
            reward_vault.key,
            reward_mint.key,
            global_account.key,
        )?,
        &[reward_vault.clone(), reward_mint.clone()],
    )?;

    // The extra accounts every transfer carries (account 0 is the source, 1 the mint, 2 the
    // destination): the global, the source's record, the destination's record. All writable.
    let seeds_of = |literal: &[u8], index: u8| {
        [
            Seed::Literal {
                bytes: literal.to_vec(),
            },
            Seed::AccountKey { index },
        ]
    };
    let metas = [
        ExtraAccountMeta::new_with_seeds(&seeds_of(b"rewards", 1), false, true)?,
        ExtraAccountMeta::new_with_seeds(&seeds_of(b"holder", 0), false, true)?,
        ExtraAccountMeta::new_with_seeds(&seeds_of(b"holder", 2), false, true)?,
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
