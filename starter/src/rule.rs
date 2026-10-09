//! # The rule: start here
//!
//! Simple rules that fit the existing config may only need this file. Rules that introduce new
//! configuration, state or extra accounts must also update the corresponding config/account
//! plumbing and tests (`starter/README.md`, "When `rule.rs` is not enough").
//!
//! Everything else in this crate is Token-2022 Transfer Hook plumbing that already works:
//! the `Execute` entrypoint, the check that rejects direct calls, the canonical validation
//! list, the per-mint config account, typed errors and versioning.
//!
//! The business rule lives here, in two functions:
//!
//! * [`validate_params`] runs once, when the mint's hook authority initialises the hook.
//!   Reject parameters your rule cannot work with.
//! * [`check_transfer`] runs on every transfer of a hooked mint, inside the transfer. Return
//!   `Ok(())` to allow it or an error to refuse it. A refusal aborts the whole transaction
//!   (whatever program started the transfer: a wallet, a swap, a vault) and rolls every
//!   balance back.
//!
//! ## What the default rule does
//!
//! A maximum transfer size: `params` is one little-endian `u64` limit and any transfer above
//! it is refused (`amount == limit` is allowed).
//!
//! ## How to change it
//!
//! 1. Decide what per-mint settings you need and how they are laid out in `params` (up to
//!    [`crate::MAX_PARAMS_LEN`] bytes). Update `validate_params` and the `HookConfig` helpers.
//! 2. Write the decision in `check_transfer`. You get the decoded per-mint config and a
//!    [`TransferContext`] (amount, source, destination, mint, authority). To look at more (a
//!    clock, another account), declare it in the validation list (`CANONICAL_VALIDATION_LIST`
//!    and `VALIDATION_LIST_LEN` in `constants.rs`, `config_extra_account_meta` in `pda.rs`) and
//!    check and read it in `process_execute`. That is security plumbing: change it deliberately,
//!    with tests. Every extra account adds to the size and compute of each hooked transfer.
//! 3. Keep it bounded: no loops over holders, no unbounded accounts, deterministic errors.
//! 4. Return a [`crate::HookError`] so integrators can tell your hook caused the refusal.
//!
//! If your rule needs to change state (a counter, a timestamp), make that account writable in
//! the validation list; every transfer of the mint then contends for it. `Execute` gets the
//! transfer accounts read-only and without the transfer authority's signature, so the hook cannot
//! re-spend the transferred tokens through it.

use crate::{
    config::HookConfig, constants::MAX_TRANSFER_PARAMS_LEN, context::TransferContext,
    error::HookError,
};

/// Validate the rule-specific `params` bytes (called once, at init).
pub fn validate_params(params: &[u8]) -> Result<(), HookError> {
    let bytes: [u8; MAX_TRANSFER_PARAMS_LEN] =
        params.try_into().map_err(|_| HookError::InvalidParams)?;
    if u64::from_le_bytes(bytes) == 0 {
        return Err(HookError::InvalidParams);
    }
    Ok(())
}

/// Decide a transfer: `Ok(())` allows it, an error refuses it.
pub fn check_transfer(config: &HookConfig, context: &TransferContext) -> Result<(), HookError> {
    // YOUR BUSINESS LOGIC HERE. Inclusive: amount == limit passes.
    if context.amount > config.max_transfer_limit()? {
        return Err(HookError::TransferExceedsLimit);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::max_transfer_params;
    use solana_program::pubkey::Pubkey;

    const LIMIT: u64 = 500;

    fn check(limit: u64, amount: u64) -> Result<(), HookError> {
        let params = max_transfer_params(limit);
        let config = HookConfig::new(255, 255, Pubkey::new_unique(), &params).unwrap();
        check_transfer(&config, &transfer(amount))
    }

    fn transfer(amount: u64) -> TransferContext {
        TransferContext {
            amount,
            source: Pubkey::new_unique(),
            destination: Pubkey::new_unique(),
            mint: Pubkey::new_unique(),
            authority: Pubkey::new_unique(),
        }
    }

    #[test]
    fn a_transfer_below_the_limit_is_allowed() {
        assert_eq!(check(LIMIT, LIMIT - 1), Ok(()));
    }

    #[test]
    fn a_transfer_exactly_at_the_limit_is_allowed() {
        assert_eq!(check(LIMIT, LIMIT), Ok(()));
    }

    #[test]
    fn a_transfer_one_over_the_limit_is_rejected() {
        assert_eq!(
            check(LIMIT, LIMIT + 1),
            Err(HookError::TransferExceedsLimit)
        );
    }

    #[test]
    fn a_zero_limit_or_malformed_params_are_not_a_valid_configuration() {
        assert_eq!(validate_params(&max_transfer_params(LIMIT)), Ok(()));
        assert_eq!(
            validate_params(&max_transfer_params(0)),
            Err(HookError::InvalidParams)
        );
        assert_eq!(validate_params(&[1, 2, 3]), Err(HookError::InvalidParams));
        assert_eq!(validate_params(&[]), Err(HookError::InvalidParams));
    }
}
