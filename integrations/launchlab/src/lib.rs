#![forbid(unsafe_code)]

use std::fmt;

use hook_policy_model::{
    HookPolicy, LaunchConfig, PlatformConfig, PolicyError, Pubkey, TransferContext,
};
use reference_hook_program::HookEngine;
use transfer_hook_sdk::{
    ResolveError, ResolvedAccountBatch, TransferHookAccountSource, TransferHookResolver,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchPhase {
    MintCreated,
    HookInitialized,
    Trading,
    Graduated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraduationRecord {
    pub mint: Pubkey,
    pub hook_program: Option<Pubkey>,
    pub validation_list_initialized: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchLabError {
    Policy(PolicyError),
    Resolution(ResolveError),
    InvalidMint,
    InvalidPhase,
    HookConfigurationMismatch,
    MissingValidationList,
    HookNotInitialized,
    TransferMintMismatch,
    MigrationMintMismatch,
}

impl fmt::Display for LaunchLabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(error) => write!(f, "launch policy rejected: {error}"),
            Self::Resolution(error) => write!(f, "trade account resolution failed: {error}"),
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
                f.write_str("launch trade transfer does not use the launch mint")
            }
            Self::MigrationMintMismatch => {
                f.write_str("graduation cannot replace the launched mint")
            }
        }
    }
}

impl std::error::Error for LaunchLabError {}

impl From<PolicyError> for LaunchLabError {
    fn from(error: PolicyError) -> Self {
        Self::Policy(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchLabLifecycle {
    mint: Pubkey,
    selected_hook_program: Option<Pubkey>,
    validation_list_initialized: bool,
    phase: LaunchPhase,
}

impl LaunchLabLifecycle {
    pub fn create(
        mint: Pubkey,
        platform: PlatformConfig,
        launch: LaunchConfig,
    ) -> Result<Self, LaunchLabError> {
        if mint == [0; 32] {
            return Err(LaunchLabError::InvalidMint);
        }
        let selected_hook_program = platform.validate_launch(launch)?;
        if platform.policy == HookPolicy::Mandatory && selected_hook_program.is_none() {
            return Err(PolicyError::MissingPlatformHook.into());
        }
        Ok(Self {
            mint,
            selected_hook_program,
            validation_list_initialized: false,
            phase: LaunchPhase::MintCreated,
        })
    }

    pub fn phase(&self) -> LaunchPhase {
        self.phase
    }

    pub fn selected_hook_program(&self) -> Option<Pubkey> {
        self.selected_hook_program
    }

    pub fn initialize_hook(
        &mut self,
        engine: &HookEngine,
        validation_list_initialized: bool,
    ) -> Result<(), LaunchLabError> {
        if self.phase != LaunchPhase::MintCreated || self.selected_hook_program.is_none() {
            return Err(LaunchLabError::InvalidPhase);
        }
        let config = engine.config();
        if config.mint != self.mint || Some(config.hook_program) != self.selected_hook_program {
            return Err(LaunchLabError::HookConfigurationMismatch);
        }
        if !validation_list_initialized {
            return Err(LaunchLabError::MissingValidationList);
        }
        self.validation_list_initialized = true;
        self.phase = LaunchPhase::HookInitialized;
        Ok(())
    }

    pub fn begin_trading(&mut self) -> Result<(), LaunchLabError> {
        match (self.phase, self.selected_hook_program) {
            (LaunchPhase::MintCreated, None) => self.phase = LaunchPhase::Trading,
            (LaunchPhase::HookInitialized, Some(_)) if self.validation_list_initialized => {
                self.phase = LaunchPhase::Trading
            }
            (LaunchPhase::MintCreated, Some(_)) => return Err(LaunchLabError::HookNotInitialized),
            _ => return Err(LaunchLabError::InvalidPhase),
        }
        Ok(())
    }

    pub fn resolve_trade<S: TransferHookAccountSource>(
        &self,
        resolver: &TransferHookResolver,
        source: &mut S,
        transfer: TransferContext,
    ) -> Result<ResolvedAccountBatch, LaunchLabError> {
        if self.phase != LaunchPhase::Trading {
            return Err(LaunchLabError::InvalidPhase);
        }
        if transfer.mint != self.mint {
            return Err(LaunchLabError::TransferMintMismatch);
        }
        let resolved = resolver
            .resolve_batch(source, &[transfer])
            .map_err(LaunchLabError::from)?;
        if resolved.transfers[0].hook_program != self.selected_hook_program {
            return Err(LaunchLabError::HookConfigurationMismatch);
        }
        Ok(resolved)
    }

    pub fn graduate(&mut self, migrated_mint: Pubkey) -> Result<GraduationRecord, LaunchLabError> {
        if self.phase != LaunchPhase::Trading {
            return Err(LaunchLabError::InvalidPhase);
        }
        if migrated_mint != self.mint {
            return Err(LaunchLabError::MigrationMintMismatch);
        }
        self.phase = LaunchPhase::Graduated;
        Ok(GraduationRecord {
            mint: self.mint,
            hook_program: self.selected_hook_program,
            validation_list_initialized: self.validation_list_initialized,
        })
    }
}

impl From<ResolveError> for LaunchLabError {
    fn from(error: ResolveError) -> Self {
        Self::Resolution(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hook_policy_model::HookAuthorityPolicy;

    #[test]
    fn mandatory_hook_must_be_initialized_before_trading() {
        let mut launch = LaunchLabLifecycle::create(
            [1; 32],
            PlatformConfig::new(
                Some([2; 32]),
                HookPolicy::Mandatory,
                HookAuthorityPolicy::PlatformRetained,
            ),
            LaunchConfig::new(),
        )
        .unwrap();
        assert_eq!(
            launch.begin_trading(),
            Err(LaunchLabError::HookNotInitialized)
        );
    }

    #[test]
    fn optional_no_hook_launch_can_trade_and_graduate_without_hook_setup() {
        let mut launch = LaunchLabLifecycle::create(
            [1; 32],
            PlatformConfig::new(
                Some([2; 32]),
                HookPolicy::Optional,
                HookAuthorityPolicy::PlatformRetained,
            ),
            LaunchConfig::new(),
        )
        .unwrap();
        launch.begin_trading().unwrap();
        let record = launch.graduate([1; 32]).unwrap();
        assert_eq!(record.hook_program, None);
        assert!(!record.validation_list_initialized);
        assert_eq!(launch.phase(), LaunchPhase::Graduated);
    }
}
