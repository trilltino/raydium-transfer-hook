//! Why a simulated launch operation was refused.

use std::fmt;

use hook_policy_model::PolicyError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchSimError {
    Policy(PolicyError),
    InvalidMint,
    InvalidPhase,
    HookConfigurationMismatch,
    MissingValidationList,
    HookNotInitialized,
    TransferMintMismatch,
    MigrationMintMismatch,
}

impl fmt::Display for LaunchSimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(error) => write!(f, "launch policy rejected: {error}"),
            Self::InvalidMint => f.write_str("launch mint must be configured"),
            Self::InvalidPhase => f.write_str("operation is not valid in the current launch phase"),
            Self::HookConfigurationMismatch => {
                f.write_str("engine mint or program does not match the selected launch hook")
            }
            Self::MissingValidationList => {
                f.write_str("a selected transfer hook requires an initialized validation list")
            }
            Self::HookNotInitialized => {
                f.write_str("hook setup must finish before the launch can trade")
            }
            Self::TransferMintMismatch => {
                f.write_str("trade transfer does not use the launch mint")
            }
            Self::MigrationMintMismatch => {
                f.write_str("graduation cannot replace the launched mint")
            }
        }
    }
}

impl std::error::Error for LaunchSimError {}

impl From<PolicyError> for LaunchSimError {
    fn from(error: PolicyError) -> Self {
        Self::Policy(error)
    }
}
