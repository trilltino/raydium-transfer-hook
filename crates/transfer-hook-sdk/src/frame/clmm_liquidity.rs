//! Framing the CLMM operations that move two tokens — opening a position, adding and removing
//! liquidity (which also collects fees), and the admin fee collections — into their hook-aware
//! instructions.
//!
//! Unlike a swap, these instructions already take remaining accounts (the tick-array bitmap
//! extension, reward accounts), so the hook slices go at the **end**: the instruction's own remaining
//! accounts stay where they are, then token_0's slice, then token_1's. The new instruction carries two
//! `u16` counts after the original arguments. [`ClmmLiquidityOp`] tabulates, per operation, the
//! discriminators, the size of the fixed account list and where each transfer's accounts sit.
//!
//! "Input" in a [`FramedSwap`] is token_0 here, and "output" is token_1.

use solana_program::instruction::Instruction;

use crate::{abi::anchor_instruction_discriminator, error::FrameError, resolve::LegHook};

use super::{
    layout::{validate_legs, LegLayout},
    FramedAbi, FramedSwap,
};

/// A CLMM operation that transfers token_0 and token_1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClmmLiquidityOp {
    /// `open_position_with_token22_nft`: the position NFT is a Token-2022 mint.
    OpenPositionWithToken22Nft,
    /// `open_position_v2`: the position NFT is a classic mint with metadata.
    OpenPosition,
    /// `increase_liquidity_v2`.
    IncreaseLiquidity,
    /// `decrease_liquidity_v2`; with zero liquidity it collects the position's fees.
    DecreaseLiquidity,
    CollectProtocolFee,
    CollectFundFee,
}

impl ClmmLiquidityOp {
    pub const ALL: [ClmmLiquidityOp; 6] = [
        Self::OpenPositionWithToken22Nft,
        Self::OpenPosition,
        Self::IncreaseLiquidity,
        Self::DecreaseLiquidity,
        Self::CollectProtocolFee,
        Self::CollectFundFee,
    ];

    /// The Anchor instruction name of the original instruction (the one that rejects hooked mints).
    pub fn name(self) -> &'static str {
        match self {
            Self::OpenPositionWithToken22Nft => "open_position_with_token22_nft",
            Self::OpenPosition => "open_position_v2",
            Self::IncreaseLiquidity => "increase_liquidity_v2",
            Self::DecreaseLiquidity => "decrease_liquidity_v2",
            Self::CollectProtocolFee => "collect_protocol_fee",
            Self::CollectFundFee => "collect_fund_fee",
        }
    }

    /// The Anchor instruction name of the hook-aware instruction.
    pub fn framed_name(self) -> &'static str {
        match self {
            Self::OpenPositionWithToken22Nft => "open_position_with_token22_nft_v3",
            Self::OpenPosition => "open_position_v3",
            Self::IncreaseLiquidity => "increase_liquidity_v3",
            Self::DecreaseLiquidity => "decrease_liquidity_v3",
            Self::CollectProtocolFee => "collect_protocol_fee_v2",
            Self::CollectFundFee => "collect_fund_fee_v2",
        }
    }

    pub fn v1_discriminator(self) -> [u8; 8] {
        anchor_instruction_discriminator(self.name())
    }

    pub fn framed_discriminator(self) -> [u8; 8] {
        anchor_instruction_discriminator(self.framed_name())
    }

    /// The least data the original instruction can have, discriminator included, and whether that
    /// is also its exact length. Opening a position and adding liquidity end in an `Option<bool>`
    /// (one or two bytes), so they only have a minimum.
    fn data_len(self) -> (usize, bool) {
        match self {
            // 4 x i32, u128, 2 x u64, `with_metadata`, and at least the `Option` tag.
            Self::OpenPositionWithToken22Nft | Self::OpenPosition => {
                (8 + 16 + 16 + 16 + 1 + 1, false)
            }
            // u128, 2 x u64, and at least the `Option` tag.
            Self::IncreaseLiquidity => (8 + 16 + 8 + 8 + 1, false),
            Self::DecreaseLiquidity => (8 + 16 + 8 + 8, true),
            Self::CollectProtocolFee | Self::CollectFundFee => (8 + 8 + 8, true),
        }
    }

    /// The accounts in the original instruction's fixed list; anything after them (bitmap extension,
    /// reward accounts) is kept, and the hook slices follow it.
    pub fn fixed_accounts(self) -> usize {
        match self {
            Self::OpenPositionWithToken22Nft => 20,
            Self::OpenPosition => 22,
            Self::IncreaseLiquidity => 15,
            Self::DecreaseLiquidity => 16,
            Self::CollectProtocolFee | Self::CollectFundFee => 11,
        }
    }

    /// Where token_0's and token_1's transfer accounts sit in the fixed list.
    fn layouts(self) -> (LegLayout, LegLayout) {
        // (source, destination, authority, mint) of the token_0 and token_1 transfers.
        let (a, b) = match self {
            // the payer pays: token account -> vault.
            Self::OpenPositionWithToken22Nft => ((9, 11, 0, 18), (10, 12, 0, 19)),
            Self::OpenPosition => ((10, 12, 0, 20), (11, 13, 0, 21)),
            Self::IncreaseLiquidity => ((7, 9, 0, 13), (8, 10, 0, 14)),
            // the pool state signs: vault -> recipient.
            Self::DecreaseLiquidity => ((5, 9, 3, 14), (6, 10, 3, 15)),
            Self::CollectProtocolFee | Self::CollectFundFee => ((3, 7, 1, 5), (4, 8, 1, 6)),
        };
        let layout = |(source, destination, authority, mint)| LegLayout {
            source,
            destination,
            authority,
            mint,
        };
        (layout(a), layout(b))
    }
}

