//! Why a launch or a configuration change was refused.

use std::fmt;

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
