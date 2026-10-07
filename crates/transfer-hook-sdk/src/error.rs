//! Structured, assertable errors for resolution and framing.
//!
//! Every error type is `Clone + PartialEq + Eq` and `#[non_exhaustive]` where
//! new variants are expected, so callers can both match on them in tests and
//! keep compiling when variants are added.

use std::fmt;

use solana_program::{program_error::ProgramError, pubkey::Pubkey};

/// A failure reported by the caller-supplied account fetcher.
///
/// The fetcher is an arbitrary async closure (RPC client, bank client, test
/// map), so its native error type is erased to a message. Keeping a plain
/// string makes [`SplResolveError`] `Clone + PartialEq + Eq`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FetchError {
    message: String,
}

impl FetchError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FetchError {}

impl From<String> for FetchError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

impl From<&str> for FetchError {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for FetchError {
    fn from(error: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self::new(error.to_string())
    }
}

impl From<std::io::Error> for FetchError {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string())
    }
}

/// Which transfer of a multi-transfer instruction a hook slice belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegRole {
    Input,
    Output,
    Token0,
    Token1,
    Base,
    Quote,
    Other(u8),
}

impl fmt::Display for LegRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input => f.write_str("input"),
            Self::Output => f.write_str("output"),
            Self::Token0 => f.write_str("token_0"),
            Self::Token1 => f.write_str("token_1"),
            Self::Base => f.write_str("base"),
            Self::Quote => f.write_str("quote"),
            Self::Other(index) => write!(f, "leg#{index}"),
        }
    }
}

/// Why a program id was rejected as a hook program before any fetch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum HookProgramInvalidReason {
    /// The all-zero key (which is also the System program id).
    Zero,
    /// The Token-2022 program itself.
    Token2022Program,
    /// The legacy SPL Token program.
    SplTokenProgram,
    /// A Raydium program id: the exchange must never be its own hook.
    RaydiumProgram,
    /// The program account's data does not parse for its loader.
    MalformedProgramAccount,
}

impl fmt::Display for HookProgramInvalidReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Zero => "the hook program id is the zero key",
            Self::Token2022Program => "the hook program id is the Token-2022 program",
            Self::SplTokenProgram => "the hook program id is the SPL Token program",
            Self::RaydiumProgram => "the hook program id is a Raydium program",
            Self::MalformedProgramAccount => "the hook program account data is malformed",
        })
    }
}

/// What changed between two inspections of the same mint's hook.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum HookChangeKind {
    /// The mint now points at a different hook program.
    MintRepointed(Pubkey),
    /// The mint no longer carries a hook.
    HookRemoved,
    /// The program was upgraded, redeployed, retracted or its authority changed.
    ProgramStateChanged,
}

/// Which expectation about the Transfer Hook extension authority failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorityExpectation {
    /// No expectation.
    Any,
    /// The extension authority must be exactly this key.
    Exactly(Pubkey),
    /// The extension authority must be revoked (`None`).
    Revoked,
}

/// Errors resolving the hook accounts of one transfer.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SplResolveError {
    AccountFetch {
        address: Pubkey,
        source: FetchError,
    },
    InvalidMintOwner(Pubkey),
    AccountKeyMismatch {
        requested: Pubkey,
        returned: Pubkey,
    },
    MissingMint,
    InvalidMintData,
    MissingHookProgram,
    HookProgramNotExecutable,
    HookProgramInvalid {
        program: Pubkey,
        reason: HookProgramInvalidReason,
    },
    HookProgramBadLoader {
        program: Pubkey,
        loader: Pubkey,
    },
    HookProgramClosed {
        program: Pubkey,
    },
    /// The mint's hook program is not the one the caller expected.
    UnexpectedHookProgram {
        expected: Pubkey,
        found: Pubkey,
    },
    /// The caller requires a hook but the mint has none.
    HookRequired,
    /// The Transfer Hook extension authority is not what the caller required.
    HookAuthorityViolation {
        expected: AuthorityExpectation,
        found: Option<Pubkey>,
    },
    MissingValidationList(Pubkey),
    InvalidValidationListOwner {
        address: Pubkey,
        owner: Pubkey,
        expected: Pubkey,
    },
    /// The validation account is owned by the hook but is not a parseable
    /// `ExtraAccountMetaList` for the Execute interface.
    ValidationListMalformed {
        address: Pubkey,
        reason: String,
    },
    /// Resolving the extras from the list failed (seed, PDA, or data error).
    ExtraAccountResolution {
        code: Option<ProgramError>,
        reason: String,
    },
    /// A resolved extra account is a signer the policy did not allow.
    UnexpectedSigner {
        address: Pubkey,
    },
    /// A resolved extra account is writable and the policy did not allow it.
    UnexpectedWritable {
        address: Pubkey,
    },
    /// The validation list contents changed between two reads.
    ValidationListChanged {
        address: Pubkey,
        before: [u8; 32],
        after: [u8; 32],
    },
    /// The hook program (or the mint's pointer to it) changed between two reads.
    HookProgramChanged {
        program: Pubkey,
        kind: HookChangeKind,
    },
}

