//! Framing CPMM `swap_base_input` into `swap_base_input_v2`.

use solana_program::instruction::Instruction;

use crate::{
    abi::{
        CPMM_SWAP_BASE_INPUT_V1_DATA_LEN, CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR,
        CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR, CPMM_SWAP_FIXED_ACCOUNTS,
    },
    error::FrameError,
    resolve::LegHook,
};

use super::{layout::*, FramedAbi, FramedSwap};

fn check_cpmm_v1(instruction: &Instruction) -> Result<(), FrameError> {
    if instruction.data.len() >= 8 && instruction.data[..8] == CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR
    {
        return Err(FrameError::AlreadyFramed);
    }
    if instruction.data.len() != CPMM_SWAP_BASE_INPUT_V1_DATA_LEN
        || instruction.data[..8] != CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR
    {
        return Err(FrameError::InvalidInstructionData);
    }
    if instruction.accounts.len() != CPMM_SWAP_FIXED_ACCOUNTS {
        return Err(FrameError::InvalidFixedAccountCount {
            expected: CPMM_SWAP_FIXED_ACCOUNTS,
            found: instruction.accounts.len(),
        });
    }
    Ok(())
}

/// Convert a V1 CPMM `swap_base_input` instruction to `swap_base_input_v2`,
/// appending the input leg's slice then the output leg's slice verbatim.
///
/// Both legs may be unhooked, which yields an explicit V2 with zero counts. Use
/// [`frame_cpmm_or_passthrough`] to leave no-hook swaps as byte-identical V1.
/// The instruction is only modified on success.
pub fn frame_cpmm_swap_base_input_v2(
    instruction: &mut Instruction,
    input: &LegHook,
    output: &LegHook,
) -> Result<FramedSwap, FrameError> {
    check_cpmm_v1(instruction)?;
    let (input_count, output_count) = validate_legs(
        &instruction.accounts,
        input,
        output,
        &CPMM_INPUT,
        &CPMM_OUTPUT,
    )?;
    instruction.data[..8].copy_from_slice(&CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR);
    instruction
        .data
        .extend_from_slice(&input_count.to_le_bytes());
    instruction
        .data
        .extend_from_slice(&output_count.to_le_bytes());
    let (input_range, output_range) = append_slices(instruction, input, output);
    Ok(FramedSwap {
        abi: FramedAbi::CpmmSwapBaseInputV2,
        tick_array_count: 0,
        bitmap_count: 0,
        input_hook_accounts: input_count,
        output_hook_accounts: output_count,
        input_range,
        output_range,
    })
}

/// Like [`frame_cpmm_swap_base_input_v2`], but if neither leg is hooked the
/// instruction is validated and left untouched (still the byte-identical V1
/// swap) and `Ok(None)` is returned.
pub fn frame_cpmm_or_passthrough(
    instruction: &mut Instruction,
    input: &LegHook,
    output: &LegHook,
) -> Result<Option<FramedSwap>, FrameError> {
    if input.is_hooked() || output.is_hooked() {
        return frame_cpmm_swap_base_input_v2(instruction, input, output).map(Some);
    }
    check_cpmm_v1(instruction)?;
    validate_legs(
        &instruction.accounts,
        input,
        output,
        &CPMM_INPUT,
        &CPMM_OUTPUT,
    )?;
    Ok(None)
}
