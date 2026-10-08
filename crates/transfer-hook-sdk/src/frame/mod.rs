//! Authentic framing of resolved hook slices into Raydium `V2` / `V3` instructions.
//!
//! Framing takes [`LegHook`]s produced by [`crate::resolve_leg`], never raw
//! account metas. Slices are appended exactly as resolved: they are never
//! merged, deduplicated, or reordered, because Raydium forwards each
//! transfer's slice to its own Token-2022 CPI.

use std::ops::Range;

mod clmm;
mod clmm_limit_order;
mod clmm_liquidity;
mod clmm_rewards;
mod cpmm;
mod cpmm_pair;
mod layout;

pub use clmm::{frame_clmm_or_passthrough, frame_clmm_swap_v3};
pub use clmm_limit_order::{
    frame_clmm_limit_order_or_passthrough, frame_clmm_limit_order_v2, ClmmLimitOrderOp,
};
pub use clmm_liquidity::{
    frame_clmm_liquidity_or_passthrough, frame_clmm_liquidity_v3, ClmmLiquidityOp,
};
pub use clmm_rewards::{
    frame_clmm_decrease_with_rewards_v4, frame_clmm_reward_or_passthrough, frame_clmm_reward_v2,
    ClmmRewardOp, FramedDecreaseWithRewards,
};
pub use cpmm::{
    frame_cpmm_or_passthrough, frame_cpmm_output_or_passthrough, frame_cpmm_swap_base_input_v2,
    frame_cpmm_swap_base_output_v2,
};
pub use cpmm_pair::{frame_cpmm_pair_or_passthrough, frame_cpmm_pair_v2, CpmmPairOp};

/// Which framed Raydium instruction was produced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FramedAbi {
    CpmmSwapBaseInputV2,
    CpmmSwapBaseOutputV2,
    /// A two-token CPMM operation other than a swap: liquidity, fee collection, pool creation.
    CpmmPair(CpmmPairOp),
    ClmmSwapV3,
    /// A two-token CLMM operation other than a swap: positions, liquidity, fee collection.
    ClmmLiquidity(ClmmLiquidityOp),
    /// A CLMM limit-order operation: open, increase, decrease, settle.
    ClmmLimitOrder(ClmmLimitOrderOp),
    /// A CLMM reward instruction that moves one reward token: funding, top-up, remaining rewards.
    ClmmReward(ClmmRewardOp),
    /// `decrease_liquidity_v4`: a decrease that also pays rewards through their hooks.
    ClmmDecreaseWithRewards,
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
