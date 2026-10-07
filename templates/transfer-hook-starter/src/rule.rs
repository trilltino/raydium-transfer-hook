//! # The rule: the one file you edit
//!
//! Everything else in this crate is Token-2022 Transfer Hook plumbing that already works:
//! the `Execute` entrypoint, the check that rejects direct calls, the canonical validation
//! list, the per-mint config account, authority modes, typed errors and versioning.
//!
//! The commercial rule lives here, in two functions:
//!
//! * [`validate_params`] runs when a creator initialises or updates a mint's configuration.
//!   Reject parameters your rule cannot work with.
//! * [`check_transfer`] runs on every transfer of a hooked mint, inside the transfer. Return
//!   `Ok(())` to allow it or an error to refuse it. A refusal aborts the whole transaction
//!   (for example the Raydium swap that caused the transfer) and rolls every balance back.
//!
//! ## What the default rule does
//!
//! A maximum transfer size: `params` is one little-endian `u64` limit and any transfer above
//! it is refused (`amount == limit` is allowed).
//!
//! ## How to change it
//!
//! 1. Decide what per-mint settings you need and how they are laid out in `params` (up to
//!    [`crate::MAX_PARAMS_LEN`] bytes). Update `validate_params` and `HookConfig` helpers.
//! 2. Write the decision in `check_transfer`. You get the decoded per-mint config and the
//!    transfer amount. To look at more (the sender, the receiver, a clock), add the account
//!    to the validation list in `config_extra_account_meta` and read it in `process_execute`;
//!    every extra account adds to the transaction size of each hooked transfer.
//! 3. Keep it bounded: no loops over holders, no unbounded accounts, deterministic errors.
//! 4. Return a [`crate::HookError`] so integrators can tell your hook caused the refusal.
//!
//! If your rule needs to change state (a counter, a timestamp), make that account writable in
//! the validation list. Do not move tokens from inside the hook: Token-2022 does not give a
//! hook authority over the transferred tokens.

use crate::{
    HookConfig, HookError, MAX_TRANSFER_PARAMS_LEN, MAX_TRANSFER_TEMPLATE_VERSION,
    TEMPLATE_MAX_TRANSFER_V1,
};
use solana_program::entrypoint::ProgramResult;

/// Validate the template-specific `params` bytes (called at init and update).
pub fn validate_params(params: &[u8]) -> Result<(), HookError> {
    let bytes: [u8; MAX_TRANSFER_PARAMS_LEN] =
        params.try_into().map_err(|_| HookError::InvalidParams)?;
    if u64::from_le_bytes(bytes) == 0 {
        return Err(HookError::InvalidParams);
    }
    Ok(())
}

/// Decide a transfer. `amount` is the transferred token amount.
pub fn check_transfer(config: &HookConfig, amount: u64) -> ProgramResult {
    if config.template_id != TEMPLATE_MAX_TRANSFER_V1 {
        return Err(HookError::UnknownTemplate.into());
    }
    if config.template_version != MAX_TRANSFER_TEMPLATE_VERSION {
        return Err(HookError::UnsupportedVersion.into());
    }
    // Inclusive: amount == limit passes.
    if amount > config.max_transfer_limit()? {
        return Err(HookError::TransferExceedsLimit.into());
    }
    Ok(())
}
