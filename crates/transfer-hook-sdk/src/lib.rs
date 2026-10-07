//! Client SDK for Token-2022 Transfer Hook accounts in Raydium CPMM / CLMM swaps.
//!
//! The flow is:
//!
//! 1. [`resolve_leg`] / [`resolve_legs`] resolve each transfer's hook tail
//!    (`extras.., hook_program, validation_list`) with the official SPL
//!    resolver, on a scratch instruction, and return typed [`LegHook`]s or an
//!    attributed [`LegError`]. Nothing the caller owns is mutated.
//! 2. `frame_*` append the validated slices to a V1 / SwapV2 instruction and
//!    switch it to the framed V2 / V3 layout. Slices are never merged.
//! 3. [`HookFingerprint::verify_unchanged`] (or [`verify_legs_unchanged`])
//!    re-checks the hook right before signing.
//!
//! There is deliberately no second, caller-implemented resolver and no
//! invented validation-list address: the PDA is always
//! `spl_transfer_hook_interface::get_extra_account_metas_address`.

#![forbid(unsafe_code)]

pub mod abi;
pub mod error;
pub mod frame;
pub mod resolve;
#[cfg(any(test, feature = "test-utils"))]
pub mod testing;

use std::future::Future;

use solana_program::pubkey::Pubkey;

pub use {solana_program, spl_tlv_account_resolution, spl_token, spl_token_2022};

pub use abi::{
    anchor_instruction_discriminator, build_clmm_swap_v2, build_cpmm_swap_base_input_v1,
    ClmmSwapAccounts, ClmmSwapArgs, CpmmSwapAccounts, CLMM_SWAP_FIXED_ACCOUNTS,
    CLMM_SWAP_V2_DISCRIMINATOR, CLMM_SWAP_V3_DISCRIMINATOR, CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR,
    CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR, CPMM_SWAP_FIXED_ACCOUNTS,
};
pub use error::{
    AuthorityExpectation, ConflictSite, FetchError, FrameError, HookChangeKind,
    HookProgramInvalidReason, LegError, LegField, LegRole, SliceFault, SplResolveError,
};
pub use frame::{
    frame_clmm_or_passthrough, frame_clmm_swap_v3, frame_cpmm_or_passthrough,
    frame_cpmm_swap_base_input_v2, FramedAbi, FramedSwap,
};
pub use resolve::{
    default_allowed_loaders, resolve_leg, resolve_legs, HookFingerprint, HookSlice, LegHook,
    PrivilegePolicy, ProgramFingerprint, ResolveOptions, SplAccount, SplTransferLeg, LOADER_V4_ID,
    RAYDIUM_PROGRAM_IDS,
};

/// Verify every hooked leg is still exactly as resolved. Call immediately before signing.
pub async fn verify_legs_unchanged<F, Fut, E>(legs: &[&LegHook], fetch: F) -> Result<(), LegError>
where
    F: Fn(Pubkey) -> Fut,
    Fut: Future<Output = Result<Option<SplAccount>, E>>,
    E: Into<FetchError>,
{
    for leg in legs {
        leg.verify_unchanged(&fetch).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
