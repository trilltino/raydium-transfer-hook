//! Program-derived addresses and the validation list's single seeds-based extra account.

use solana_program::{program_error::ProgramError, pubkey::Pubkey};
use spl_tlv_account_resolution::{account::ExtraAccountMeta, seeds::Seed};

use crate::constants::*;

pub fn config_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[CONFIG_SEED, mint.as_ref()], program_id)
}

pub fn validation_list_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    spl_transfer_hook_interface::get_extra_account_metas_address_and_bump_seed(mint, program_id)
}

/// The single seeds-based meta stored in every validation list.
pub fn config_extra_account_meta() -> Result<ExtraAccountMeta, ProgramError> {
    ExtraAccountMeta::new_with_seeds(
        &[
            Seed::Literal {
                bytes: CONFIG_SEED.to_vec(),
            },
            Seed::AccountKey { index: 1 },
        ],
        false,
        false,
    )
}

pub fn execute_instruction_data(amount: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(16);
    data.extend_from_slice(&EXECUTE_DISCRIMINATOR);
    data.extend_from_slice(&amount.to_le_bytes());
    data
}
