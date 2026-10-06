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

    pub fn selected_program(self, launch: LaunchConfig) -> Result<Option<Pubkey>, PolicyError> {
        self.validate_launch(launch)
    }

    pub fn validate_launch(self, launch: LaunchConfig) -> Result<Option<Pubkey>, PolicyError> {
        match (self.policy, self.hook_program, launch.preset) {
            (HookPolicy::Disabled, _, Some(_)) => Err(PolicyError::DisabledHookSelected),
            (HookPolicy::Disabled, _, None) => Ok(None),
            (HookPolicy::Optional, None, Some(_)) => Err(PolicyError::PresetWithoutHook),
            (HookPolicy::Optional, Some(program), Some(_)) => Ok(Some(program)),
            (HookPolicy::Optional, Some(_), None) => Ok(None),
            (HookPolicy::Optional, None, None) => Ok(None),
            (HookPolicy::Mandatory, None, _) => Err(PolicyError::MissingPlatformHook),
            (HookPolicy::Mandatory, Some(program), _) => Ok(Some(program)),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyError {
    MissingPlatformHook,
    DisabledHookSelected,
    PresetWithoutHook,
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::MissingPlatformHook => "a mandatory platform hook requires a configured program",
            Self::DisabledHookSelected => {
                "the launch selected a hook preset while the policy is disabled"
            }
            Self::PresetWithoutHook => "a hook preset requires a configured platform hook program",
        };
        f.write_str(message)
    }
}

impl std::error::Error for PolicyError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountMeta {
    pub key: Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
}

impl AccountMeta {
    pub const fn new(key: Pubkey, is_signer: bool, is_writable: bool) -> Self {
        Self {
            key,
            is_signer,
            is_writable,
        }
    }

    pub fn with_signer(mut self, is_signer: bool) -> Self {
        self.is_signer = is_signer;
        self
    }

    pub fn with_writable(mut self, is_writable: bool) -> Self {
        self.is_writable = is_writable;
        self
    }
}

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
}
