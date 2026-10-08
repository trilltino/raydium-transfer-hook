//! # The rule: the one file you edit
//!
//! Everything else in this crate is Token-2022 Transfer Hook plumbing that already works:
//! the `Execute` entrypoint, the check that rejects direct calls, the canonical validation
//! list, the per-mint config account, authority modes, typed errors and versioning.
//!
//! The business rule lives here, in two functions:
//!
//! * [`validate_params`] runs when a creator initialises or updates a mint's configuration.
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
//!    clock, another account), add the account to the validation list in
//!    `config_extra_account_meta` and read it in `process_execute`; every extra account adds to
//!    the transaction size of each hooked transfer.
//! 3. Keep it bounded: no loops over holders, no unbounded accounts, deterministic errors.
//! 4. Return a [`crate::HookError`] so integrators can tell your hook caused the refusal.
//!
//! If your rule needs to change state (a counter, a timestamp), make that account writable in
//! the validation list. Do not move tokens from inside the hook: Token-2022 does not give a
//! hook authority over the transferred tokens.

use crate::{
    HookConfig, HookError, TransferContext, MAX_TRANSFER_PARAMS_LEN, MAX_TRANSFER_TEMPLATE_VERSION,
    TEMPLATE_MAX_TRANSFER_V1,
};

/// Validate the rule-specific `params` bytes (called at init and update).
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
    if config.template_id != TEMPLATE_MAX_TRANSFER_V1 {
        return Err(HookError::UnknownTemplate);
    }
    if config.template_version != MAX_TRANSFER_TEMPLATE_VERSION {
        return Err(HookError::UnsupportedVersion);
    }
    // YOUR BUSINESS LOGIC HERE. Inclusive: amount == limit passes.
    if context.amount > config.max_transfer_limit()? {
        return Err(HookError::TransferExceedsLimit);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{max_transfer_params, AuthorityMode};
    use solana_program::pubkey::Pubkey;

    const LIMIT: u64 = 500;

    fn config(limit: u64) -> HookConfig {
        HookConfig::new(
            255,
            255,
            AuthorityMode::ExtensionAuthority,
            TEMPLATE_MAX_TRANSFER_V1,
            MAX_TRANSFER_TEMPLATE_VERSION,
            Pubkey::new_unique(),
            Pubkey::default(),
            0,
            &max_transfer_params(limit),
        )
        .unwrap()
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
        assert_eq!(check_transfer(&config(LIMIT), &transfer(LIMIT - 1)), Ok(()));
    }

    #[test]
    fn a_transfer_exactly_at_the_limit_is_allowed() {
        assert_eq!(check_transfer(&config(LIMIT), &transfer(LIMIT)), Ok(()));
    }

    #[test]
    fn a_transfer_one_over_the_limit_is_rejected() {
        assert_eq!(
            check_transfer(&config(LIMIT), &transfer(LIMIT + 1)),
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

    #[test]
    fn a_config_for_another_rule_is_refused_not_guessed_at() {
        let mut other = config(LIMIT);
        other.template_id[0] ^= 1;
        assert_eq!(
            check_transfer(&other, &transfer(1)),
            Err(HookError::UnknownTemplate)
        );
    }
}
