//! This template's errors, codes from `0xA001` (the shared plumbing uses `0x8001..`).

use solana_program::program_error::ProgramError;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitmentError {
    /// The schedule is backwards or its cliff lies outside it.
    InvalidSchedule = 0xA001,
    /// A commitment must lock something.
    ZeroLockedAmount = 0xA002,
    /// The creator account is not a token account of the hooked mint.
    CreatorAccountMismatch = 0xA003,
    /// The creator account holds less than the amount it is supposed to lock.
    InsufficientBalanceAtInit = 0xA004,
    /// The transfer would leave the creator account below the locked amount.
    VestingFloorBreached = 0xA005,
    /// The config account is not the one this mint's commitment created.
    InvalidConfig = 0xA006,
    /// An init instruction was malformed.
    InvalidInstruction = 0xA007,
}

impl CommitmentError {
    pub const fn code(self) -> u32 {
        self as u32
    }
}

impl From<CommitmentError> for ProgramError {
    fn from(error: CommitmentError) -> Self {
        ProgramError::Custom(error.code())
    }
}
