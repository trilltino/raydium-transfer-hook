//! Framing CLMM `swap_v2` into `swap_v3`.

use solana_program::instruction::Instruction;

use crate::{
    abi::{
        CLMM_SWAP_FIXED_ACCOUNTS, CLMM_SWAP_V2_DATA_LEN, CLMM_SWAP_V2_DISCRIMINATOR,
        CLMM_SWAP_V3_DISCRIMINATOR,
    },
    error::FrameError,
    resolve::LegHook,
};

use super::{layout::*, FramedAbi, FramedSwap};

fn check_clmm_v2(
    instruction: &Instruction,
    tick_array_count: u16,
    bitmap_count: u16,
) -> Result<(), FrameError> {
    if instruction.data.len() >= 8 && instruction.data[..8] == CLMM_SWAP_V3_DISCRIMINATOR {
        return Err(FrameError::AlreadyFramed);
    }
    if instruction.data.len() != CLMM_SWAP_V2_DATA_LEN
        || instruction.data[..8] != CLMM_SWAP_V2_DISCRIMINATOR
    {
        return Err(FrameError::InvalidInstructionData);
    }
    if instruction.accounts.len() < CLMM_SWAP_FIXED_ACCOUNTS {
        return Err(FrameError::InvalidFixedAccountCount {
            expected: CLMM_SWAP_FIXED_ACCOUNTS,
            found: instruction.accounts.len(),
        });
    }
    let ticks = usize::from(tick_array_count);
    let bitmaps = usize::from(bitmap_count);
    if bitmaps > 1
        || CLMM_SWAP_FIXED_ACCOUNTS
            .checked_add(ticks)
            .and_then(|count| count.checked_add(bitmaps))
            != Some(instruction.accounts.len())
    {
        return Err(FrameError::InvalidRemainingAccountSections);
    }
    Ok(())
}

/// Convert a CLMM `swap_v2` instruction to `swap_v3`. The instruction's
/// existing remaining accounts must be exactly the declared tick arrays then
/// the optional bitmap extension. Slices are appended input-then-output.
/// The instruction is only modified on success.
pub fn frame_clmm_swap_v3(
    instruction: &mut Instruction,
    tick_array_count: u16,
    bitmap_count: u16,
    input: &LegHook,
    output: &LegHook,
) -> Result<FramedSwap, FrameError> {
    check_clmm_v2(instruction, tick_array_count, bitmap_count)?;
    let (input_count, output_count) = validate_legs(
        &instruction.accounts,
        input,
        output,
        &CLMM_INPUT,
        &CLMM_OUTPUT,
    )?;
    instruction.data[..8].copy_from_slice(&CLMM_SWAP_V3_DISCRIMINATOR);
    for count in [tick_array_count, bitmap_count, input_count, output_count] {
        instruction.data.extend_from_slice(&count.to_le_bytes());
    }
    let (input_range, output_range) = append_slices(instruction, input, output);
    Ok(FramedSwap {
        abi: FramedAbi::ClmmSwapV3,
        tick_array_count,
        bitmap_count,
        input_hook_accounts: input_count,
        output_hook_accounts: output_count,
        input_range,
        output_range,
    })
}

/// Like [`frame_clmm_swap_v3`], but a swap with no hooked leg is validated and
/// left as the byte-identical `swap_v2` (`Ok(None)`).
pub fn frame_clmm_or_passthrough(
    instruction: &mut Instruction,
    tick_array_count: u16,
    bitmap_count: u16,
    input: &LegHook,
    output: &LegHook,
) -> Result<Option<FramedSwap>, FrameError> {
    if input.is_hooked() || output.is_hooked() {
        return frame_clmm_swap_v3(instruction, tick_array_count, bitmap_count, input, output)
            .map(Some);
    }
    check_clmm_v2(instruction, tick_array_count, bitmap_count)?;
    validate_legs(
        &instruction.accounts,
        input,
        output,
        &CLMM_INPUT,
        &CLMM_OUTPUT,
    )?;
    Ok(None)
}
