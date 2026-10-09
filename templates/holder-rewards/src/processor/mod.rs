//! Entry dispatch: the SPL `Execute` instruction, or one of this program's own instructions.

mod claim;
mod common;
mod execute;
mod fund;
mod initialize;
mod reconcile;
mod register;

use hook_kit::EXECUTE_DISCRIMINATOR;
use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey};

use crate::instruction::HolderRewardsInstruction;

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() >= 8 && data[..8] == EXECUTE_DISCRIMINATOR {
        return execute::process(program_id, accounts, data);
    }
    match HolderRewardsInstruction::unpack(data)? {
        HolderRewardsInstruction::Initialize { one_time } => {
            initialize::process(program_id, accounts, one_time)
        }
        HolderRewardsInstruction::Register => register::process(program_id, accounts),
        HolderRewardsInstruction::Fund { amount, duration } => {
            fund::process(program_id, accounts, amount, duration)
        }
        HolderRewardsInstruction::Claim => claim::process(program_id, accounts),
        HolderRewardsInstruction::Reconcile => reconcile::process(program_id, accounts),
    }
}
