//! The one setup instruction. (`Execute` is the SPL interface instruction and is not defined here.)

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

use crate::{
    config::{config_address, counter_address},
    error::FairLaunchError,
    rule::Params,
};

const INITIALIZE_TAG: u8 = 0;
const PARAMS_LEN: usize = 8 + 8 + 8 + 8 + 4 + 8;

/// Create the config, the slot counter and the validation list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FairLaunchInstruction {
    Initialize(Params),
}

impl FairLaunchInstruction {
    pub fn unpack(data: &[u8]) -> Result<Self, FairLaunchError> {
        match data.split_first() {
            Some((&INITIALIZE_TAG, rest)) if rest.len() == PARAMS_LEN => {
                let array = |at: usize, len: usize| &rest[at..at + len];
                let int = |at: usize| -> [u8; 8] { array(at, 8).try_into().unwrap() };
                Ok(Self::Initialize(Params {
                    window_start: i64::from_le_bytes(int(0)),
                    window_end: i64::from_le_bytes(int(8)),
                    max_buy: u64::from_le_bytes(int(16)),
                    max_wallet: u64::from_le_bytes(int(24)),
                    max_buys_per_slot: u32::from_le_bytes(array(32, 4).try_into().unwrap()),
                    max_priority_micro_lamports: u64::from_le_bytes(int(36)),
                }))
            }
            _ => Err(FairLaunchError::InvalidInstruction),
        }
    }

    pub fn pack(&self) -> Vec<u8> {
        let Self::Initialize(params) = self;
        let mut data = vec![INITIALIZE_TAG];
        data.extend_from_slice(&params.window_start.to_le_bytes());
        data.extend_from_slice(&params.window_end.to_le_bytes());
        data.extend_from_slice(&params.max_buy.to_le_bytes());
        data.extend_from_slice(&params.max_wallet.to_le_bytes());
        data.extend_from_slice(&params.max_buys_per_slot.to_le_bytes());
        data.extend_from_slice(&params.max_priority_micro_lamports.to_le_bytes());
        data
    }
}

/// Build the `Initialize` instruction.
///
/// `authority` must be the mint's live TransferHook extension authority and sign. `venues` are the
/// pool vaults of the hooked mint (one to four) whose outgoing transfers count as buys.
pub fn initialize(
    program_id: &Pubkey,
    payer: &Pubkey,
    authority: &Pubkey,
    mint: &Pubkey,
    venues: &[Pubkey],
    params: Params,
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(*payer, true),
        AccountMeta::new_readonly(*authority, true),
        AccountMeta::new_readonly(*mint, false),
        AccountMeta::new(config_address(mint, program_id).0, false),
        AccountMeta::new(counter_address(mint, program_id).0, false),
        AccountMeta::new(hook_kit::validation_list_address(mint, program_id).0, false),
        AccountMeta::new_readonly(system_program::id(), false),
    ];
    accounts.extend(venues.iter().map(|v| AccountMeta::new_readonly(*v, false)));
    Instruction {
        program_id: *program_id,
        accounts,
        data: FairLaunchInstruction::Initialize(params).pack(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_round_trips() {
        let params = Params {
            window_start: -1,
            window_end: 9,
            max_buy: 5,
            max_wallet: 6,
            max_buys_per_slot: 7,
            max_priority_micro_lamports: 8,
        };
        let packed = FairLaunchInstruction::Initialize(params).pack();
        assert_eq!(packed.len(), 1 + PARAMS_LEN);
        assert_eq!(
            FairLaunchInstruction::unpack(&packed),
            Ok(FairLaunchInstruction::Initialize(params))
        );
        assert!(FairLaunchInstruction::unpack(&packed[..packed.len() - 1]).is_err());
        assert!(FairLaunchInstruction::unpack(&[]).is_err());
    }
}
