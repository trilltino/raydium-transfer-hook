//! MODEL ONLY: not a Raydium CPI.
//!
//! Plans the two transfer legs of a CPMM `swap_base_input` and delegates the
//! framing to the SDK's authentic framers. This crate never builds or sends a
//! Raydium instruction by itself. Live entrypoints are `swap_base_input_v2`
//! (hooked) and the unchanged V1; deposits and withdrawals are rejected by the
//! program for hooked mints, so they have no plan here.

#![forbid(unsafe_code)]
// These return the SDK's rich `LegError` (it names the leg, mint and cause), which is larger than
// clippy's default for an `Err` variant. See `transfer-hook-sdk` for why it is not boxed.
#![allow(clippy::result_large_err)]

mod legs;
mod plan;
#[cfg(test)]
mod tests;

pub use legs::cpmm_swap_legs;
pub use plan::{plan_cpmm_swap_base_input, CpmmHookPlan};
