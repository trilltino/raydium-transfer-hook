//! A hook's identity at a point in time, so a change between resolve and sign is detected.

use std::future::Future;

use solana_program::pubkey::Pubkey;

use super::{accounts::SplAccount, inspect::inspect_mint, options::ResolveOptions};
use crate::error::{AuthorityExpectation, FetchError, HookChangeKind, SplResolveError};

/// Loader-specific state of the hook program, recorded for change detection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramFingerprint {
    /// BPF loader v1/v2: the code cannot change.
    Immutable,
    Upgradeable {
        program_data: Pubkey,
        slot: u64,
        upgrade_authority: Option<Pubkey>,
    },
    LoaderV4 {
        slot: u64,
        authority_or_next_version: Pubkey,
        status: u64,
    },
}

/// Everything about a hooked leg that must still be true when the transaction
/// is signed. Capture it with the leg ([`HookSlice::fingerprint`]) and check it
/// with [`HookFingerprint::verify_unchanged`] right before signing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HookFingerprint {
    pub mint: Pubkey,
    pub hook_program: Pubkey,
    /// The Transfer Hook extension authority at resolution time.
    pub hook_authority: Option<Pubkey>,
    pub loader: Pubkey,
    pub program: ProgramFingerprint,
    pub validation_list: Pubkey,
    /// sha256 of the validation list account data.
    pub validation_list_hash: [u8; 32],
}

impl HookFingerprint {
    /// Re-read the chain and fail if the hook (or its list) is not exactly what
    /// was resolved. Returns a specific error for what changed.
    pub async fn verify_unchanged<F, Fut, E>(&self, fetch: F) -> Result<(), SplResolveError>
    where
        F: Fn(Pubkey) -> Fut,
        Fut: Future<Output = Result<Option<SplAccount>, E>>,
        E: Into<FetchError>,
    {
        let options = ResolveOptions {
            allowed_loaders: vec![self.loader],
            ..ResolveOptions::default()
        };
        let now = inspect_mint(&fetch, self.mint, &options).await?;
        let Some(now) = now else {
            return Err(SplResolveError::HookProgramChanged {
                program: self.hook_program,
                kind: HookChangeKind::HookRemoved,
            });
        };
        diff_fingerprint(self, &now.fingerprint)
    }
}

pub(super) fn diff_fingerprint(
    before: &HookFingerprint,
    after: &HookFingerprint,
) -> Result<(), SplResolveError> {
    if before.hook_program != after.hook_program {
        return Err(SplResolveError::HookProgramChanged {
            program: before.hook_program,
            kind: HookChangeKind::MintRepointed(after.hook_program),
        });
    }
    if before.loader != after.loader || before.program != after.program {
        return Err(SplResolveError::HookProgramChanged {
            program: before.hook_program,
            kind: HookChangeKind::ProgramStateChanged,
        });
    }
    if before.hook_authority != after.hook_authority {
        return Err(SplResolveError::HookAuthorityViolation {
            expected: match before.hook_authority {
                Some(key) => AuthorityExpectation::Exactly(key),
                None => AuthorityExpectation::Revoked,
            },
            found: after.hook_authority,
        });
    }
    if before.validation_list_hash != after.validation_list_hash {
        return Err(SplResolveError::ValidationListChanged {
            address: before.validation_list,
            before: before.validation_list_hash,
            after: after.validation_list_hash,
        });
    }
    Ok(())
}
