//! Typed errors (`ProgramError::Custom`, codes from `0x9001`).

use solana_program::program_error::ProgramError;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArbError {
    /// At most `max_per_slot` transfers per slot.
    SlotLimitExceeded = 0x9001,
    /// Execute was not invoked by Token-2022 during a transfer.
    NotDirectInvocation = 0x9002,
    WrongAccountCount = 0x9003,
    InvalidPolicy = 0x9004,
    InvalidStats = 0x9005,
    MintHookMismatch = 0x9006,
    AuthorityMismatch = 0x9007,
    AlreadyInitialized = 0x9008,
    InvalidValidationList = 0x9009,
    MintNotToken2022 = 0x900a,
    StatsNotWritable = 0x900b,
    Overflow = 0x900c,
    InvalidParams = 0x900d,
}

impl ArbError {
    pub const fn code(self) -> u32 {
        self as u32
    }
}

impl From<ArbError> for ProgramError {
    fn from(error: ArbError) -> Self {
        ProgramError::Custom(error.code())
    }
}
