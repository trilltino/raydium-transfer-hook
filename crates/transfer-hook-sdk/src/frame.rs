//! Authentic framing of resolved hook slices into Raydium `V2` / `V3` instructions.
//!
//! Framing takes [`LegHook`]s produced by [`crate::resolve_leg`], never raw
//! account metas. Slices are appended exactly as resolved: they are never
//! merged, deduplicated, or reordered, because Raydium forwards each
//! transfer's slice to its own Token-2022 CPI.

use std::ops::Range;

use solana_program::instruction::{AccountMeta, Instruction};
use spl_transfer_hook_interface::get_extra_account_metas_address;

use crate::{
    abi::{
        clmm_index, cpmm_index, CLMM_SWAP_FIXED_ACCOUNTS, CLMM_SWAP_V2_DATA_LEN,
        CLMM_SWAP_V2_DISCRIMINATOR, CLMM_SWAP_V3_DISCRIMINATOR, CPMM_SWAP_BASE_INPUT_V1_DATA_LEN,
        CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR, CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR,
        CPMM_SWAP_FIXED_ACCOUNTS,
    },
    error::{ConflictSite, FrameError, LegField, SliceFault},
    resolve::{HookSlice, LegHook},
};

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

/// Where each leg's transfer accounts sit in the fixed account list.
struct LegLayout {
    source: usize,
    destination: usize,
    authority: usize,
    mint: usize,
}

const CPMM_INPUT: LegLayout = LegLayout {
    source: cpmm_index::INPUT_TOKEN_ACCOUNT,
    destination: cpmm_index::INPUT_VAULT,
    authority: cpmm_index::PAYER,
    mint: cpmm_index::INPUT_TOKEN_MINT,
};
const CPMM_OUTPUT: LegLayout = LegLayout {
    source: cpmm_index::OUTPUT_VAULT,
    destination: cpmm_index::OUTPUT_TOKEN_ACCOUNT,
    authority: cpmm_index::AUTHORITY,
    mint: cpmm_index::OUTPUT_TOKEN_MINT,
};
const CLMM_INPUT: LegLayout = LegLayout {
    source: clmm_index::INPUT_TOKEN_ACCOUNT,
    destination: clmm_index::INPUT_VAULT,
    authority: clmm_index::PAYER,
    mint: clmm_index::INPUT_VAULT_MINT,
};
const CLMM_OUTPUT: LegLayout = LegLayout {
    source: clmm_index::OUTPUT_VAULT,
    destination: clmm_index::OUTPUT_TOKEN_ACCOUNT,
    authority: clmm_index::POOL_STATE,
    mint: clmm_index::OUTPUT_VAULT_MINT,
};

fn check_leg_matches(
    leg: &LegHook,
    layout: &LegLayout,
    accounts: &[AccountMeta],
) -> Result<(), FrameError> {
    let transfer = leg.transfer();
    for (field, index, found) in [
        (LegField::Mint, layout.mint, transfer.mint),
        (LegField::Source, layout.source, transfer.source),
        (
            LegField::Destination,
            layout.destination,
            transfer.destination,
        ),
        (LegField::Authority, layout.authority, transfer.authority),
    ] {
        let expected = accounts[index].pubkey;
        if expected != found {
            return Err(FrameError::LegMismatch {
                leg: leg.role(),
                field,
                expected,
                found,
            });
        }
    }
    Ok(())
}

/// Re-derive everything about a slice that the resolver established.
fn check_slice_shape(leg: &LegHook, slice: &HookSlice) -> Result<(), FrameError> {
    let fault = |reason| FrameError::InvalidSlice {
        leg: leg.role(),
        reason,
    };
    let metas = slice.metas();
    if metas.len() < 2 {
        return Err(fault(SliceFault::TooShort));
    }
    let program = &metas[metas.len() - 2];
    let list = &metas[metas.len() - 1];
    if program.pubkey != slice.hook_program() {
        return Err(fault(SliceFault::TailNotHookProgram));
    }
    if list.pubkey != slice.validation_list() {
        return Err(fault(SliceFault::TailNotValidationList));
    }
    if program.is_signer || program.is_writable || list.is_signer || list.is_writable {
        return Err(fault(SliceFault::TailPrivileged));
    }
    let canonical = get_extra_account_metas_address(&leg.transfer().mint, &slice.hook_program());
    if canonical != slice.validation_list() {
        return Err(fault(SliceFault::NonCanonicalValidationList {
            expected: canonical,
            found: slice.validation_list(),
        }));
    }
    Ok(())
}

/// Solana merges account flags per key across the whole transaction, so a
/// slice account that shares a key with a fixed account or the other slice and
/// carries more privilege would escalate that account for the Raydium handler.
fn check_privilege_conflicts(
    accounts: &[AccountMeta],
    input: &LegHook,
    output: &LegHook,
) -> Result<(), FrameError> {
    for leg in [input, output] {
        let Some(slice) = leg.slice() else { continue };
        for meta in slice.metas() {
            for (index, fixed) in accounts.iter().enumerate() {
                if fixed.pubkey == meta.pubkey
                    && ((meta.is_signer && !fixed.is_signer)
                        || (meta.is_writable && !fixed.is_writable))
                {
                    return Err(FrameError::CrossSlicePrivilegeConflict {
                        leg: leg.role(),
                        address: meta.pubkey,
                        other: ConflictSite::Fixed(index),
                    });
                }
            }
        }
    }
    if let (Some(first), Some(second)) = (input.slice(), output.slice()) {
        for a in first.metas() {
            for b in second.metas() {
                if a.pubkey == b.pubkey
                    && (a.is_signer != b.is_signer || a.is_writable != b.is_writable)
                {
                    return Err(FrameError::CrossSlicePrivilegeConflict {
                        leg: output.role(),
                        address: a.pubkey,
                        other: ConflictSite::Leg(input.role()),
                    });
                }
            }
        }
    }
    Ok(())
}

fn hook_count(leg: &LegHook) -> Result<u16, FrameError> {
    u16::try_from(leg.account_count()).map_err(|_| FrameError::AccountCountOverflow)
}

fn validate_legs(
    accounts: &[AccountMeta],
    input: &LegHook,
    output: &LegHook,
    input_layout: &LegLayout,
    output_layout: &LegLayout,
) -> Result<(u16, u16), FrameError> {
    check_leg_matches(input, input_layout, accounts)?;
    check_leg_matches(output, output_layout, accounts)?;
    for leg in [input, output] {
        if let Some(slice) = leg.slice() {
            check_slice_shape(leg, slice)?;
        }
    }
    check_privilege_conflicts(accounts, input, output)?;
    Ok((hook_count(input)?, hook_count(output)?))
}

fn append_slices(
    instruction: &mut Instruction,
    input: &LegHook,
    output: &LegHook,
) -> (Range<usize>, Range<usize>) {
    let start = instruction.accounts.len();
    if let Some(slice) = input.slice() {
        instruction.accounts.extend_from_slice(slice.metas());
    }
    let middle = instruction.accounts.len();
    if let Some(slice) = output.slice() {
        instruction.accounts.extend_from_slice(slice.metas());
    }
    (start..middle, middle..instruction.accounts.len())
}

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
