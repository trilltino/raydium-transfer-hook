//! Errors framing a hooked swap instruction.

use std::fmt;

use solana_program::pubkey::Pubkey;

use super::leg::{LegField, LegRole};

/// Why a hook slice is not an authentic `(extras.., hook_program, validation_list)` tail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SliceFault {
    /// Fewer than two accounts.
    TooShort,
    /// The second-to-last account is not the hook program.
    TailNotHookProgram,
    /// The last account is not the validation list.
    TailNotValidationList,
    /// The validation list is not the canonical PDA for `(mint, hook_program)`.
    NonCanonicalValidationList { expected: Pubkey, found: Pubkey },
    /// The hook program or validation list is a signer or writable.
    TailPrivileged,
}

/// Where a conflicting account sits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictSite {
    /// A fixed (or tick/bitmap) account already in the instruction, by index.
    Fixed(usize),
    /// The other leg's hook slice.
    Leg(LegRole),
}

/// Errors from the framing API.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FrameError {
    /// The instruction data is not the expected V1 / SwapV2 layout.
    InvalidInstructionData,
    /// The instruction is already a framed V2 / V3 instruction.
    AlreadyFramed,
    InvalidFixedAccountCount {
        expected: usize,
        found: usize,
    },
    InvalidRemainingAccountSections,
    AccountCountOverflow,
    /// The leg's transfer does not match the swap instruction's accounts.
    LegMismatch {
        leg: LegRole,
        field: LegField,
        expected: Pubkey,
        found: Pubkey,
    },
    InvalidSlice {
        leg: LegRole,
        reason: SliceFault,
    },
    /// A slice account would escalate privileges of an account that is shared
    /// with the fixed accounts or the other slice. Solana unions flags per
    /// key across the whole transaction, so this would escalate the shared account.
    CrossSlicePrivilegeConflict {
        leg: LegRole,
        address: Pubkey,
        other: ConflictSite,
    },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInstructionData => f.write_str("unexpected Raydium instruction data"),
            Self::AlreadyFramed => f.write_str("instruction is already hook-framed"),
            Self::InvalidFixedAccountCount { expected, found } => write!(
                f,
                "Raydium instruction has {found} accounts, expected {expected}"
            ),
            Self::InvalidRemainingAccountSections => {
                f.write_str("CLMM tick and bitmap sections do not match remaining accounts")
            }
            Self::AccountCountOverflow => {
                f.write_str("Raydium remaining-account count exceeds the u16 framing limit")
            }
            Self::LegMismatch {
                leg,
                field,
                expected,
                found,
            } => write!(
                f,
                "{leg} leg {field:?} is {found} but the swap instruction uses {expected}"
            ),
            Self::InvalidSlice { leg, reason } => {
                write!(f, "{leg} leg hook slice is not authentic: {reason:?}")
            }
            Self::CrossSlicePrivilegeConflict {
                leg,
                address,
                other,
            } => write!(
                f,
                "{leg} leg slice account {address} escalates privileges shared with {other:?}"
            ),
        }
    }
}

impl std::error::Error for FrameError {}
