//! The setup and holder instructions. (`Execute` is the SPL interface instruction and is not
//! defined here.)
//!
//! | Instruction | Who | What |
//! |---|---|---|
//! | `Initialize` | the mint's hook authority | create the global, the reward vault, the validation list |
//! | `Register` | anyone (pays the record's rent) | start counting a token account in the stream |
//! | `Fund` | anyone | add reward tokens to be paid out over a period |
//! | `Claim` | the token account's owner | pay out what the account has earned |

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

use crate::{
    error::LoyaltyError,
    state::{global_address, record_address, reward_vault_address},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoyaltyInstruction {
    Initialize,
    Register,
    Fund { amount: u64, duration: u32 },
    Claim,
}

impl LoyaltyInstruction {
    pub fn unpack(data: &[u8]) -> Result<Self, LoyaltyError> {
        match data.split_first() {
            Some((0, [])) => Ok(Self::Initialize),
            Some((1, [])) => Ok(Self::Register),
            Some((2, rest)) if rest.len() == 12 => Ok(Self::Fund {
                amount: u64::from_le_bytes(rest[..8].try_into().unwrap()),
                duration: u32::from_le_bytes(rest[8..].try_into().unwrap()),
            }),
            Some((3, [])) => Ok(Self::Claim),
            _ => Err(LoyaltyError::InvalidInstruction),
        }
    }

    pub fn pack(&self) -> Vec<u8> {
        match self {
            Self::Initialize => vec![0],
            Self::Register => vec![1],
            Self::Fund { amount, duration } => {
                let mut data = vec![2];
                data.extend_from_slice(&amount.to_le_bytes());
                data.extend_from_slice(&duration.to_le_bytes());
                data
            }
            Self::Claim => vec![3],
        }
    }
}

/// Build `Initialize`. `authority` must be the mint's live TransferHook authority and sign; the mint
/// must have no mint authority. `pool_vault` is the pool's token account of the hooked mint.
pub fn initialize(
    program_id: &Pubkey,
    payer: &Pubkey,
    authority: &Pubkey,
    mint: &Pubkey,
    pool_vault: &Pubkey,
    reward_mint: &Pubkey,
    reward_token_program: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(*pool_vault, false),
            AccountMeta::new_readonly(*reward_mint, false),
            AccountMeta::new(global_address(mint, program_id).0, false),
            AccountMeta::new(reward_vault_address(mint, program_id).0, false),
            AccountMeta::new(hook_kit::validation_list_address(mint, program_id).0, false),
            AccountMeta::new_readonly(*reward_token_program, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: LoyaltyInstruction::Initialize.pack(),
    }
}

/// Build `Register`: `payer` pays the record's rent.
pub fn register(
    program_id: &Pubkey,
    payer: &Pubkey,
    mint: &Pubkey,
    token_account: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(*token_account, false),
            AccountMeta::new(record_address(token_account, program_id).0, false),
            AccountMeta::new(global_address(mint, program_id).0, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: LoyaltyInstruction::Register.pack(),
    }
}

/// Build `Fund`: `funder` pays `amount` reward tokens from `funder_account`.
#[allow(clippy::too_many_arguments)]
pub fn fund(
    program_id: &Pubkey,
    funder: &Pubkey,
    funder_account: &Pubkey,
    mint: &Pubkey,
    reward_mint: &Pubkey,
    reward_token_program: &Pubkey,
    amount: u64,
    duration: u32,
) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new_readonly(*funder, true),
            AccountMeta::new(*funder_account, false),
            AccountMeta::new(reward_vault_address(mint, program_id).0, false),
            AccountMeta::new(global_address(mint, program_id).0, false),
            AccountMeta::new_readonly(*reward_mint, false),
            AccountMeta::new_readonly(*reward_token_program, false),
        ],
        data: LoyaltyInstruction::Fund { amount, duration }.pack(),
    }
}

/// Build `Claim`: `owner` (who must own `token_account`) is paid into `owner_reward_account`.
pub fn claim(
    program_id: &Pubkey,
    owner: &Pubkey,
    mint: &Pubkey,
    token_account: &Pubkey,
    owner_reward_account: &Pubkey,
    reward_mint: &Pubkey,
    reward_token_program: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new_readonly(*token_account, false),
            AccountMeta::new(record_address(token_account, program_id).0, false),
            AccountMeta::new(global_address(mint, program_id).0, false),
            AccountMeta::new(reward_vault_address(mint, program_id).0, false),
            AccountMeta::new(*owner_reward_account, false),
            AccountMeta::new_readonly(*reward_mint, false),
            AccountMeta::new_readonly(*reward_token_program, false),
        ],
        data: LoyaltyInstruction::Claim.pack(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_round_trip() {
        for instruction in [
            LoyaltyInstruction::Initialize,
            LoyaltyInstruction::Register,
            LoyaltyInstruction::Fund {
                amount: u64::MAX,
                duration: 7,
            },
            LoyaltyInstruction::Claim,
        ] {
            assert_eq!(
                LoyaltyInstruction::unpack(&instruction.pack()),
                Ok(instruction)
            );
        }
        assert!(LoyaltyInstruction::unpack(&[]).is_err());
        assert!(LoyaltyInstruction::unpack(&[2, 1]).is_err());
        assert!(LoyaltyInstruction::unpack(&[9]).is_err());
    }
}
