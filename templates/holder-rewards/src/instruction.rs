//! The setup and holder instructions. (`Execute` is the SPL interface instruction and is not
//! defined here.)
//!
//! | Instruction | Who | What |
//! |---|---|---|
//! | `Initialize` | the mint's hook authority | create the global, the reward vault, the validation list; `one_time` makes it a single-funding spin-off |
//! | `Register` | anyone (pays the record's rent) | start counting a token account in the stream |
//! | `Fund` | anyone | add reward tokens to be paid out over a period |
//! | `Claim` | the token account's owner | pay out what the account has earned |
//! | `Reconcile` | anyone | correct a record whose balance fell without a transfer (a burn, or a closed account), so it stops diluting the stream |

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

use crate::{
    error::HolderRewardsError,
    state::{global_address, record_address, reward_vault_address},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HolderRewardsInstruction {
    /// `one_time`: the allocation can be funded once (a spin-off) instead of topped up.
    Initialize {
        one_time: bool,
    },
    Register,
    Fund {
        amount: u64,
        duration: u32,
    },
    Claim,
    /// Correct a stale record after a burn or a closed token account. Permissionless.
    Reconcile,
}

const INVALID: HolderRewardsError = HolderRewardsError::InvalidInstruction;

impl HolderRewardsInstruction {
    pub fn unpack(data: &[u8]) -> Result<Self, HolderRewardsError> {
        match data.split_first() {
            Some((0, [])) => Ok(Self::Initialize { one_time: false }),
            Some((0, [mode])) if *mode <= 1 => Ok(Self::Initialize {
                one_time: *mode == 1,
            }),
            Some((1, [])) => Ok(Self::Register),
            Some((2, rest)) if rest.len() == 12 => {
                let (amount, duration) = rest.split_at(8);
                Ok(Self::Fund {
                    amount: u64::from_le_bytes(amount.try_into().map_err(|_| INVALID)?),
                    duration: u32::from_le_bytes(duration.try_into().map_err(|_| INVALID)?),
                })
            }
            Some((3, [])) => Ok(Self::Claim),
            Some((4, [])) => Ok(Self::Reconcile),
            _ => Err(HolderRewardsError::InvalidInstruction),
        }
    }

    pub fn pack(&self) -> Vec<u8> {
        match self {
            Self::Initialize { one_time: false } => vec![0],
            Self::Initialize { one_time: true } => vec![0, 1],
            Self::Register => vec![1],
            Self::Fund { amount, duration } => {
                let mut data = vec![2];
                data.extend_from_slice(&amount.to_le_bytes());
                data.extend_from_slice(&duration.to_le_bytes());
                data
            }
            Self::Claim => vec![3],
            Self::Reconcile => vec![4],
        }
    }
}

/// Build `Initialize` for an ongoing programme (the reward stream can be topped up). `authority` must
/// be the mint's live TransferHook authority and sign; the mint must have no mint authority.
/// `pool_vault` is the pool's token account of the hooked mint.
pub fn initialize(
    program_id: &Pubkey,
    payer: &Pubkey,
    authority: &Pubkey,
    mint: &Pubkey,
    pool_vault: &Pubkey,
    reward_mint: &Pubkey,
    reward_token_program: &Pubkey,
) -> Instruction {
    initialize_with_mode(
        program_id,
        payer,
        authority,
        mint,
        pool_vault,
        reward_mint,
        reward_token_program,
        false,
    )
}

/// Build `Initialize` for a one-time allocation (a spin-off): it can be funded once.
pub fn initialize_one_time(
    program_id: &Pubkey,
    payer: &Pubkey,
    authority: &Pubkey,
    mint: &Pubkey,
    pool_vault: &Pubkey,
    reward_mint: &Pubkey,
    reward_token_program: &Pubkey,
) -> Instruction {
    initialize_with_mode(
        program_id,
        payer,
        authority,
        mint,
        pool_vault,
        reward_mint,
        reward_token_program,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn initialize_with_mode(
    program_id: &Pubkey,
    payer: &Pubkey,
    authority: &Pubkey,
    mint: &Pubkey,
    pool_vault: &Pubkey,
    reward_mint: &Pubkey,
    reward_token_program: &Pubkey,
    one_time: bool,
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
        data: HolderRewardsInstruction::Initialize { one_time }.pack(),
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
        data: HolderRewardsInstruction::Register.pack(),
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
        data: HolderRewardsInstruction::Fund { amount, duration }.pack(),
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
        data: HolderRewardsInstruction::Claim.pack(),
    }
}

/// Build `Reconcile` for `token_account` of `mint`. Anyone may send it; it only ever lowers a stale
/// count (the token account may be closed).
#[must_use]
pub fn reconcile(program_id: &Pubkey, mint: &Pubkey, token_account: &Pubkey) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new_readonly(*token_account, false),
            AccountMeta::new(record_address(token_account, program_id).0, false),
            AccountMeta::new(global_address(mint, program_id).0, false),
        ],
        data: HolderRewardsInstruction::Reconcile.pack(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_round_trip() {
        for instruction in [
            HolderRewardsInstruction::Initialize { one_time: false },
            HolderRewardsInstruction::Initialize { one_time: true },
            HolderRewardsInstruction::Register,
            HolderRewardsInstruction::Fund {
                amount: u64::MAX,
                duration: 7,
            },
            HolderRewardsInstruction::Claim,
            HolderRewardsInstruction::Reconcile,
        ] {
            assert_eq!(
                HolderRewardsInstruction::unpack(&instruction.pack()),
                Ok(instruction)
            );
        }
        assert!(HolderRewardsInstruction::unpack(&[]).is_err());
        assert!(HolderRewardsInstruction::unpack(&[2, 1]).is_err());
        assert!(HolderRewardsInstruction::unpack(&[9]).is_err());
        // The original one-byte Initialize still means an ongoing programme; the mode byte is 0 or 1.
        assert_eq!(
            HolderRewardsInstruction::unpack(&[0]),
            Ok(HolderRewardsInstruction::Initialize { one_time: false })
        );
        assert!(HolderRewardsInstruction::unpack(&[0, 2]).is_err());
    }
}
