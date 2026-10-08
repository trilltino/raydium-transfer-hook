//! The three platform-level choices: whether a hook is used, who may change it, and which
//! commercial preset a launch picks.

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