impl fmt::Display for SplResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccountFetch { address, source } => {
                write!(f, "account fetch for {address} failed: {source}")
            }
            Self::InvalidMintOwner(owner) => {
                write!(f, "mint owner {owner} is not SPL Token or Token-2022")
            }
            Self::AccountKeyMismatch {
                requested,
                returned,
            } => write!(
                f,
                "fetcher returned account {returned} for request {requested}"
            ),
            Self::MissingMint => f.write_str("mint account was not found"),
            Self::InvalidMintData => f.write_str("mint account data is invalid"),
            Self::MissingHookProgram => f.write_str("hook program account was not found"),
            Self::HookProgramNotExecutable => {
                f.write_str("mint transfer-hook program account is not executable")
            }
            Self::HookProgramInvalid { program, reason } => {
                write!(f, "hook program {program} rejected: {reason}")
            }
            Self::HookProgramBadLoader { program, loader } => write!(
                f,
                "hook program {program} is owned by disallowed loader {loader}"
            ),
            Self::HookProgramClosed { program } => {
                write!(f, "hook program {program} is closed or retracted")
            }
            Self::UnexpectedHookProgram { expected, found } => write!(
                f,
                "mint hook program {found} is not the expected program {expected}"
            ),
            Self::HookRequired => f.write_str("a transfer hook is required but the mint has none"),
            Self::HookAuthorityViolation { expected, found } => write!(
                f,
                "transfer-hook authority {found:?} violates expectation {expected:?}"
            ),
            Self::MissingValidationList(address) => {
                write!(f, "validation ExtraAccountMetaList {address} was not found")
            }
            Self::InvalidValidationListOwner {
                address,
                owner,
                expected,
            } => write!(
                f,
                "validation list {address} is owned by {owner}, expected hook program {expected}"
            ),
            Self::ValidationListMalformed { address, reason } => {
                write!(f, "validation list {address} is malformed: {reason}")
            }
            Self::ExtraAccountResolution { reason, .. } => {
                write!(f, "extra account resolution failed: {reason}")
            }
            Self::UnexpectedSigner { address } => {
                write!(
                    f,
                    "resolved extra account {address} is an unexpected signer"
                )
            }
            Self::UnexpectedWritable { address } => write!(
                f,
                "resolved extra account {address} is unexpectedly writable"
            ),
            Self::ValidationListChanged { address, .. } => {
                write!(f, "validation list {address} changed since it was read")
            }
            Self::HookProgramChanged { program, kind } => {
                write!(
                    f,
                    "hook program {program} changed since it was read: {kind:?}"
                )
            }
        }
    }
}

impl std::error::Error for SplResolveError {}

/// A resolution failure attributed to one leg of a multi-transfer instruction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegError {
    pub leg: LegRole,
    pub mint: Pubkey,
    pub source: SplResolveError,
}

impl fmt::Display for LegError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} leg (mint {}) failed to resolve: {}",
            self.leg, self.mint, self.source
        )
    }
}

impl std::error::Error for LegError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Which field of a transfer leg disagreed with the swap instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegField {
    Mint,
    Source,
    Destination,
    Authority,
}

/// Why a hook slice is not an authentic `(extras.., hook_program, validation_list)` tail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SliceFault {
    /// Fewer than two accounts.
    TooShort,
    /// The second-to-last account is not the hook program.
    TailNotHookProgram,
    /// The last account is not the validation list.
    TailNotValidationList,
    /// The validation list is not the canonical PDA for `(mint, hook_program)`.
    NonCanonicalValidationList { expected: Pubkey, found: Pubkey },
    /// The hook program or validation list is a signer or writable.
    TailPrivileged,
}

/// Where a conflicting account sits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictSite {
    /// A fixed (or tick/bitmap) account already in the instruction, by index.
    Fixed(usize),
    /// The other leg's hook slice.
    Leg(LegRole),
}

/// Errors from the framing API.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FrameError {
    /// The instruction data is not the expected V1 / SwapV2 layout.
    InvalidInstructionData,
    /// The instruction is already a framed V2 / V3 instruction.
    AlreadyFramed,
    InvalidFixedAccountCount {
        expected: usize,
        found: usize,
    },
    InvalidRemainingAccountSections,
    AccountCountOverflow,
    /// The leg's transfer does not match the swap instruction's accounts.
    LegMismatch {
        leg: LegRole,
        field: LegField,
        expected: Pubkey,
        found: Pubkey,
    },
    InvalidSlice {
        leg: LegRole,
        reason: SliceFault,
    },
    /// A slice account would escalate privileges of an account that is shared
    /// with the fixed accounts or the other slice. Solana unions flags per
    /// key across the whole transaction, so this would escalate the shared account.
    CrossSlicePrivilegeConflict {
        leg: LegRole,
        address: Pubkey,
        other: ConflictSite,
    },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInstructionData => f.write_str("unexpected Raydium instruction data"),
            Self::AlreadyFramed => f.write_str("instruction is already hook-framed"),
            Self::InvalidFixedAccountCount { expected, found } => write!(
                f,
                "Raydium instruction has {found} accounts, expected {expected}"
            ),
            Self::InvalidRemainingAccountSections => {
                f.write_str("CLMM tick and bitmap sections do not match remaining accounts")
            }
            Self::AccountCountOverflow => {
                f.write_str("Raydium remaining-account count exceeds the u16 framing limit")
            }
            Self::LegMismatch {
                leg,
                field,
                expected,
                found,
            } => write!(
                f,
                "{leg} leg {field:?} is {found} but the swap instruction uses {expected}"
            ),
            Self::InvalidSlice { leg, reason } => {
                write!(f, "{leg} leg hook slice is not authentic: {reason:?}")
            }
            Self::CrossSlicePrivilegeConflict {
                leg,
                address,
                other,
            } => write!(
                f,
                "{leg} leg slice account {address} escalates privileges shared with {other:?}"
            ),
        }
    }
}

impl std::error::Error for FrameError {}
