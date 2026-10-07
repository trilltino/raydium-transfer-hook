//! The one setup instruction. (`Execute` is the SPL interface instruction and is not defined here.)

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

use crate::{
    error::AntiBundleError,
    rule::Params,
    state::{config_address, counter_address},
};

const INITIALIZE_TAG: u8 = 0;
const PARAMS_LEN: usize = 8 + 2;

/// Create the config, the slot counter and the validation list. The venue vaults follow the fixed
/// accounts, one account each.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AntiBundleInstruction {
    Initialize(Params),
}

impl AntiBundleInstruction {
    pub fn unpack(data: &[u8]) -> Result<Self, AntiBundleError> {
        match data.split_first() {
            Some((&INITIALIZE_TAG, rest)) if rest.len() == PARAMS_LEN => {
                Ok(Self::Initialize(Params {
                    active_until: i64::from_le_bytes(rest[..8].try_into().unwrap()),
                    max_buys_per_slot: u16::from_le_bytes(rest[8..10].try_into().unwrap()),
                }))
            }
            _ => Err(AntiBundleError::InvalidInstruction),
        }
    }

    pub fn pack(&self) -> Vec<u8> {
        let Self::Initialize(params) = self;
        let mut data = vec![INITIALIZE_TAG];
        data.extend_from_slice(&params.active_until.to_le_bytes());
        data.extend_from_slice(&params.max_buys_per_slot.to_le_bytes());
        data
    }
}

/// Build the `Initialize` instruction.
///
/// `authority` must be the mint's live TransferHook extension authority and sign. `venues` are the
/// pool vaults of the hooked mint whose outgoing transfers count as buys.
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
        data: AntiBundleInstruction::Initialize(params).pack(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_round_trips() {
        let params = Params {
            active_until: -5,
            max_buys_per_slot: 9,
        };
        let packed = AntiBundleInstruction::Initialize(params).pack();
        assert_eq!(
            AntiBundleInstruction::unpack(&packed),
            Ok(AntiBundleInstruction::Initialize(params))
        );
        assert!(AntiBundleInstruction::unpack(&packed[..packed.len() - 1]).is_err());
        assert!(AntiBundleInstruction::unpack(&[]).is_err());
    }
}
