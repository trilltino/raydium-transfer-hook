//! The one setup instruction. (`Execute` is the SPL interface instruction and is not defined here.)

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

use crate::{
    config::config_address,
    error::CommitmentError,
    rule::Schedule,
};

const INITIALIZE_TAG: u8 = 0;

/// Create the config and the validation list, and lock `schedule.locked_total` of the creator
/// account's balance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitmentInstruction {
    Initialize(Schedule),
}

impl CommitmentInstruction {
    pub fn unpack(data: &[u8]) -> Result<Self, CommitmentError> {
        match data.split_first() {
            Some((&INITIALIZE_TAG, rest)) if rest.len() == 32 => {
                let word = |i: usize| -> [u8; 8] { rest[i * 8..i * 8 + 8].try_into().unwrap() };
                Ok(Self::Initialize(Schedule {
                    locked_total: u64::from_le_bytes(word(0)),
                    start: i64::from_le_bytes(word(1)),
                    cliff: i64::from_le_bytes(word(2)),
                    end: i64::from_le_bytes(word(3)),
                }))
            }
            _ => Err(CommitmentError::InvalidInstruction),
        }
    }

    pub fn pack(&self) -> Vec<u8> {
        let Self::Initialize(schedule) = self;
        let mut data = vec![INITIALIZE_TAG];
        data.extend_from_slice(&schedule.locked_total.to_le_bytes());
        data.extend_from_slice(&schedule.start.to_le_bytes());
        data.extend_from_slice(&schedule.cliff.to_le_bytes());
        data.extend_from_slice(&schedule.end.to_le_bytes());
        data
    }
}

/// Build the `Initialize` instruction.
///
/// `authority` must be the mint's live TransferHook extension authority and sign.
pub fn initialize(
    program_id: &Pubkey,
    payer: &Pubkey,
    authority: &Pubkey,
    mint: &Pubkey,
    creator_account: &Pubkey,
    schedule: Schedule,
) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(*creator_account, false),
            AccountMeta::new(config_address(mint, program_id).0, false),
            AccountMeta::new(hook_kit::validation_list_address(mint, program_id).0, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: CommitmentInstruction::Initialize(schedule).pack(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_round_trips() {
        let schedule = Schedule {
            locked_total: 5,
            start: -1,
            cliff: 0,
            end: 9,
        };
        let packed = CommitmentInstruction::Initialize(schedule).pack();
        assert_eq!(
            CommitmentInstruction::unpack(&packed),
            Ok(CommitmentInstruction::Initialize(schedule))
        );
        assert!(CommitmentInstruction::unpack(&packed[..packed.len() - 1]).is_err());
        assert!(CommitmentInstruction::unpack(&[]).is_err());
    }
}
