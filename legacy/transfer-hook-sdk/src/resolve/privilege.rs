//! Which privilege escalations resolved extra accounts may carry.

use solana_program::{instruction::AccountMeta, pubkey::Pubkey};

use crate::error::SplResolveError;

/// Which privilege escalations resolved extra accounts may carry.
///
/// The hook program and the validation list are always readonly non-signers.
/// Every other resolved account is rejected if it is a signer or writable,
/// unless explicitly allowed. A hook-controlled list must not be able to make a
/// wallet or pool account signer or writable.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PrivilegePolicy {
    /// Extra accounts that may be marked signer.
    pub allowed_signers: Vec<Pubkey>,
    /// Extra accounts that may be marked writable.
    pub allowed_writable: Vec<Pubkey>,
    /// Disable the check entirely. Only for hooks you fully trust.
    pub trust_hook_privileges: bool,
}

impl PrivilegePolicy {
    /// Reject every signer and writable extra account (the default).
    pub fn reject_all() -> Self {
        Self::default()
    }

    /// Allow the listed accounts to be writable.
    pub fn allowing_writable(keys: impl IntoIterator<Item = Pubkey>) -> Self {
        Self {
            allowed_writable: keys.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Accept whatever privileges the hook's list declares.
    pub fn trust_everything() -> Self {
        Self {
            trust_hook_privileges: true,
            ..Self::default()
        }
    }

    pub(super) fn check(&self, meta: &AccountMeta) -> Result<(), SplResolveError> {
        if self.trust_hook_privileges {
            return Ok(());
        }
        if meta.is_signer && !self.allowed_signers.contains(&meta.pubkey) {
            return Err(SplResolveError::UnexpectedSigner {
                address: meta.pubkey,
            });
        }
        if meta.is_writable && !self.allowed_writable.contains(&meta.pubkey) {
            return Err(SplResolveError::UnexpectedWritable {
                address: meta.pubkey,
            });
        }
        Ok(())
    }
}
