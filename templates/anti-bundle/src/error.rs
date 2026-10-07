//! This template's errors, codes from `0xD001` (the shared plumbing uses `0x8001..`).

use solana_program::program_error::ProgramError;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AntiBundleError {
    /// A zero budget.
    InvalidParams = 0xD001,
    /// No venues, more than the supported maximum, or a venue listed twice.
    InvalidVenues = 0xD002,
    /// A venue is not a token account of the hooked mint.
    VenueMismatch = 0xD003,
    /// More buys in one slot than the budget allows.
    TooManyBuysInSlot = 0xD004,
    /// The config account is not the one this mint's setup created.
    InvalidConfig = 0xD005,
    /// An init instruction was malformed.
    InvalidInstruction = 0xD006,
    /// The slot-counter account is wrong, malformed, or not writable.
    InvalidState = 0xD007,
}

impl AntiBundleError {
    pub const fn code(self) -> u32 {
        self as u32
    }
}

impl From<AntiBundleError> for ProgramError {
    fn from(error: AntiBundleError) -> Self {
        ProgramError::Custom(error.code())
    }
}
