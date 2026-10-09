//! This template's errors, codes from `0xC001` (the shared plumbing uses `0x8001..`).

use solana_program::program_error::ProgramError;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HolderRewardsError {
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
    /// A one-time allocation was already funded: it is funded once.
    AlreadyFunded = 0xC00E,
    /// A top-up would lower the rate of a stream that is still paying out.
    FundingLowersRate = 0xC00F,
    /// A mint carries a Token-2022 extension this rule cannot account for safely.
    UnsupportedMintExtension = 0xC010,
    /// `Reconcile` found nothing to correct: the token account still holds its counted balance.
    NothingToReconcile = 0xC011,
}

impl HolderRewardsError {
    #[must_use]
    pub const fn code(self) -> u32 {
        self as u32
    }
}

impl From<HolderRewardsError> for ProgramError {
    fn from(error: HolderRewardsError) -> Self {
        ProgramError::Custom(error.code())
    }
}

impl core::fmt::Display for HolderRewardsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?} (0x{:x})", self.code())
    }
}

impl std::error::Error for HolderRewardsError {}
