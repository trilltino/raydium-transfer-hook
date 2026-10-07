//! Entry dispatch: the SPL `Execute` instruction, or this program's setup instruction.

mod execute;
mod initialize;

use hook_kit::EXECUTE_DISCRIMINATOR;
use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey};

use crate::instruction::AntiBundleInstruction;

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() >= 8 && data[..8] == EXECUTE_DISCRIMINATOR {
        return execute::process(program_id, accounts, data);
    }
    match AntiBundleInstruction::unpack(data)? {
        AntiBundleInstruction::Initialize(params) => {
            initialize::process(program_id, accounts, params)
        }
    }
}
