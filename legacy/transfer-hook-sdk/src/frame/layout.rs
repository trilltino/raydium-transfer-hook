//! Where each leg's accounts sit in a swap instruction, and the checks every framer shares.

use std::ops::Range;

use solana_program::instruction::{AccountMeta, Instruction};
use spl_transfer_hook_interface::get_extra_account_metas_address;

use crate::{
    abi::{clmm_index, cpmm_index},
    error::{ConflictSite, FrameError, LegField, SliceFault},
    resolve::{HookSlice, LegHook},
};

/// Where each leg's transfer accounts sit in the fixed account list.
pub(super) struct LegLayout {
    pub(super) source: usize,
    pub(super) destination: usize,
    pub(super) authority: usize,
    pub(super) mint: usize,
}

pub(super) const CPMM_INPUT: LegLayout = LegLayout {
    source: cpmm_index::INPUT_TOKEN_ACCOUNT,
    destination: cpmm_index::INPUT_VAULT,
    authority: cpmm_index::PAYER,
    mint: cpmm_index::INPUT_TOKEN_MINT,
};
pub(super) const CPMM_OUTPUT: LegLayout = LegLayout {
    source: cpmm_index::OUTPUT_VAULT,
    destination: cpmm_index::OUTPUT_TOKEN_ACCOUNT,
    authority: cpmm_index::AUTHORITY,
    mint: cpmm_index::OUTPUT_TOKEN_MINT,
};
pub(super) const CLMM_INPUT: LegLayout = LegLayout {
    source: clmm_index::INPUT_TOKEN_ACCOUNT,
    destination: clmm_index::INPUT_VAULT,
    authority: clmm_index::PAYER,
    mint: clmm_index::INPUT_VAULT_MINT,
};
pub(super) const CLMM_OUTPUT: LegLayout = LegLayout {
    source: clmm_index::OUTPUT_VAULT,
    destination: clmm_index::OUTPUT_TOKEN_ACCOUNT,
    authority: clmm_index::POOL_STATE,
    mint: clmm_index::OUTPUT_VAULT_MINT,
};

pub(super) fn check_leg_matches(
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
pub(super) fn check_slice_shape(leg: &LegHook, slice: &HookSlice) -> Result<(), FrameError> {
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
pub(super) fn check_privilege_conflicts(
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

pub(super) fn hook_count(leg: &LegHook) -> Result<u16, FrameError> {
    u16::try_from(leg.account_count()).map_err(|_| FrameError::AccountCountOverflow)
}

pub(super) fn validate_legs(
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

pub(super) fn append_slices(
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
