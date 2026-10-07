//! The single-funding guard, then `loyalty-rewards`' processor for everything else.
//!
//! `Execute`, `Initialize`, `Register` and `Claim` behave exactly as in `loyalty-rewards`; only
//! `Fund` has an extra rule, from [`crate::rule`].

use loyalty_rewards_hook::{instruction::LoyaltyInstruction, state::Global};
use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey};

use crate::rule::check_funding;

/// `Fund`'s accounts: funder, funder token account, reward vault, **global**, reward mint, token
/// program.
const FUND_GLOBAL_INDEX: usize = 3;

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if let Ok(LoyaltyInstruction::Fund { .. }) = LoyaltyInstruction::unpack(data) {
        // The delegated `Fund` validates that this is the real global account, so a lookalike
        // cannot be used to slip past the guard.
        if let Some(global_account) = accounts.get(FUND_GLOBAL_INDEX) {
            if let Ok(global) = Global::decode(&global_account.try_borrow_data()?) {
                check_funding(global.stream.rate)?;
            }
        }
    }
    loyalty_rewards_hook::process_instruction(program_id, accounts, data)
}
