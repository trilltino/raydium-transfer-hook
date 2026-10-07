//! `Initialize` and `Execute` of the bench hook.

use hook_kit::{
    create_pda, create_validation_list, execute_prelude, read_hook_mint,
    require_extension_authority, require_hook_program, EXECUTE_DISCRIMINATOR,
};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
    system_program,
};
use spl_tlv_account_resolution::{account::ExtraAccountMeta, seeds::Seed};

/// The most extras one hook can declare here. The validation list is one account; this keeps it
/// well inside one allocation.
pub const MAX_EXTRAS: u8 = 200;

const INITIALIZE_TAG: u8 = 0;
const COUNTER_LEN: usize = 8;

/// The `index`th extra of `mint`'s hook: a PDA of the program (it need not exist).
pub fn extra_address(program_id: &Pubkey, mint: &Pubkey, index: u8) -> Pubkey {
    Pubkey::find_program_address(&[b"bench-extra", mint.as_ref(), &[index]], program_id).0
}

/// The writable counter of `mint`'s hook, when `writable_counter` was set.
pub fn counter_address(program_id: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"bench-counter", mint.as_ref()], program_id).0
}

/// Build `Initialize`: declare `extras` extra accounts for `mint` (the first is a writable counter
/// if `writable_counter`). `authority` must be the mint's hook authority and sign.
pub fn initialize_instruction(
    program_id: &Pubkey,
    payer: &Pubkey,
    authority: &Pubkey,
    mint: &Pubkey,
    extras: u8,
    writable_counter: bool,
) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new(counter_address(program_id, mint), false),
            AccountMeta::new(hook_kit::validation_list_address(mint, program_id).0, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: vec![INITIALIZE_TAG, extras, writable_counter as u8],
    }
}

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() >= 8 && data[..8] == EXECUTE_DISCRIMINATOR {
        return execute(program_id, accounts, data);
    }
    match data {
        [INITIALIZE_TAG, extras, writable] if *extras <= MAX_EXTRAS => {
            initialize(program_id, accounts, *extras, *writable != 0)
        }
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

fn initialize(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    extras: u8,
    writable_counter: bool,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let payer = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;
    let mint = next_account_info(accounts_iter)?;
    let counter = next_account_info(accounts_iter)?;
    let validation_list = next_account_info(accounts_iter)?;
    let system = next_account_info(accounts_iter)?;

    let hook_mint = read_hook_mint(mint)?;
    require_hook_program(&hook_mint, program_id)?;
    require_extension_authority(&hook_mint, authority)?;

    let mint_seed = |literal: &[u8]| Seed::Literal {
        bytes: literal.to_vec(),
    };
    let mut metas = Vec::with_capacity(extras as usize);
    for index in 0..extras {
        let meta = if writable_counter && index == 0 {
            // Account 1 of a transfer is the mint.
            ExtraAccountMeta::new_with_seeds(
                &[mint_seed(b"bench-counter"), Seed::AccountKey { index: 1 }],
                false,
                true,
            )?
        } else {
            ExtraAccountMeta::new_with_seeds(
                &[
                    mint_seed(b"bench-extra"),
                    Seed::AccountKey { index: 1 },
                    mint_seed(&[index]),
                ],
                false,
                false,
            )?
        };
        metas.push(meta);
    }
    if writable_counter && extras > 0 {
        let (expected, bump) =
            Pubkey::find_program_address(&[b"bench-counter", mint.key.as_ref()], program_id);
        if counter.key != &expected {
            return Err(ProgramError::InvalidSeeds);
        }
        create_pda(
            payer,
            counter,
            system,
            program_id,
            COUNTER_LEN,
            &[b"bench-counter", mint.key.as_ref(), &[bump]],
        )?;
    }
    create_validation_list(payer, validation_list, system, mint.key, program_id, &metas)
}

fn execute(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    // The number of extras is whatever the validation list declares; the prelude checks that the
    // list and the accounts agree.
    let extras = accounts.len().saturating_sub(5);
    let ctx = execute_prelude(program_id, accounts, data, extras)?;
    if let Some(first) = ctx.extras.first() {
        if first.is_writable {
            let mut counter = first.try_borrow_mut_data()?;
            if counter.len() != COUNTER_LEN || first.owner != program_id {
                return Err(ProgramError::InvalidAccountData);
            }
            let next = u64::from_le_bytes(counter[..COUNTER_LEN].try_into().expect("8 bytes"))
                .wrapping_add(1);
            counter[..COUNTER_LEN].copy_from_slice(&next.to_le_bytes());
        }
    }
    Ok(())
}
