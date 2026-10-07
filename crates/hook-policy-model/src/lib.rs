//! MODEL ONLY: platform policy rules, not an on-chain program or a Raydium CPI.
//!
//! Keys here are plain `[u8; 32]` so this crate stays free of Solana types. The
//! SDK converts a [`PolicyDecision`] into its resolution options.

#![forbid(unsafe_code)]

use std::fmt;

pub type Pubkey = [u8; 32];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookPolicy {
    Disabled,
    Optional,
    Mandatory,
}

impl HookPolicy {
    pub fn allows_hook(self) -> bool {
        matches!(self, HookPolicy::Optional | HookPolicy::Mandatory)
    }

    pub fn requires_hook(self) -> bool {
        matches!(self, HookPolicy::Mandatory)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookAuthorityPolicy {
    PlatformRetained,
    ImmutableAtLaunch,
    GovernedTimelock,
}

impl HookAuthorityPolicy {
    pub fn is_platform_retained(self) -> bool {
        matches!(self, Self::PlatformRetained)
    }

    pub fn is_immutable_at_launch(self) -> bool {
        matches!(self, Self::ImmutableAtLaunch)
    }

    pub fn is_governed_timelock(self) -> bool {
        matches!(self, Self::GovernedTimelock)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookPreset {
    FairLaunch,
    LoyaltyRewards,
}

impl HookPreset {
    pub fn name(self) -> &'static str {
        match self {
            Self::FairLaunch => "fair_launch",
            Self::LoyaltyRewards => "loyalty_rewards",
        }
    }
}

/// Who is attempting a configuration change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigActor {
    Platform,
    Timelock,
    Other,
}

/// The outcome of applying platform policy to a launch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyDecision {
    /// The hook program the launch must use, if any.
    pub hook_program: Option<Pubkey>,
    /// Whether trading must fail when the mint does not carry that hook.
    pub hook_required: bool,
    pub authority_policy: HookAuthorityPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformConfig {
    pub hook_program: Option<Pubkey>,
    pub policy: HookPolicy,
    pub authority_policy: HookAuthorityPolicy,
}

impl PlatformConfig {
    pub fn new(
        hook_program: Option<Pubkey>,
        policy: HookPolicy,
        authority_policy: HookAuthorityPolicy,
    ) -> Self {
        Self {
            hook_program,
            policy,
            authority_policy,
        }
    }

    pub fn with_program(
        program: Pubkey,
        policy: HookPolicy,
        authority_policy: HookAuthorityPolicy,
    ) -> Self {
        Self::new(Some(program), policy, authority_policy)
    }

    pub fn without_program(policy: HookPolicy, authority_policy: HookAuthorityPolicy) -> Self {
        Self::new(None, policy, authority_policy)
    }

    /// Check the platform configuration itself, independent of any launch.
    pub fn validate(self) -> Result<(), PolicyError> {
        if self.hook_program == Some([0; 32]) {
            return Err(PolicyError::ZeroHookProgram);
        }
        Ok(())
    }

    /// Decide what a launch gets: which hook program (if any), whether the
    /// hook is required, and under which authority policy it is held.
    pub fn decide(self, launch: LaunchConfig) -> Result<PolicyDecision, PolicyError> {
        self.validate()?;
        let (hook_program, hook_required) = match (self.policy, self.hook_program, launch.preset) {
            (HookPolicy::Disabled, _, Some(_)) => return Err(PolicyError::DisabledHookSelected),
            (HookPolicy::Disabled, _, None) => (None, false),
            (HookPolicy::Optional, None, Some(_)) => return Err(PolicyError::PresetWithoutHook),
            (HookPolicy::Optional, Some(program), Some(_)) => (Some(program), true),
            (HookPolicy::Optional, _, None) => (None, false),
            (HookPolicy::Mandatory, None, _) => return Err(PolicyError::MissingPlatformHook),
            (HookPolicy::Mandatory, Some(program), _) => (Some(program), true),
        };
        Ok(PolicyDecision {
            hook_program,
            hook_required,
            authority_policy: self.authority_policy,
        })
    }

    /// The hook program a launch must use, or `None` if it gets no hook.
    pub fn validate_launch(self, launch: LaunchConfig) -> Result<Option<Pubkey>, PolicyError> {
        self.decide(launch).map(|decision| decision.hook_program)
    }

    /// Enforce the authority policy for a configuration change.
    ///
    /// `launched` is whether the launch has already gone live. Immutable
    /// policies reject every change once launched; retained policies accept
    /// only the platform; governed policies accept only the timelock.
    pub fn enforce_authority(self, actor: ConfigActor, launched: bool) -> Result<(), PolicyError> {
        match (self.authority_policy, actor) {
            (HookAuthorityPolicy::ImmutableAtLaunch, _) if launched => {
                Err(PolicyError::ConfigurationImmutable)
            }
            (HookAuthorityPolicy::ImmutableAtLaunch, ConfigActor::Platform) => Ok(()),
            (HookAuthorityPolicy::PlatformRetained, ConfigActor::Platform) => Ok(()),
            (HookAuthorityPolicy::GovernedTimelock, ConfigActor::Timelock) => Ok(()),
            _ => Err(PolicyError::UnauthorizedActor),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaunchConfig {
    pub preset: Option<HookPreset>,
}

impl LaunchConfig {
    pub const fn new() -> Self {
        Self { preset: None }
    }

    pub const fn with_preset(preset: HookPreset) -> Self {
        Self {
            preset: Some(preset),
        }
    }
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyError {
    MissingPlatformHook,
    DisabledHookSelected,
    PresetWithoutHook,
    ZeroHookProgram,
    ConfigurationImmutable,
    UnauthorizedActor,
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::MissingPlatformHook => "a mandatory platform hook requires a configured program",
            Self::DisabledHookSelected => {
                "the launch selected a hook preset while the policy is disabled"
            }
            Self::PresetWithoutHook => "a hook preset requires a configured platform hook program",
            Self::ZeroHookProgram => "the platform hook program must not be the zero key",
            Self::ConfigurationImmutable => "the hook configuration is immutable after launch",
            Self::UnauthorizedActor => "the actor may not change this hook configuration",
        };
        f.write_str(message)
    }
}

impl std::error::Error for PolicyError {}

/// The transfer a model hook engine evaluates. (The SDK uses its own
/// `SplTransferLeg` with Solana key types; this is for the pure-Rust model.)
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferContext {
    pub source: Pubkey,
    pub mint: Pubkey,
    pub destination: Pubkey,
    pub authority: Pubkey,
    pub amount: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> Pubkey {
        [byte; 32]
    }

    #[test]
    fn disabled_policy_keeps_non_hook_launches_unchanged() {
        let config = PlatformConfig {
            hook_program: None,
            policy: HookPolicy::Disabled,
            authority_policy: HookAuthorityPolicy::PlatformRetained,
        };

        assert_eq!(
            config.validate_launch(LaunchConfig { preset: None }),
            Ok(None)
        );
        assert_eq!(
            config.validate_launch(LaunchConfig {
                preset: Some(HookPreset::FairLaunch)
            }),
            Err(PolicyError::DisabledHookSelected)
        );
    }

    #[test]
    fn mandatory_policy_uses_the_platform_selected_program() {
        let program = key(7);
        let config = PlatformConfig {
            hook_program: Some(program),
            policy: HookPolicy::Mandatory,
            authority_policy: HookAuthorityPolicy::GovernedTimelock,
        };

        assert_eq!(
            config.validate_launch(LaunchConfig {
                preset: Some(HookPreset::FairLaunch)
            }),
            Ok(Some(program))
        );
    }

    #[test]
    fn mandatory_policy_requires_a_platform_program() {
        let config = PlatformConfig {
            hook_program: None,
            policy: HookPolicy::Mandatory,
            authority_policy: HookAuthorityPolicy::ImmutableAtLaunch,
        };

        assert_eq!(
            config.validate_launch(LaunchConfig { preset: None }),
            Err(PolicyError::MissingPlatformHook)
        );
    }

    #[test]
    fn optional_policy_rejects_presets_without_an_engine() {
        let config = PlatformConfig {
            hook_program: None,
            policy: HookPolicy::Optional,
            authority_policy: HookAuthorityPolicy::PlatformRetained,
        };

        assert_eq!(
            config.validate_launch(LaunchConfig {
                preset: Some(HookPreset::LoyaltyRewards)
            }),
            Err(PolicyError::PresetWithoutHook)
        );
    }

    #[test]
    fn optional_policy_does_not_enable_the_hook_without_a_preset() {
        let config = PlatformConfig {
            hook_program: Some(key(8)),
            policy: HookPolicy::Optional,
            authority_policy: HookAuthorityPolicy::PlatformRetained,
        };

        assert_eq!(
            config.validate_launch(LaunchConfig { preset: None }),
            Ok(None)
        );
    }

    #[test]
    fn zero_hook_program_is_rejected_under_every_policy() {
        for policy in [
            HookPolicy::Disabled,
            HookPolicy::Optional,
            HookPolicy::Mandatory,
        ] {
            let config = PlatformConfig::with_program(
                [0; 32],
                policy,
                HookAuthorityPolicy::PlatformRetained,
            );
            assert_eq!(config.validate(), Err(PolicyError::ZeroHookProgram));
            assert_eq!(
                config.validate_launch(LaunchConfig::new()),
                Err(PolicyError::ZeroHookProgram)
            );
        }
    }

    #[test]
    fn decision_marks_hook_required_only_when_the_launch_is_hooked() {
        let optional = PlatformConfig::with_program(
            key(5),
            HookPolicy::Optional,
            HookAuthorityPolicy::ImmutableAtLaunch,
        );
        assert_eq!(
            optional.decide(LaunchConfig::new()).unwrap(),
            PolicyDecision {
                hook_program: None,
                hook_required: false,
                authority_policy: HookAuthorityPolicy::ImmutableAtLaunch,
            }
        );
        assert_eq!(
            optional
                .decide(LaunchConfig::with_preset(HookPreset::FairLaunch))
                .unwrap(),
            PolicyDecision {
                hook_program: Some(key(5)),
                hook_required: true,
                authority_policy: HookAuthorityPolicy::ImmutableAtLaunch,
            }
        );
    }

    #[test]
    fn authority_policy_is_enforced_per_actor() {
        let platform = PlatformConfig::without_program(
            HookPolicy::Disabled,
            HookAuthorityPolicy::PlatformRetained,
        );
        assert_eq!(
            platform.enforce_authority(ConfigActor::Platform, true),
            Ok(())
        );
        assert_eq!(
            platform.enforce_authority(ConfigActor::Timelock, true),
            Err(PolicyError::UnauthorizedActor)
        );
        assert_eq!(
            platform.enforce_authority(ConfigActor::Other, false),
            Err(PolicyError::UnauthorizedActor)
        );

        let governed = PlatformConfig::without_program(
            HookPolicy::Disabled,
            HookAuthorityPolicy::GovernedTimelock,
        );
        assert_eq!(
            governed.enforce_authority(ConfigActor::Timelock, true),
            Ok(())
        );
        assert_eq!(
            governed.enforce_authority(ConfigActor::Platform, true),
            Err(PolicyError::UnauthorizedActor)
        );

        let immutable = PlatformConfig::without_program(
            HookPolicy::Disabled,
            HookAuthorityPolicy::ImmutableAtLaunch,
        );
        assert_eq!(
            immutable.enforce_authority(ConfigActor::Platform, false),
            Ok(())
        );
        assert_eq!(
            immutable.enforce_authority(ConfigActor::Platform, true),
            Err(PolicyError::ConfigurationImmutable)
        );
        assert_eq!(
            immutable.enforce_authority(ConfigActor::Timelock, true),
            Err(PolicyError::ConfigurationImmutable)
        );
        assert_eq!(
            immutable.enforce_authority(ConfigActor::Other, false),
            Err(PolicyError::UnauthorizedActor)
        );
    }
}
