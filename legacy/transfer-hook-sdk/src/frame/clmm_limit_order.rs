//! Framing the CLMM limit-order instructions into their hook-aware `_v2` versions.
//!
//! A limit order has an *input* token (the one the owner deposits and gets back when the order is
//! cancelled) and an *output* token (what the order's fills are paid in). Each instruction moves only
//! some of them:
//!
//! | Operation | Input token | Output token |
//! |---|---|---|
//! | open, increase | user to vault | not moved |
//! | decrease | vault to user (the unfilled part) | vault to user (the filled part) |
//! | settle | not moved | vault to user |
//!
//! The hook slices go at the **end** of the remaining accounts (the bitmap extension, if the
//! instruction takes one, stays where it is): the input transfer's slice, then the output transfer's.
//! The new instruction carries two `u16` counts after the original arguments, and a token the
//! instruction does not move must have a count of zero. In a [`FramedSwap`] "input" and "output" are
//! the order's input and output tokens.

use solana_program::instruction::Instruction;

use crate::{abi::anchor_instruction_discriminator, error::FrameError, resolve::LegHook};

use super::{
    layout::{
        check_leg_matches, check_privilege_conflicts, check_slice_shape, hook_count, LegLayout,
    },
    FramedAbi, FramedSwap,
};

/// A CLMM limit-order operation that transfers tokens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClmmLimitOrderOp {
    /// `open_limit_order`.
    Open,
    /// `increase_limit_order`.
    Increase,
    /// `decrease_limit_order`; it also settles the filled part first.
    Decrease,
    /// `settle_limit_order`.
    Settle,
}

impl ClmmLimitOrderOp {
    pub const ALL: [ClmmLimitOrderOp; 4] =
        [Self::Open, Self::Increase, Self::Decrease, Self::Settle];

    /// The Anchor instruction name of the original instruction (the one that rejects hooked mints).
    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "open_limit_order",
            Self::Increase => "increase_limit_order",
            Self::Decrease => "decrease_limit_order",
            Self::Settle => "settle_limit_order",
        }
    }

    /// The Anchor instruction name of the hook-aware instruction.
    pub fn framed_name(self) -> &'static str {
        match self {
            Self::Open => "open_limit_order_v2",
            Self::Increase => "increase_limit_order_v2",
            Self::Decrease => "decrease_limit_order_v2",
            Self::Settle => "settle_limit_order_v2",
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
            // nonce_index u8, zero_for_one bool, tick_index i32, amount u64
            Self::Open => 8 + 1 + 1 + 4 + 8,
            Self::Increase => 8 + 8,
            // amount, amount_min
            Self::Decrease => 8 + 8 + 8,
            Self::Settle => 8,
        }
    }

    /// The accounts in the original instruction's fixed list; the bitmap extension, if any, follows.
    pub fn fixed_accounts(self) -> usize {
        match self {
            Self::Open => 13,
            Self::Increase | Self::Settle => 8,
            Self::Decrease => 12,
        }
    }

    /// Whether the operation transfers the order's input token.
    pub fn moves_input(self) -> bool {
        !matches!(self, Self::Settle)
    }

    /// Whether the operation transfers the order's output token.
    pub fn moves_output(self) -> bool {
        matches!(self, Self::Decrease | Self::Settle)
    }

    /// Where the input and output transfers' accounts sit in the fixed list, `None` for a token the
    /// operation does not move.
    fn layouts(self) -> (Option<LegLayout>, Option<LegLayout>) {
        let layout = |(source, destination, authority, mint)| LegLayout {
            source,
            destination,
            authority,
            mint,
        };
        match self {
            // payer pays: input_token_account -> input_vault.
            Self::Open => (Some(layout((5, 7, 0, 9))), None),
            Self::Increase => (Some(layout((4, 5, 0, 6))), None),
            // the pool state signs: vault -> the owner's account.
            Self::Decrease => (Some(layout((6, 4, 1, 8))), Some(layout((7, 5, 1, 9)))),
            Self::Settle => (None, Some(layout((5, 4, 1, 6)))),
        }
    }
}

