//! Typed errors. Every failure the hook returns is a `ProgramError::Custom` carrying one of
//! these codes, so integrators can tell that this hook, and which rule of it, refused a transfer.

use solana_program::program_error::ProgramError;

/// Declares [`HookError`] together with [`HookError::ALL`] and [`HookError::from_code`], so a new
/// variant needs exactly one line and the three can never disagree. Add rule-specific variants at
/// the end of the list with the next free code; never renumber an existing one.
macro_rules! hook_errors {
    ($($(#[$doc:meta])* $name:ident = $code:literal,)+) => {
        /// Typed hook errors, surfaced as `ProgramError::Custom(code)`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum HookError {
            $($(#[$doc])* $name = $code,)+
        }

        impl HookError {
            /// Every variant, in declaration order.
            pub const ALL: &'static [HookError] = &[$(HookError::$name),+];

            /// Decode a `ProgramError::Custom` code back into a hook error.
            #[must_use]
            pub const fn from_code(code: u32) -> Option<HookError> {
                match code {
                    $($code => Some(HookError::$name),)+
                    _ => None,
                }
            }
        }
    };
}

hook_errors! {
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
    /// An `Execute` account is writable, or a token account belongs to another mint.
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
    #[must_use]
    pub const fn code(self) -> u32 {
        self as u32
    }
}

impl core::fmt::Display for HookError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?} (0x{:x})", self.code())
    }
}

impl std::error::Error for HookError {}

impl From<HookError> for ProgramError {
    fn from(error: HookError) -> Self {
        ProgramError::Custom(error.code())
    }
}
