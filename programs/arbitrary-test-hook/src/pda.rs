//! Program-derived addresses and the validation list's two seeds-based extra accounts.

use solana_program::{program_error::ProgramError, pubkey::Pubkey};
use spl_tlv_account_resolution::{account::ExtraAccountMeta, seeds::Seed};

use crate::constants::*;

pub fn policy_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[POLICY_SEED, mint.as_ref()], program_id)
}

pub fn stats_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[STATS_SEED, mint.as_ref()], program_id)
}

pub fn validation_list_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[VALIDATION_LIST_SEED, mint.as_ref()], program_id)
}

/// The two seeds-based metas stored in every validation list.
pub fn extra_account_metas() -> Result<[ExtraAccountMeta; EXTRA_ACCOUNTS], ProgramError> {
    Ok([
        ExtraAccountMeta::new_with_seeds(
            &[
                Seed::Literal {
                    bytes: POLICY_SEED.to_vec(),
                },
                Seed::AccountKey { index: 1 },
            ],
            false,
            false,
        )?,
        ExtraAccountMeta::new_with_seeds(
            &[
                Seed::Literal {
                    bytes: STATS_SEED.to_vec(),
                },
                Seed::AccountKey { index: 1 },
            ],
            false,
            true,
        )?,
    ])
}
