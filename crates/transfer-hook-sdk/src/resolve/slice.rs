//! A resolved hook slice and a resolved transfer leg.

use std::future::Future;

use solana_program::{instruction::AccountMeta, pubkey::Pubkey};

use super::{
    accounts::{SplAccount, SplTransferLeg},
    fingerprint::HookFingerprint,
    inspect::inspect_mint,
    options::ResolveOptions,
};
use crate::error::{FetchError, HookChangeKind, LegError, LegRole, SplResolveError};

/// The resolved per-transfer hook tail: `extras.., hook_program, validation_list`.
///
/// There is no public constructor: the only way to obtain one is
/// [`resolve_leg`], so a framed instruction can only carry slices that were
/// derived from a real validation list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HookSlice {
    metas: Vec<AccountMeta>,
    hook_program: Pubkey,
    validation_list: Pubkey,
    fingerprint: HookFingerprint,
}

impl HookSlice {
    pub(crate) fn new(
        metas: Vec<AccountMeta>,
        hook_program: Pubkey,
        validation_list: Pubkey,
        fingerprint: HookFingerprint,
    ) -> Self {
        Self {
            metas,
            hook_program,
            validation_list,
            fingerprint,
        }
    }

    /// The full slice in instruction order (N extras followed by the 2-account tail).
    pub fn metas(&self) -> &[AccountMeta] {
        &self.metas
    }

    /// The accounts resolved from the validation list, without the tail.
    pub fn extras(&self) -> &[AccountMeta] {
        &self.metas[..self.metas.len().saturating_sub(2)]
    }

    pub fn hook_program(&self) -> Pubkey {
        self.hook_program
    }

    pub fn validation_list(&self) -> Pubkey {
        self.validation_list
    }

    pub fn fingerprint(&self) -> &HookFingerprint {
        &self.fingerprint
    }

    pub fn len(&self) -> usize {
        self.metas.len()
    }

    pub fn is_empty(&self) -> bool {
        self.metas.is_empty()
    }
}

/// The resolution result for one transfer: either a hook slice or "no hook".
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegHook {
    role: LegRole,
    transfer: SplTransferLeg,
    slice: Option<HookSlice>,
}

impl LegHook {
    pub(crate) fn new(role: LegRole, transfer: SplTransferLeg, slice: Option<HookSlice>) -> Self {
        Self {
            role,
            transfer,
            slice,
        }
    }

    pub fn role(&self) -> LegRole {
        self.role
    }

    pub fn transfer(&self) -> &SplTransferLeg {
        &self.transfer
    }

    pub fn slice(&self) -> Option<&HookSlice> {
        self.slice.as_ref()
    }

    pub fn is_hooked(&self) -> bool {
        self.slice.is_some()
    }

    /// Number of accounts this leg adds to the instruction (0 when unhooked).
    pub fn account_count(&self) -> usize {
        self.slice.as_ref().map_or(0, HookSlice::len)
    }

    pub fn fingerprint(&self) -> Option<&HookFingerprint> {
        self.slice.as_ref().map(HookSlice::fingerprint)
    }

    /// Check that this leg's hook is still exactly what was resolved.
    /// Unhooked legs only need the mint to still be unhooked.
    pub async fn verify_unchanged<F, Fut, E>(&self, fetch: F) -> Result<(), LegError>
    where
        F: Fn(Pubkey) -> Fut,
        Fut: Future<Output = Result<Option<SplAccount>, E>>,
        E: Into<FetchError>,
    {
        let attribute = |source| LegError {
            leg: self.role,
            mint: self.transfer.mint,
            source,
        };
        match &self.slice {
            Some(slice) => slice
                .fingerprint
                .verify_unchanged(fetch)
                .await
                .map_err(attribute),
            None => {
                let options = ResolveOptions::default();
                match inspect_mint(&fetch, self.transfer.mint, &options)
                    .await
                    .map_err(attribute)?
                {
                    None => Ok(()),
                    Some(now) => Err(attribute(SplResolveError::HookProgramChanged {
                        program: now.hook_program,
                        kind: HookChangeKind::MintRepointed(now.hook_program),
                    })),
                }
            }
        }
    }
}
