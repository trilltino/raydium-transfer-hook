//! Framing the CLMM reward-emission instructions, for reward mints that have a Transfer Hook.
//!
//! Three instructions move a reward token in or out of a reward vault, one transfer each:
//!
//! | Operation | Transfer |
//! |---|---|
//! | `initialize_reward` | funder to vault (funds the whole first period) |
//! | `set_reward_params` | funder to vault (the top-up, if the change needs one) |
//! | `collect_remaining_rewards` | vault to funder (what was never emitted) |
//!
//! Their hook-aware `_v2` versions take the transfer's hook slice as the **last** remaining accounts and
//! one `u16` count after the original arguments; everything the original takes stays where it is.
//!
//! Rewards are also paid to a position's owner by `decrease_liquidity` (with zero liquidity too). The
//! hook-aware version for pools whose reward mints may be hooked is `decrease_liquidity_v4`
//! ([`frame_clmm_decrease_with_rewards_v4`]): after its own accounts (including one group of reward vault,
//! recipient account and reward mint per initialised reward) it takes the slices of token 0, token 1 and
//! rewards 0 to 2 in that order, and five `u16` counts.

use std::ops::Range;

use solana_program::instruction::Instruction;

use crate::{abi::anchor_instruction_discriminator, error::FrameError, resolve::LegHook};

use super::{
    layout::{
        check_leg_matches, check_privilege_conflicts, check_slice_shape, hook_count, LegLayout,
    },
    ClmmLiquidityOp, FramedAbi, FramedSwap,
};

/// A CLMM reward operation that moves one reward token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClmmRewardOp {
    /// `initialize_reward`.
    InitializeReward,
    /// `set_reward_params`; its token accounts are remaining accounts, present when it tops up.
    SetRewardParams,
    /// `collect_remaining_rewards`.
    CollectRemainingRewards,
}

impl ClmmRewardOp {
    pub const ALL: [ClmmRewardOp; 3] = [
        Self::InitializeReward,
        Self::SetRewardParams,
        Self::CollectRemainingRewards,
    ];

    /// The Anchor instruction name of the original instruction (the one that rejects hooked mints).
    pub fn name(self) -> &'static str {
        match self {
            Self::InitializeReward => "initialize_reward",
            Self::SetRewardParams => "set_reward_params",
            Self::CollectRemainingRewards => "collect_remaining_rewards",
        }
    }

    /// The Anchor instruction name of the hook-aware instruction.
    pub fn framed_name(self) -> &'static str {
        match self {
            Self::InitializeReward => "initialize_reward_v2",
            Self::SetRewardParams => "set_reward_params_v2",
            Self::CollectRemainingRewards => "collect_remaining_rewards_v2",
        }
    }

    pub fn v1_discriminator(self) -> [u8; 8] {
        anchor_instruction_discriminator(self.name())
    }

    pub fn framed_discriminator(self) -> [u8; 8] {
        anchor_instruction_discriminator(self.framed_name())
    }

    /// The exact length of the original instruction's data, discriminator included.
    fn data_len(self) -> usize {
        match self {
            // open_time u64, end_time u64, emissions u128
            Self::InitializeReward => 8 + 8 + 8 + 16,
            // reward_index u8, emissions u128, open_time u64, end_time u64
            Self::SetRewardParams => 8 + 1 + 16 + 8 + 8,
            Self::CollectRemainingRewards => 8 + 1,
        }
    }

    /// The accounts the framer needs to find in the original instruction: for `set_reward_params` that
    /// includes its three remaining accounts (vault, authority token account, reward mint), which are
    /// present when the change needs a top-up.
    pub fn fixed_accounts(self) -> usize {
        match self {
            Self::InitializeReward => 10,
            Self::SetRewardParams => 6 + 3,
            Self::CollectRemainingRewards => 8,
        }
    }

    fn layout(self) -> LegLayout {
        let (source, destination, authority, mint) = match self {
            // reward_funder, funder_token_account, amm_config, pool_state, operation_state,
            // reward_token_mint, reward_token_vault, reward_token_program, system_program, rent
            Self::InitializeReward => (1, 6, 0, 5),
            // authority, amm_config, pool_state, operation_state, token_program, token_program_2022,
            // then (remaining) reward_token_vault, authority_token_account, reward_vault_mint
            Self::SetRewardParams => (7, 6, 0, 8),
            // reward_funder, funder_token_account, pool_state, reward_token_vault, reward_vault_mint,
            // token_program, token_program_2022, memo_program; the pool state signs.
            Self::CollectRemainingRewards => (3, 1, 2, 4),
        };
        LegLayout {
            source,
            destination,
            authority,
            mint,
        }
    }
}

