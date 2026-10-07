//! This template's errors, codes from `0xC001` (the shared plumbing uses `0x8001..`).

use solana_program::program_error::ProgramError;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoyaltyError {
    /// An instruction was malformed.
    InvalidInstruction = 0xC001,
    /// The global account is not the one this mint's rewards created.
    InvalidGlobal = 0xC002,
    /// The holder record is not the one for this token account.
    InvalidRecord = 0xC003,
    /// The pool's vault cannot earn rewards.
    ExcludedAccount = 0xC004,
    /// The token account has no holder record: it was never registered.
    NotRegistered = 0xC005,
    /// An amount of zero.
    ZeroAmount = 0xC006,
    /// A reward period of zero, or longer than the supported maximum.
    InvalidDuration = 0xC007,
    /// A fixed-point calculation overflowed.
    MathOverflow = 0xC008,
    /// The reward mint carries a TransferHook of its own.
    RewardMintHasHook = 0xC009,
    /// The reward vault, reward mint or token program is not the one the rewards were set up with.
    RewardAccountMismatch = 0xC00A,
    /// The signer does not own the token account.
    WrongOwner = 0xC00B,
    /// Nothing to claim yet.
    NothingToClaim = 0xC00C,
    /// The pool vault is not a token account of the hooked mint.
    PoolVaultMismatch = 0xC00D,
}

impl LoyaltyError {
    pub const fn code(self) -> u32 {
        self as u32
    }
}

impl From<LoyaltyError> for ProgramError {
    fn from(error: LoyaltyError) -> Self {
        ProgramError::Custom(error.code())
    }
}
