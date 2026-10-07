//! MODEL ONLY: a policy simulator, not a LaunchLab integration and not a CPI.
//!
//! LaunchLab's handlers are not public, so there is no real LaunchLab ABI to
//! frame against and this crate offers none. What remains is
//! [`LaunchPolicySimulator`]: a small state machine that applies the platform
//! policy model to a launch's lifecycle, and can hand the SDK the
//! [`ResolveOptions`] a trade on that launch would have to satisfy. It is gated
//! behind the `model` feature so it is never mistaken for an API.

#![forbid(unsafe_code)]

#[cfg(feature = "model")]
pub use model::*;

#[cfg(feature = "model")]
mod model {
    use std::fmt;

    use hook_policy_model::{LaunchConfig, PlatformConfig, PolicyDecision, PolicyError, Pubkey};
    use reference_hook_program::HookEngine;
    use transfer_hook_sdk::ResolveOptions;

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
                Self::InvalidPhase => {
                    f.write_str("operation is not valid in the current launch phase")
                }
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

    /// Simulates a launch's lifecycle under a platform policy. Model only.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct LaunchPolicySimulator {
        mint: Pubkey,
        decision: PolicyDecision,
        validation_list_initialized: bool,
        phase: LaunchPhase,
    }

    impl LaunchPolicySimulator {
        pub fn create(
            mint: Pubkey,
            platform: PlatformConfig,
            launch: LaunchConfig,
        ) -> Result<Self, LaunchSimError> {
            if mint == [0; 32] {
                return Err(LaunchSimError::InvalidMint);
            }
            let decision = platform.decide(launch)?;
            Ok(Self {
                mint,
                decision,
                validation_list_initialized: false,
                phase: LaunchPhase::MintCreated,
            })
        }

        pub fn phase(&self) -> LaunchPhase {
            self.phase
        }

        pub fn selected_hook_program(&self) -> Option<Pubkey> {
            self.decision.hook_program
        }

        /// The resolution options a trade on this launch must satisfy: the
        /// selected hook program is pinned, and a required hook is enforced.
        pub fn resolve_options(&self) -> ResolveOptions {
            ResolveOptions::from_policy_decision(&self.decision)
        }

        /// Check that a trade transfer uses the launch mint and the launch is live.
        pub fn check_trade_mint(&self, mint: Pubkey) -> Result<(), LaunchSimError> {
            if self.phase != LaunchPhase::Trading {
                return Err(LaunchSimError::InvalidPhase);
            }
            if mint != self.mint {
                return Err(LaunchSimError::TransferMintMismatch);
            }
            Ok(())
        }

        /// Record hook setup. `validation_list_initialized` is a caller claim
        /// in this model; the SDK checks the real list at resolution time.
        pub fn initialize_hook(
            &mut self,
            engine: &HookEngine,
            validation_list_initialized: bool,
        ) -> Result<(), LaunchSimError> {
            if self.phase != LaunchPhase::MintCreated || self.decision.hook_program.is_none() {
                return Err(LaunchSimError::InvalidPhase);
            }
            let config = engine.config();
            if config.mint != self.mint || Some(config.hook_program) != self.decision.hook_program {
                return Err(LaunchSimError::HookConfigurationMismatch);
            }
            if !validation_list_initialized {
                return Err(LaunchSimError::MissingValidationList);
            }
            self.validation_list_initialized = true;
            self.phase = LaunchPhase::HookInitialized;
            Ok(())
        }

        pub fn begin_trading(&mut self) -> Result<(), LaunchSimError> {
            match (self.phase, self.decision.hook_program) {
                (LaunchPhase::MintCreated, None) => self.phase = LaunchPhase::Trading,
                (LaunchPhase::HookInitialized, Some(_)) if self.validation_list_initialized => {
                    self.phase = LaunchPhase::Trading
                }
                (LaunchPhase::MintCreated, Some(_)) => {
                    return Err(LaunchSimError::HookNotInitialized)
                }
                _ => return Err(LaunchSimError::InvalidPhase),
            }
            Ok(())
        }

        pub fn graduate(
            &mut self,
            migrated_mint: Pubkey,
        ) -> Result<GraduationRecord, LaunchSimError> {
            if self.phase != LaunchPhase::Trading {
                return Err(LaunchSimError::InvalidPhase);
            }
            if migrated_mint != self.mint {
                return Err(LaunchSimError::MigrationMintMismatch);
            }
            self.phase = LaunchPhase::Graduated;
            Ok(GraduationRecord {
                mint: self.mint,
                hook_program: self.decision.hook_program,
                validation_list_initialized: self.validation_list_initialized,
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use hook_policy_model::{HookAuthorityPolicy, HookPolicy};
        use transfer_hook_sdk::AuthorityExpectation;

        #[test]
        fn mandatory_hook_must_be_initialized_before_trading() {
            let mut launch = LaunchPolicySimulator::create(
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
                Err(LaunchSimError::HookNotInitialized)
            );
        }

        #[test]
        fn optional_no_hook_launch_can_trade_and_graduate_without_hook_setup() {
            let mut launch = LaunchPolicySimulator::create(
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

        #[test]
        fn resolve_options_pin_the_selected_hook_and_authority_policy() {
            let launch = LaunchPolicySimulator::create(
                [1; 32],
                PlatformConfig::new(
                    Some([2; 32]),
                    HookPolicy::Mandatory,
                    HookAuthorityPolicy::ImmutableAtLaunch,
                ),
                LaunchConfig::new(),
            )
            .unwrap();
            let options = launch.resolve_options();
            assert_eq!(
                options.expected_hook_program,
                Some(transfer_hook_sdk::solana_program::pubkey::Pubkey::new_from_array([2; 32]))
            );
            assert!(options.require_hook);
            assert_eq!(
                options.expected_hook_authority,
                AuthorityExpectation::Revoked
            );
        }

        #[test]
        fn trades_require_the_launch_mint_and_the_trading_phase() {
            let mut launch = LaunchPolicySimulator::create(
                [1; 32],
                PlatformConfig::without_program(
                    HookPolicy::Disabled,
                    HookAuthorityPolicy::PlatformRetained,
                ),
                LaunchConfig::new(),
            )
            .unwrap();
            assert_eq!(
                launch.check_trade_mint([1; 32]),
                Err(LaunchSimError::InvalidPhase)
            );
            launch.begin_trading().unwrap();
            assert_eq!(launch.check_trade_mint([1; 32]), Ok(()));
            assert_eq!(
                launch.check_trade_mint([9; 32]),
                Err(LaunchSimError::TransferMintMismatch)
            );
        }

        #[test]
        fn zero_platform_hook_is_rejected_at_creation() {
            assert_eq!(
                LaunchPolicySimulator::create(
                    [1; 32],
                    PlatformConfig::with_program(
                        [0; 32],
                        HookPolicy::Optional,
                        HookAuthorityPolicy::PlatformRetained,
                    ),
                    LaunchConfig::new(),
                ),
                Err(LaunchSimError::Policy(PolicyError::ZeroHookProgram))
            );
        }
    }
}
