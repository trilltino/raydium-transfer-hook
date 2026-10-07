//! The two transfers implied by a swap's fixed accounts.

use transfer_hook_sdk::{CpmmSwapAccounts, SplTransferLeg};

/// The input (trader to input vault) and output (output vault to trader)
/// transfers implied by the fixed accounts of a CPMM swap.
pub fn cpmm_swap_legs(
    accounts: &CpmmSwapAccounts,
    amount_in: u64,
    expected_amount_out: u64,
) -> (SplTransferLeg, SplTransferLeg) {
    (
        SplTransferLeg {
            source: accounts.input_token_account,
            mint: accounts.input_token_mint,
            destination: accounts.input_vault,
            authority: accounts.payer,
            amount: amount_in,
        },
        SplTransferLeg {
            source: accounts.output_vault,
            mint: accounts.output_token_mint,
            destination: accounts.output_token_account,
            authority: accounts.authority,
            amount: expected_amount_out,
        },
    )
}