fn check_original(
    instruction: &Instruction,
    framed: [u8; 8],
    original: [u8; 8],
    data_len: usize,
    fixed_accounts: usize,
) -> Result<(), FrameError> {
    if instruction.data.len() >= 8 && instruction.data[..8] == framed {
        return Err(FrameError::AlreadyFramed);
    }
    if instruction.data.len() != data_len || instruction.data[..8] != original {
        return Err(FrameError::InvalidInstructionData);
    }
    if instruction.accounts.len() < fixed_accounts {
        return Err(FrameError::InvalidFixedAccountCount {
            expected: fixed_accounts,
            found: instruction.accounts.len(),
        });
    }
    Ok(())
}

fn validate_one(
    instruction: &Instruction,
    layout: &LegLayout,
    leg: &LegHook,
) -> Result<u16, FrameError> {
    check_leg_matches(leg, layout, &instruction.accounts)?;
    if let Some(slice) = leg.slice() {
        check_slice_shape(leg, slice)?;
    }
    check_privilege_conflicts(&instruction.accounts, leg, leg)?;
    hook_count(leg)
}

/// Convert an original CLMM reward instruction to its hook-aware version: the reward transfer's slice is
/// appended after the instruction's own accounts and its count after the arguments. The instruction is only
/// modified on success. If the reward mint has no hook the instruction gets a count of zero.
pub fn frame_clmm_reward_v2(
    op: ClmmRewardOp,
    instruction: &mut Instruction,
    reward: &LegHook,
) -> Result<FramedSwap, FrameError> {
    check_original(
        instruction,
        op.framed_discriminator(),
        op.v1_discriminator(),
        op.data_len(),
        op.fixed_accounts(),
    )?;
    let count = validate_one(instruction, &op.layout(), reward)?;
    instruction.data[..8].copy_from_slice(&op.framed_discriminator());
    instruction.data.extend_from_slice(&count.to_le_bytes());
    let start = instruction.accounts.len();
    if let Some(slice) = reward.slice() {
        instruction.accounts.extend_from_slice(slice.metas());
    }
    let end = instruction.accounts.len();
    Ok(FramedSwap {
        abi: FramedAbi::ClmmReward(op),
        tick_array_count: 0,
        bitmap_count: 0,
        input_hook_accounts: count,
        output_hook_accounts: 0,
        input_range: start..end,
        output_range: end..end,
    })
}

/// Like [`frame_clmm_reward_v2`], but if the reward mint has no hook the instruction is validated and
/// left as the original (`Ok(None)`).
pub fn frame_clmm_reward_or_passthrough(
    op: ClmmRewardOp,
    instruction: &mut Instruction,
    reward: &LegHook,
) -> Result<Option<FramedSwap>, FrameError> {
    if reward.is_hooked() {
        return frame_clmm_reward_v2(op, instruction, reward).map(Some);
    }
    check_original(
        instruction,
        op.framed_discriminator(),
        op.v1_discriminator(),
        op.data_len(),
        op.fixed_accounts(),
    )?;
    validate_one(instruction, &op.layout(), reward)?;
    Ok(None)
}

/// What [`frame_clmm_decrease_with_rewards_v4`] did: the usual framing result for the two pool tokens and
/// where each reward's slice went.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FramedDecreaseWithRewards {
    pub framed: FramedSwap,
    /// The hook account count of rewards 0 to 2 (zero for an unhooked or missing reward).
    pub reward_hook_accounts: [u16; 3],
    pub reward_ranges: [Range<usize>; 3],
}