fn check_original(instruction: &Instruction, op: ClmmLiquidityOp) -> Result<(), FrameError> {
    if instruction.data.len() >= 8 && instruction.data[..8] == op.framed_discriminator() {
        return Err(FrameError::AlreadyFramed);
    }
    let (len, exact) = op.data_len();
    let sized = if exact {
        instruction.data.len() == len
    } else {
        instruction.data.len() >= len
    };
    if !sized || instruction.data[..8] != op.v1_discriminator() {
        return Err(FrameError::InvalidInstructionData);
    }
    if instruction.accounts.len() < op.fixed_accounts() {
        return Err(FrameError::InvalidFixedAccountCount {
            expected: op.fixed_accounts(),
            found: instruction.accounts.len(),
        });
    }
    Ok(())
}

fn frame(
    op: ClmmLiquidityOp,
    instruction: &mut Instruction,
    token_0: &LegHook,
    token_1: &LegHook,
) -> Result<FramedSwap, FrameError> {
    check_original(instruction, op)?;
    let (layout_0, layout_1) = op.layouts();
    let (count_0, count_1) = validate_legs(
        &instruction.accounts,
        token_0,
        token_1,
        &layout_0,
        &layout_1,
    )?;

    instruction.data[..8].copy_from_slice(&op.framed_discriminator());
    instruction.data.extend_from_slice(&count_0.to_le_bytes());
    instruction.data.extend_from_slice(&count_1.to_le_bytes());

    // The slices are the last accounts of the instruction.
    let start = instruction.accounts.len();
    if let Some(slice) = token_0.slice() {
        instruction.accounts.extend_from_slice(slice.metas());
    }
    let middle = instruction.accounts.len();
    if let Some(slice) = token_1.slice() {
        instruction.accounts.extend_from_slice(slice.metas());
    }
    let end = instruction.accounts.len();

    Ok(FramedSwap {
        abi: FramedAbi::ClmmLiquidity(op),
        tick_array_count: 0,
        bitmap_count: 0,
        input_hook_accounts: count_0,
        output_hook_accounts: count_1,
        input_range: start..middle,
        output_range: middle..end,
    })
}

/// Convert an original CLMM two-token instruction to its hook-aware version, appending token_0's slice
/// then token_1's after the instruction's own accounts. Both legs may be unhooked, which yields an
/// explicit framed instruction with zero counts. The instruction is only modified on success.
pub fn frame_clmm_liquidity_v3(
    op: ClmmLiquidityOp,
    instruction: &mut Instruction,
    token_0: &LegHook,
    token_1: &LegHook,
) -> Result<FramedSwap, FrameError> {
    frame(op, instruction, token_0, token_1)
}

/// Like [`frame_clmm_liquidity_v3`], but if neither leg is hooked the instruction is validated and
/// left as the original (`Ok(None)`).
pub fn frame_clmm_liquidity_or_passthrough(
    op: ClmmLiquidityOp,
    instruction: &mut Instruction,
    token_0: &LegHook,
    token_1: &LegHook,
) -> Result<Option<FramedSwap>, FrameError> {
    if token_0.is_hooked() || token_1.is_hooked() {
        return frame(op, instruction, token_0, token_1).map(Some);
    }
    check_original(instruction, op)?;
    let (layout_0, layout_1) = op.layouts();
    validate_legs(
        &instruction.accounts,
        token_0,
        token_1,
        &layout_0,
        &layout_1,
    )?;
    Ok(None)
}
