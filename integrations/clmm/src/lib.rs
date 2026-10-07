//! MODEL ONLY: not a Raydium CPI.
//!
//! Plans the two transfer legs of a CLMM swap and delegates framing to the
//! SDK's `frame_clmm_swap_v3`. The live hooked entrypoint is `swap_v3`; this
//! crate does not execute it, and no CLMM hooked swap has been run on a
//! runtime. Tick arrays and the bitmap extension stay outside the per-leg hook
//! slices and are only counted here.

#![forbid(unsafe_code)]

mod legs;
mod plan;
#[cfg(test)]
mod tests;

pub use legs::clmm_swap_legs;
pub use plan::{plan_clmm_swap_v3, ClmmSwapV3Plan};