/// The accounts in front of the reward groups of a `decrease_liquidity_v2`: its fixed list.
const DECREASE_FIXED: usize = 16;

/// Convert an original `decrease_liquidity_v2` to `decrease_liquidity_v4`, which also runs the hooks of
/// the reward mints it pays out.
///
/// `rewards[i]` is the transfer of reward `i` for each of the pool's initialised rewards (rewards are
/// initialised from index 0, so there are as many legs as groups); the instruction must hold exactly one
/// group of (reward vault, recipient account, reward mint) per leg, starting at `groups_start`, which is
/// 16 (the fixed list) plus one if the tick-array bitmap extension is passed. The slices are appended in
/// the order token_0, token_1, reward 0, reward 1, reward 2.
pub fn frame_clmm_decrease_with_rewards_v4(
    instruction: &mut Instruction,
    token_0: &LegHook,
    token_1: &LegHook,
    rewards: &[&LegHook],
    groups_start: usize,
) -> Result<FramedDecreaseWithRewards, FrameError> {
    let op = ClmmLiquidityOp::DecreaseLiquidity;
    let framed_discriminator = anchor_instruction_discriminator("decrease_liquidity_v4");
    if rewards.len() > 3
        || groups_start < DECREASE_FIXED
        || instruction.accounts.len() != groups_start + 3 * rewards.len()
    {
        return Err(FrameError::InvalidRemainingAccountSections);
    }
    check_original(
        instruction,
        framed_discriminator,
        op.v1_discriminator(),
        8 + 16 + 8 + 8,
        DECREASE_FIXED,
    )?;

    // Token 0 and token 1 sit where `ClmmLiquidityOp::DecreaseLiquidity` has them.
    let mut legs: Vec<(&LegHook, LegLayout)> = vec![
        (
            token_0,
            LegLayout {
                source: 5,
                destination: 9,
                authority: 3,
                mint: 14,
            },
        ),
        (
            token_1,
            LegLayout {
                source: 6,
                destination: 10,
                authority: 3,
                mint: 15,
            },
        ),
    ];
    for (index, reward) in rewards.iter().enumerate() {
        let group = groups_start + 3 * index;
        // vault -> recipient, signed by the pool state, mint last in the group.
        legs.push((
            reward,
            LegLayout {
                source: group,
                destination: group + 1,
                authority: 3,
                mint: group + 2,
            },
        ));
    }
    let mut counts = [0u16; 5];
    for (index, (leg, layout)) in legs.iter().enumerate() {
        check_leg_matches(leg, layout, &instruction.accounts)?;
        if let Some(slice) = leg.slice() {
            check_slice_shape(leg, slice)?;
        }
        check_privilege_conflicts(&instruction.accounts, leg, leg)?;
        counts[index] = hook_count(leg)?;
    }
    // No two slices may give one account different privileges.
    for (i, (first, _)) in legs.iter().enumerate() {
        for (second, _) in legs.iter().skip(i + 1) {
            check_privilege_conflicts(&instruction.accounts, first, second)?;
        }
    }

    instruction.data[..8].copy_from_slice(&framed_discriminator);
    for count in counts {
        instruction.data.extend_from_slice(&count.to_le_bytes());
    }
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for (leg, _) in &legs {
        let start = instruction.accounts.len();
        if let Some(slice) = leg.slice() {
            instruction.accounts.extend_from_slice(slice.metas());
        }
        ranges.push(start..instruction.accounts.len());
    }
    let end = instruction.accounts.len();
    while ranges.len() < 5 {
        ranges.push(end..end);
    }
    Ok(FramedDecreaseWithRewards {
        framed: FramedSwap {
            abi: FramedAbi::ClmmDecreaseWithRewards,
            tick_array_count: 0,
            bitmap_count: 0,
            input_hook_accounts: counts[0],
            output_hook_accounts: counts[1],
            input_range: ranges[0].clone(),
            output_range: ranges[1].clone(),
        },
        reward_hook_accounts: [counts[2], counts[3], counts[4]],
        reward_ranges: [ranges[2].clone(), ranges[3].clone(), ranges[4].clone()],
    })
}