fn check_original(instruction: &Instruction, op: ClmmLimitOrderOp) -> Result<(), FrameError> {
    if instruction.data.len() >= 8 && instruction.data[..8] == op.framed_discriminator() {
        return Err(FrameError::AlreadyFramed);
    }
    if instruction.data.len() != op.data_len() || instruction.data[..8] != op.v1_discriminator() {
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

/// Check the legs against the operation and the instruction; return the two counts (0 for an unused
/// token).
fn validate(
    op: ClmmLimitOrderOp,
    instruction: &Instruction,
    input: Option<&LegHook>,
    output: Option<&LegHook>,
) -> Result<(u16, u16), FrameError> {
    use crate::error::LegRole;

    for (moved, leg, role) in [
        (op.moves_input(), input, LegRole::Input),
        (op.moves_output(), output, LegRole::Output),
    ] {
        if moved != leg.is_some() {
            return Err(FrameError::UnexpectedLeg { leg: role, moved });
        }
    }
    let (input_layout, output_layout) = op.layouts();
    let mut counts = [0u16; 2];
    for (index, (leg, layout)) in [(input, input_layout), (output, output_layout)]
        .into_iter()
        .enumerate()
    {
        let (Some(leg), Some(layout)) = (leg, layout) else {
            continue;
        };
        check_leg_matches(leg, &layout, &instruction.accounts)?;
        if let Some(slice) = leg.slice() {
            check_slice_shape(leg, slice)?;
        }
        counts[index] = hook_count(leg)?;
    }
    // With one leg the same leg stands for both sides: the cross-slice check then compares it with itself.
    match (input, output) {
        (Some(a), Some(b)) => check_privilege_conflicts(&instruction.accounts, a, b)?,
        (Some(a), None) | (None, Some(a)) => {
            check_privilege_conflicts(&instruction.accounts, a, a)?
        }
        (None, None) => {}
    }
    Ok((counts[0], counts[1]))
}

fn frame(
    op: ClmmLimitOrderOp,
    instruction: &mut Instruction,
    input: Option<&LegHook>,
    output: Option<&LegHook>,
) -> Result<FramedSwap, FrameError> {
    check_original(instruction, op)?;
    let (count_input, count_output) = validate(op, instruction, input, output)?;

    instruction.data[..8].copy_from_slice(&op.framed_discriminator());
    instruction
        .data
        .extend_from_slice(&count_input.to_le_bytes());
    instruction
        .data
        .extend_from_slice(&count_output.to_le_bytes());

    // The slices are the last accounts of the instruction.
    let start = instruction.accounts.len();
    if let Some(slice) = input.and_then(LegHook::slice) {
        instruction.accounts.extend_from_slice(slice.metas());
    }
    let middle = instruction.accounts.len();
    if let Some(slice) = output.and_then(LegHook::slice) {
        instruction.accounts.extend_from_slice(slice.metas());
    }
    let end = instruction.accounts.len();

    Ok(FramedSwap {
        abi: FramedAbi::ClmmLimitOrder(op),
        tick_array_count: 0,
        bitmap_count: 0,
        input_hook_accounts: count_input,
        output_hook_accounts: count_output,
        input_range: start..middle,
        output_range: middle..end,
    })
}

/// Convert an original CLMM limit-order instruction to its hook-aware version, appending the input
/// token's slice then the output token's after the instruction's own accounts. Pass `None` for a token
/// the operation does not move (see the table in the module documentation), and a leg for each it does,
/// hooked or not. The instruction is only modified on success.
pub fn frame_clmm_limit_order_v2(
    op: ClmmLimitOrderOp,
    instruction: &mut Instruction,
    input: Option<&LegHook>,
    output: Option<&LegHook>,
) -> Result<FramedSwap, FrameError> {
    frame(op, instruction, input, output)
}

/// Like [`frame_clmm_limit_order_v2`], but if no moved token is hooked the instruction is validated and
/// left as the original (`Ok(None)`).
pub fn frame_clmm_limit_order_or_passthrough(
    op: ClmmLimitOrderOp,
    instruction: &mut Instruction,
    input: Option<&LegHook>,
    output: Option<&LegHook>,
) -> Result<Option<FramedSwap>, FrameError> {
    if input.is_some_and(LegHook::is_hooked) || output.is_some_and(LegHook::is_hooked) {
        return frame(op, instruction, input, output).map(Some);
    }
    check_original(instruction, op)?;
    validate(op, instruction, input, output)?;
    Ok(None)
}
