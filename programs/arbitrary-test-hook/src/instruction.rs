//! Client-side builder for the init instruction.

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

use crate::{constants::INIT_DISCRIMINATOR, pda::*};

/// Build the init instruction. `authority` must be the mint's live TransferHook authority.
pub fn init_instruction(
    program_id: Pubkey,
    payer: Pubkey,
    mint: Pubkey,
    authority: Pubkey,
    max_per_slot: u32,
) -> Instruction {
    let mut data = INIT_DISCRIMINATOR.to_vec();
    data.extend_from_slice(&max_per_slot.to_le_bytes());
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(policy_address(&mint, &program_id).0, false),
            AccountMeta::new(stats_address(&mint, &program_id).0, false),
            AccountMeta::new(validation_list_address(&mint, &program_id).0, false),
            AccountMeta::new_readonly(solana_program::system_program::id(), false),
        ],
        data,
    }
}
