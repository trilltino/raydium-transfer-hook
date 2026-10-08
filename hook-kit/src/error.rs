//! Errors raised by the shared plumbing. Codes start at `0x8001` so a hook built on the kit can
//! use its own range for rule violations and integrators can tell the two apart.

use solana_program::program_error::ProgramError;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KitError {
    /// The mint is not owned by Token-2022.
    MintNotToken2022 = 0x8001,
    /// The mint has no TransferHook extension.
    MintHookExtensionMissing = 0x8002,
    /// The mint's hook program is not this program.
    MintHookMismatch = 0x8003,
    /// The signer is not the mint's live TransferHook authority.
    AuthorityMismatch = 0x8004,
    /// The TransferHook authority has been revoked, so nobody can authorise setup.
    AuthorityUnavailable = 0x8005,
    /// A PDA this call would create already holds data or belongs to another program.
    AlreadyInitialized = 0x8006,
    /// `Execute` was not called by Token-2022 during a transfer.
    NotDirectInvocation = 0x8007,
    /// The account list is not the one the validation list declares.
    WrongAccountCount = 0x8008,
    /// A token account does not belong to the hooked mint.
    TokenAccountMismatch = 0x8009,
    /// The validation list is missing, malformed or not the canonical address.
    InvalidValidationList = 0x800a,
    /// The mint can still be minted, which the rule cannot allow.
    MintAuthorityNotRevoked = 0x800b,
}

impl KitError {
    pub const fn code(self) -> u32 {
        self as u32
    }
}

impl From<KitError> for ProgramError {
    fn from(error: KitError) -> Self {
        ProgramError::Custom(error.code())
    }
}
