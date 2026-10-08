//! Typed errors. Every failure the hook returns is a `ProgramError::Custom` carrying one of
//! these codes, so integrators can tell that this hook, and which rule of it, refused a transfer.

use solana_program::program_error::ProgramError;

/// Typed hook errors, surfaced as `ProgramError::Custom(code)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum HookError {
    /// Execute was called while the source or destination is not flagged `transferring`.
    NotDirectInvocation = 0x7001,
    /// The mint account is not owned by the Token-2022 program.
    MintOwnerNotToken2022 = 0x7002,
    /// The mint's TransferHook program id is not this program.
    MintHookProgramMismatch = 0x7003,
    /// The mint has no TransferHook extension.
    MintHookExtensionMissing = 0x7004,
    /// The config account is not the config PDA of the given mint.
    InvalidConfigPda = 0x7005,
    /// The config account is not owned by this program.
    InvalidConfigOwner = 0x7006,
    /// The config bytes are malformed (length, discriminator, field values).
    InvalidConfigData = 0x7007,
    /// The config version is not supported (fail closed).
    UnsupportedVersion = 0x7008,
    /// The validation list is not the canonical list of the mint, or is malformed.
    InvalidValidationList = 0x7009,
    /// The resolved extra accounts do not match the validation list.
    AccountOrderMismatch = 0x700a,
    /// The transfer amount is above the configured limit. This is the default rule's refusal.
    TransferExceedsLimit = 0x700b,
    /// The signer is not the mint's Transfer Hook extension authority.
    AuthorityMismatch = 0x700c,
    /// The mint's Transfer Hook extension authority has been revoked, so nobody can initialise.
    AuthorityUnavailable = 0x700d,
    /// The config or validation list already exists.
    AlreadyInitialized = 0x700e,
    /// `params_len` is above 256.
    ParamsTooLarge = 0x700f,
    /// Execute was invoked with a number of accounts other than six.
    WrongAccountCount = 0x7010,
    /// The rule's params are invalid.
    InvalidParams = 0x7011,
    /// SPL InitializeExtraAccountMetaList / UpdateExtraAccountMetaList are not accepted.
    SplInterfaceUnsupported = 0x7012,
}

impl HookError {
    pub const ALL: [HookError; 18] = [
        HookError::NotDirectInvocation,
        HookError::MintOwnerNotToken2022,
        HookError::MintHookProgramMismatch,
        HookError::MintHookExtensionMissing,
        HookError::InvalidConfigPda,
        HookError::InvalidConfigOwner,
        HookError::InvalidConfigData,
        HookError::UnsupportedVersion,
        HookError::InvalidValidationList,
        HookError::AccountOrderMismatch,
        HookError::TransferExceedsLimit,
        HookError::AuthorityMismatch,
        HookError::AuthorityUnavailable,
        HookError::AlreadyInitialized,
        HookError::ParamsTooLarge,
        HookError::WrongAccountCount,
        HookError::InvalidParams,
        HookError::SplInterfaceUnsupported,
    ];

    pub const fn code(self) -> u32 {
        self as u32
    }

    /// Decode a `ProgramError::Custom` code back into a hook error.
    pub fn from_code(code: u32) -> Option<HookError> {
        Self::ALL.into_iter().find(|error| error.code() == code)
    }

    pub const fn name(self) -> &'static str {
        match self {
            HookError::NotDirectInvocation => "NotDirectInvocation",
            HookError::MintOwnerNotToken2022 => "MintOwnerNotToken2022",
            HookError::MintHookProgramMismatch => "MintHookProgramMismatch",
            HookError::MintHookExtensionMissing => "MintHookExtensionMissing",
            HookError::InvalidConfigPda => "InvalidConfigPda",
            HookError::InvalidConfigOwner => "InvalidConfigOwner",
            HookError::InvalidConfigData => "InvalidConfigData",
            HookError::UnsupportedVersion => "UnsupportedVersion",
            HookError::InvalidValidationList => "InvalidValidationList",
            HookError::AccountOrderMismatch => "AccountOrderMismatch",
            HookError::TransferExceedsLimit => "TransferExceedsLimit",
            HookError::AuthorityMismatch => "AuthorityMismatch",
            HookError::AuthorityUnavailable => "AuthorityUnavailable",
            HookError::AlreadyInitialized => "AlreadyInitialized",
            HookError::ParamsTooLarge => "ParamsTooLarge",
            HookError::WrongAccountCount => "WrongAccountCount",
            HookError::InvalidParams => "InvalidParams",
            HookError::SplInterfaceUnsupported => "SplInterfaceUnsupported",
        }
    }
}

impl core::fmt::Display for HookError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} (0x{:x})", self.name(), self.code())
    }
}

impl std::error::Error for HookError {}

impl From<HookError> for ProgramError {
    fn from(error: HookError) -> Self {
        ProgramError::Custom(error.code())
    }
}
