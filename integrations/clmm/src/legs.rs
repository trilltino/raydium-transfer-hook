//! The two transfers implied by a swap's fixed accounts.

use transfer_hook_sdk::{ClmmSwapAccounts, SplTransferLeg};

/// The input (trader to input vault) and output (output vault to trader)
/// transfers implied by the fixed accounts of a CLMM swap. The pool state
/// authorizes the output transfer.
pub fn clmm_swap_legs(
    accounts: &ClmmSwapAccounts,
    amount_in: u64,
    expected_amount_out: u64,
) -> (SplTransferLeg, SplTransferLeg) {
    (
        SplTransferLeg {
            source: accounts.input_token_account,
            mint: accounts.input_vault_mint,
            destination: accounts.input_vault,
            authority: accounts.payer,
            amount: amount_in,
        },
        SplTransferLeg {
            source: accounts.output_vault,
            mint: accounts.output_vault_mint,
            destination: accounts.output_token_account,
            authority: accounts.pool_state,
            amount: expected_amount_out,
        },
    )
}
