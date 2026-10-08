//! Errors resolving the hook accounts of one transfer.

use std::fmt;

use solana_program::{program_error::ProgramError, pubkey::Pubkey};

use super::fetch::FetchError;

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
