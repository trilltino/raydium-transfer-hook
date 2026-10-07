//! The per-mint model configuration and its validation.

use std::fmt;

use hook_policy_model::{HookAuthorityPolicy, Pubkey};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookModule {
    TransferLimit,
    AddressAllowList,
    AddressDenyList,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MintHookConfig {
    pub mint: Pubkey,
    pub hook_program: Pubkey,
    pub platform_authority: Pubkey,
    pub authority_policy: HookAuthorityPolicy,
    pub allowed_modules: Vec<HookModule>,
    pub enabled_modules: Vec<HookModule>,
    pub max_transfer_amount: Option<u64>,
    pub allowed_accounts: Vec<Pubkey>,
    pub denied_accounts: Vec<Pubkey>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigError {
    MissingMint,
    MissingHookProgram,
    MissingPlatformAuthority,
    DuplicateModule,
    ModuleNotAllowed,
    MissingModuleConfiguration,
    UnexpectedModuleConfiguration,
    InvalidTransferLimit,
    EmptyAddressList,
    DuplicateAddress,
    IncompatibleAddressLists,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::MissingMint => "mint must be configured",
            Self::MissingHookProgram => "hook program must be configured",
            Self::MissingPlatformAuthority => "platform authority must be configured",
            Self::DuplicateModule => "module lists cannot contain duplicates",
            Self::ModuleNotAllowed => "enabled modules must be in the allow list",
            Self::MissingModuleConfiguration => "enabled module configuration is missing",
            Self::UnexpectedModuleConfiguration => {
                "module configuration requires its module to be enabled"
            }
            Self::InvalidTransferLimit => "transfer limit must be greater than zero",
            Self::EmptyAddressList => "address policy lists must not be empty",
            Self::DuplicateAddress => "address policy lists cannot contain duplicates",
            Self::IncompatibleAddressLists => {
                "allow-list and deny-list modules cannot be enabled together"
            }
        };
        f.write_str(message)
    }
}

impl std::error::Error for ConfigError {}

impl MintHookConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.mint == [0; 32] {
            return Err(ConfigError::MissingMint);
        }
        if self.hook_program == [0; 32] {
            return Err(ConfigError::MissingHookProgram);
        }
        if self.platform_authority == [0; 32] {
            return Err(ConfigError::MissingPlatformAuthority);
        }
        ensure_unique(&self.allowed_modules)?;
        ensure_unique(&self.enabled_modules)?;
        if self
            .enabled_modules
            .iter()
            .any(|module| !self.allowed_modules.contains(module))
        {
            return Err(ConfigError::ModuleNotAllowed);
        }

        let has_limit = self.enabled_modules.contains(&HookModule::TransferLimit);
        match (has_limit, self.max_transfer_amount) {
            (true, Some(limit)) if limit > 0 => {}
            (true, _) => return Err(ConfigError::InvalidTransferLimit),
            (false, Some(_)) => return Err(ConfigError::UnexpectedModuleConfiguration),
            (false, None) => {}
        }

        let has_allow_list = self.enabled_modules.contains(&HookModule::AddressAllowList);
        let has_deny_list = self.enabled_modules.contains(&HookModule::AddressDenyList);
        if has_allow_list && has_deny_list {
            return Err(ConfigError::IncompatibleAddressLists);
        }
        validate_address_list(has_allow_list, &self.allowed_accounts)?;
        validate_address_list(has_deny_list, &self.denied_accounts)?;
        Ok(())
    }
}

fn ensure_unique<T: Eq>(values: &[T]) -> Result<(), ConfigError> {
    for (index, value) in values.iter().enumerate() {
        if values[..index].contains(value) {
            return Err(ConfigError::DuplicateModule);
        }
    }
    Ok(())
}

fn validate_address_list(enabled: bool, addresses: &[Pubkey]) -> Result<(), ConfigError> {
    if enabled && addresses.is_empty() {
        return Err(ConfigError::MissingModuleConfiguration);
    }
    if !enabled && !addresses.is_empty() {
        return Err(ConfigError::UnexpectedModuleConfiguration);
    }
    for (index, address) in addresses.iter().enumerate() {
        if *address == [0; 32] {
            return Err(ConfigError::EmptyAddressList);
        }
        if addresses[..index].contains(address) {
            return Err(ConfigError::DuplicateAddress);
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigAuthorization {
    Platform(Pubkey),
    Timelock(Pubkey),
}
