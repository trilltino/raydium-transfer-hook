//! This program's errors, codes from `0xF001` (the shared plumbing uses `0x8001..`).

use solana_program::program_error::ProgramError;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistryError {
    /// An instruction was malformed.
    InvalidInstruction = 0xF001,
    /// The descriptor account is malformed, or not at the address its contents imply.
    InvalidDescriptor = 0xF002,
    /// The hook program account is not an executable program.
    HookProgramNotExecutable = 0xF003,
    /// The signer is not the descriptor's template authority.
    NotTemplateAuthority = 0xF004,
}

impl RegistryError {
    pub const fn code(self) -> u32 {
        self as u32
    }
}

impl From<RegistryError> for ProgramError {
    fn from(error: RegistryError) -> Self {
        ProgramError::Custom(error.code())
    }
}
