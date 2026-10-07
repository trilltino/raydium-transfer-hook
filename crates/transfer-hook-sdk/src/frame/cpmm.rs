//! Framing CPMM `swap_base_input` into `swap_base_input_v2`, and `swap_base_output` into
//! `swap_base_output_v2`. The two pairs have the same account list and the same data shape (a
//! discriminator, two `u64`s, and for V2 two `u16` slice counts), so one implementation serves both.

use solana_program::instruction::Instruction;

use crate::{
    abi::{
        CPMM_SWAP_BASE_INPUT_V1_DATA_LEN, CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR,
        CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR, CPMM_SWAP_BASE_OUTPUT_V1_DISCRIMINATOR,
        CPMM_SWAP_BASE_OUTPUT_V2_DISCRIMINATOR, CPMM_SWAP_FIXED_ACCOUNTS,
    },
    error::FrameError,
    resolve::LegHook,
};

use super::{layout::*, FramedAbi, FramedSwap};

/// Which CPMM swap is being framed.
#[derive(Clone, Copy)]
struct Variant {
    v1: [u8; 8],
    v2: [u8; 8],
    abi: FramedAbi,
}

const BASE_INPUT: Variant = Variant {
    v1: CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR,
    v2: CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR,
    abi: FramedAbi::CpmmSwapBaseInputV2,
};

const BASE_OUTPUT: Variant = Variant {
    v1: CPMM_SWAP_BASE_OUTPUT_V1_DISCRIMINATOR,
    v2: CPMM_SWAP_BASE_OUTPUT_V2_DISCRIMINATOR,
    abi: FramedAbi::CpmmSwapBaseOutputV2,
};

fn check_cpmm_v1(instruction: &Instruction, variant: Variant) -> Result<(), FrameError> {
    if instruction.data.len() >= 8 && instruction.data[..8] == variant.v2 {
        return Err(FrameError::AlreadyFramed);
    }
    if instruction.data.len() != CPMM_SWAP_BASE_INPUT_V1_DATA_LEN
        || instruction.data[..8] != variant.v1
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

fn frame(
    instruction: &mut Instruction,
    input: &LegHook,
    output: &LegHook,
    variant: Variant,
) -> Result<FramedSwap, FrameError> {
    check_cpmm_v1(instruction, variant)?;
    let (input_count, output_count) = validate_legs(
        &instruction.accounts,
        input,
        output,
        &CPMM_INPUT,
        &CPMM_OUTPUT,
    )?;
    instruction.data[..8].copy_from_slice(&variant.v2);
    instruction
        .data
        .extend_from_slice(&input_count.to_le_bytes());
    instruction
        .data
        .extend_from_slice(&output_count.to_le_bytes());
    let (input_range, output_range) = append_slices(instruction, input, output);
    Ok(FramedSwap {
        abi: variant.abi,
        tick_array_count: 0,
        bitmap_count: 0,
        input_hook_accounts: input_count,
        output_hook_accounts: output_count,
        input_range,
        output_range,
    })
}

fn frame_or_passthrough(
    instruction: &mut Instruction,
    input: &LegHook,
    output: &LegHook,
    variant: Variant,
) -> Result<Option<FramedSwap>, FrameError> {
    if input.is_hooked() || output.is_hooked() {
        return frame(instruction, input, output, variant).map(Some);
    }
    check_cpmm_v1(instruction, variant)?;
    validate_legs(
        &instruction.accounts,
        input,
        output,
        &CPMM_INPUT,
        &CPMM_OUTPUT,
    )?;
    Ok(None)
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
    frame(instruction, input, output, BASE_INPUT)
}

/// Like [`frame_cpmm_swap_base_input_v2`], but if neither leg is hooked the
/// instruction is validated and left untouched (still the byte-identical V1
/// swap) and `Ok(None)` is returned.
pub fn frame_cpmm_or_passthrough(
    instruction: &mut Instruction,
    input: &LegHook,
    output: &LegHook,
) -> Result<Option<FramedSwap>, FrameError> {
    frame_or_passthrough(instruction, input, output, BASE_INPUT)
}

/// Convert a V1 CPMM `swap_base_output` (exact output) instruction to `swap_base_output_v2`,
/// appending the input leg's slice then the output leg's slice verbatim. The same rules as
/// [`frame_cpmm_swap_base_input_v2`]: the instruction is only modified on success.
pub fn frame_cpmm_swap_base_output_v2(
    instruction: &mut Instruction,
    input: &LegHook,
    output: &LegHook,
) -> Result<FramedSwap, FrameError> {
    frame(instruction, input, output, BASE_OUTPUT)
}

/// Like [`frame_cpmm_swap_base_output_v2`], but if neither leg is hooked the instruction is
/// validated and left as the byte-identical V1 `swap_base_output`.
pub fn frame_cpmm_output_or_passthrough(
    instruction: &mut Instruction,
    input: &LegHook,
    output: &LegHook,
) -> Result<Option<FramedSwap>, FrameError> {
    frame_or_passthrough(instruction, input, output, BASE_OUTPUT)
}
