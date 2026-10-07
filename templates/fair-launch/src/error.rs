//! This template's errors, codes from `0xB001` (the shared plumbing uses `0x8001..`).

use solana_program::program_error::ProgramError;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FairLaunchError {
    /// An empty window or a zero limit.
    InvalidParams = 0xB001,
    /// The pool vault is not a token account of the hooked mint.
    PoolVaultMismatch = 0xB002,
    /// The buy is larger than the per-buy cap.
    PerBuyCapExceeded = 0xB003,
    /// The buyer's balance after the buy would exceed the per-wallet cap.
    MaxWalletExceeded = 0xB004,
    /// More buys in one slot than the launch allows.
    TooManyBuysInSlot = 0xB005,
    /// The transaction declares a priority fee above the cap.
    PriorityFeeTooHigh = 0xB006,
    /// The config account is not the one this mint's launch created.
    InvalidConfig = 0xB007,
    /// An init instruction was malformed.
    InvalidInstruction = 0xB008,
    /// The slot-counter account is wrong, malformed, or not writable.
    InvalidCounter = 0xB009,
    /// The instructions sysvar account is missing or wrong.
    InvalidSysvar = 0xB00a,
}

impl FairLaunchError {
    pub const fn code(self) -> u32 {
        self as u32
    }
}

impl From<FairLaunchError> for ProgramError {
    fn from(error: FairLaunchError) -> Self {
        ProgramError::Custom(error.code())
    }
}
