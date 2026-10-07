//! This template's own errors, codes from `0xE001`. Everything else it can return comes from the
//! accounting it reuses: `loyalty-rewards` (`0xC001..`) and the shared plumbing (`0x8001..`).

use solana_program::program_error::ProgramError;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpinOffError {
    /// The child allocation was already funded: a spin-off is funded once.
    AlreadyFunded = 0xE001,
}

impl SpinOffError {
    pub const fn code(self) -> u32 {
        self as u32
    }
}

impl From<SpinOffError> for ProgramError {
    fn from(error: SpinOffError) -> Self {
        ProgramError::Custom(error.code())
    }
}
