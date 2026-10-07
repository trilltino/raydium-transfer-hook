//! Who may initialise and later change a mint's hook configuration.

use crate::error::HookError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AuthorityMode {
    ExtensionAuthority = 0,
    MintAuthority = 1,
    Explicit = 2,
    Immutable = 3,
}

impl AuthorityMode {
    /// Mode 4 (PlatformControlled) is reserved and, like every unknown value, unsupported.
    pub fn from_u8(value: u8) -> Result<Self, HookError> {
        match value {
            0 => Ok(AuthorityMode::ExtensionAuthority),
            1 => Ok(AuthorityMode::MintAuthority),
            2 => Ok(AuthorityMode::Explicit),
            3 => Ok(AuthorityMode::Immutable),
            _ => Err(HookError::UnsupportedMode),
        }
    }
}
