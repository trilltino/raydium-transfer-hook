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
    /// The config bytes are malformed (length, discriminator, reserved bytes, field values).
    InvalidConfigData = 0x7007,
    /// The config version is not supported (fail closed).
    UnsupportedVersion = 0x7008,
    /// The validation list is not the canonical list of the mint, or is malformed.
    InvalidValidationList = 0x7009,
    /// The resolved extra accounts do not match the validation list.
    AccountOrderMismatch = 0x700a,
    /// The transfer amount is above the configured limit.
    TransferExceedsLimit = 0x700b,
    /// The signer is not the authority required by the config's authority mode.
    AuthorityMismatch = 0x700c,
    /// The authority required by the mode is `None` on the mint.
    AuthorityUnavailable = 0x700d,
    /// The config or validation list already exists.
    AlreadyInitialized = 0x700e,
    /// The authority mode is reserved or unknown, or not valid for this instruction.
    UnsupportedMode = 0x700f,
    /// `params_len` is above 256.
    ParamsTooLarge = 0x7010,
    /// `config_hash` does not match the stored template and params.
    HashMismatch = 0x7011,
    /// `expected_seq` is not the current `config_seq`.
    StaleConfigSeq = 0x7012,
    /// Execute was invoked with a number of accounts other than six.
    WrongAccountCount = 0x7013,
    /// The config is in Immutable mode and cannot be changed.
    ConfigImmutable = 0x7014,
    /// Template params or flags are invalid for the template.
    InvalidParams = 0x7015,
    /// The template id is not known to this program.
    UnknownTemplate = 0x7016,
    /// SPL InitializeExtraAccountMetaList / UpdateExtraAccountMetaList are not accepted.
    SplInterfaceUnsupported = 0x7017,
}

impl HookError {
    pub const ALL: [HookError; 23] = [
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
        HookError::UnsupportedMode,
        HookError::ParamsTooLarge,
        HookError::HashMismatch,
        HookError::StaleConfigSeq,
        HookError::WrongAccountCount,
        HookError::ConfigImmutable,
        HookError::InvalidParams,
        HookError::UnknownTemplate,
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
            HookError::UnsupportedMode => "UnsupportedMode",
            HookError::ParamsTooLarge => "ParamsTooLarge",
            HookError::HashMismatch => "HashMismatch",
            HookError::StaleConfigSeq => "StaleConfigSeq",
            HookError::WrongAccountCount => "WrongAccountCount",
            HookError::ConfigImmutable => "ConfigImmutable",
            HookError::InvalidParams => "InvalidParams",
            HookError::UnknownTemplate => "UnknownTemplate",
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
