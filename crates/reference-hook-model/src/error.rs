//! Why the model engine refused a transfer or a configuration change.

use std::fmt;

use crate::config::ConfigError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookError {
    InvalidConfig(ConfigError),
    WrongMint,
    MissingTransferAccount,
    TransferLimitExceeded,
    AddressNotAllowed,
    AddressDenied,
    UnauthorizedReconfiguration,
    ImmutableConfiguration,
    ImmutableFieldsChanged,
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(error) => write!(f, "invalid hook configuration: {error}"),
            Self::WrongMint => f.write_str("transfer mint does not match hook configuration"),
            Self::MissingTransferAccount => {
                f.write_str("transfer source, destination, and authority are required")
            }
            Self::TransferLimitExceeded => f.write_str("transfer exceeds the configured maximum"),
            Self::AddressNotAllowed => {
                f.write_str("source and destination must both be allow-listed")
            }
            Self::AddressDenied => f.write_str("source or destination is deny-listed"),
            Self::UnauthorizedReconfiguration => {
                f.write_str("configuration update is not authorized")
            }
            Self::ImmutableConfiguration => f.write_str("configuration is immutable after launch"),
            Self::ImmutableFieldsChanged => {
                f.write_str("mint, hook program, and authority policy cannot change")
            }
        }
    }
}

impl std::error::Error for HookError {}
