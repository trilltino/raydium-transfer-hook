//! Authentic framing of resolved hook slices into Raydium `V2` / `V3` instructions.
//!
//! Framing takes [`LegHook`]s produced by [`crate::resolve_leg`], never raw
//! account metas. Slices are appended exactly as resolved: they are never
//! merged, deduplicated, or reordered, because Raydium forwards each
//! transfer's slice to its own Token-2022 CPI.

use std::ops::Range;

mod clmm;
mod cpmm;
mod layout;

pub use clmm::{frame_clmm_or_passthrough, frame_clmm_swap_v3};
pub use cpmm::{frame_cpmm_or_passthrough, frame_cpmm_swap_base_input_v2};

/// Which framed Raydium instruction was produced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FramedAbi {
    CpmmSwapBaseInputV2,
    ClmmSwapV3,
}

/// What `frame_*` did to the instruction, with the account ranges of each slice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FramedSwap {
    pub abi: FramedAbi,
    pub tick_array_count: u16,
    pub bitmap_count: u16,
    pub input_hook_accounts: u16,
    pub output_hook_accounts: u16,
    /// Range of the input slice within `instruction.accounts`.
    pub input_range: Range<usize>,
    /// Range of the output slice within `instruction.accounts`.
    pub output_range: Range<usize>,
}
