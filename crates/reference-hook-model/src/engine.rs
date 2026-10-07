//! The model engine: applies a configuration to a transfer.

use hook_policy_model::{HookAuthorityPolicy, TransferContext};

use crate::{
    config::{ConfigAuthorization, HookModule, MintHookConfig},
    error::HookError,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HookEngine {
    config: MintHookConfig,
}

impl HookEngine {
    pub fn initialize(config: MintHookConfig) -> Result<Self, HookError> {
        config.validate().map_err(HookError::InvalidConfig)?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &MintHookConfig {
        &self.config
    }

    pub fn execute(&self, transfer: TransferContext) -> Result<(), HookError> {
        if transfer.mint != self.config.mint {
            return Err(HookError::WrongMint);
        }
        if transfer.source == [0; 32]
            || transfer.destination == [0; 32]
            || transfer.authority == [0; 32]
        {
            return Err(HookError::MissingTransferAccount);
        }
        if self
            .config
            .max_transfer_amount
            .is_some_and(|limit| transfer.amount > limit)
        {
            return Err(HookError::TransferLimitExceeded);
        }
        if self
            .config
            .enabled_modules
            .contains(&HookModule::AddressAllowList)
            && (!self.config.allowed_accounts.contains(&transfer.source)
                || !self.config.allowed_accounts.contains(&transfer.destination))
        {
            return Err(HookError::AddressNotAllowed);
        }
        if self
            .config
            .enabled_modules
            .contains(&HookModule::AddressDenyList)
            && (self.config.denied_accounts.contains(&transfer.source)
                || self.config.denied_accounts.contains(&transfer.destination))
        {
            return Err(HookError::AddressDenied);
        }
        Ok(())
    }

    pub fn reconfigure(
        &mut self,
        next: MintHookConfig,
        authorization: ConfigAuthorization,
    ) -> Result<(), HookError> {
        if self.config.authority_policy == HookAuthorityPolicy::ImmutableAtLaunch {
            return Err(HookError::ImmutableConfiguration);
        }
        if next.mint != self.config.mint
            || next.hook_program != self.config.hook_program
            || next.platform_authority != self.config.platform_authority
            || next.authority_policy != self.config.authority_policy
        {
            return Err(HookError::ImmutableFieldsChanged);
        }
        let authorized = match (self.config.authority_policy, authorization) {
            (HookAuthorityPolicy::PlatformRetained, ConfigAuthorization::Platform(signer))
            | (HookAuthorityPolicy::GovernedTimelock, ConfigAuthorization::Timelock(signer)) => {
                signer == self.config.platform_authority
            }
            _ => false,
        };
        if !authorized {
            return Err(HookError::UnauthorizedReconfiguration);
        }
        next.validate().map_err(HookError::InvalidConfig)?;
        self.config = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigError;
    use hook_policy_model::Pubkey;

    fn key(byte: u8) -> Pubkey {
        [byte; 32]
    }

    fn config() -> MintHookConfig {
        MintHookConfig {
            mint: key(1),
            hook_program: key(2),
            platform_authority: key(3),
            authority_policy: HookAuthorityPolicy::PlatformRetained,
            allowed_modules: vec![
                HookModule::TransferLimit,
                HookModule::AddressAllowList,
                HookModule::AddressDenyList,
            ],
            enabled_modules: vec![HookModule::TransferLimit],
            max_transfer_amount: Some(100),
            allowed_accounts: Vec::new(),
            denied_accounts: Vec::new(),
        }
    }

    fn transfer(amount: u64) -> TransferContext {
        TransferContext {
            source: key(4),
            mint: key(1),
            destination: key(5),
            authority: key(6),
            amount,
        }
    }

    #[test]
    fn validates_allow_list_membership_and_module_configurations() {
        let mut invalid = config();
        invalid.enabled_modules.push(HookModule::AddressAllowList);
        assert_eq!(
            invalid.validate(),
            Err(ConfigError::MissingModuleConfiguration)
        );

        let mut invalid = config();
        invalid.enabled_modules.push(HookModule::AddressAllowList);
        invalid.enabled_modules.push(HookModule::AddressDenyList);
        invalid.allowed_accounts.push(key(8));
        invalid.denied_accounts.push(key(7));
        assert_eq!(
            invalid.validate(),
            Err(ConfigError::IncompatibleAddressLists)
        );

        let mut invalid = config();
        invalid.enabled_modules.push(HookModule::AddressAllowList);
        invalid.allowed_accounts.push(key(7));
        invalid.allowed_accounts.push(key(7));
        assert_eq!(invalid.validate(), Err(ConfigError::DuplicateAddress));

        let mut invalid = config();
        invalid.enabled_modules.push(HookModule::AddressAllowList);
        invalid.allowed_accounts.push(key(7));
        invalid
            .allowed_modules
            .retain(|module| *module != HookModule::AddressAllowList);
        assert_eq!(invalid.validate(), Err(ConfigError::ModuleNotAllowed));
    }

    #[test]
    fn executes_transfer_limit_and_rejects_other_mints() {
        let engine = HookEngine::initialize(config()).unwrap();
        assert_eq!(engine.execute(transfer(100)), Ok(()));
        assert_eq!(
            engine.execute(transfer(101)),
            Err(HookError::TransferLimitExceeded)
        );
        let mut wrong_mint = transfer(1);
        wrong_mint.mint = key(9);
        assert_eq!(engine.execute(wrong_mint), Err(HookError::WrongMint));
    }

    #[test]
    fn enforces_address_lists_at_transfer_time() {
        let mut allowed = config();
        allowed.enabled_modules = vec![HookModule::AddressAllowList];
        allowed.max_transfer_amount = None;
        allowed.allowed_accounts = vec![key(4), key(5)];
        let engine = HookEngine::initialize(allowed).unwrap();
        assert_eq!(engine.execute(transfer(1)), Ok(()));
        let mut rejected = transfer(1);
        rejected.destination = key(8);
        assert_eq!(engine.execute(rejected), Err(HookError::AddressNotAllowed));

        let mut denied = config();
        denied.enabled_modules = vec![HookModule::AddressDenyList];
        denied.max_transfer_amount = None;
        denied.denied_accounts = vec![key(5)];
        let engine = HookEngine::initialize(denied).unwrap();
        assert_eq!(engine.execute(transfer(1)), Err(HookError::AddressDenied));
    }

    #[test]
    fn reconfiguration_obeys_authority_policy() {
        let mut engine = HookEngine::initialize(config()).unwrap();
        let mut next = config();
        next.max_transfer_amount = Some(200);
        assert_eq!(
            engine.reconfigure(next.clone(), ConfigAuthorization::Platform(key(8))),
            Err(HookError::UnauthorizedReconfiguration)
        );
        assert_eq!(
            engine.reconfigure(next.clone(), ConfigAuthorization::Platform(key(3))),
            Ok(())
        );

        let mut immutable_config = config();
        immutable_config.authority_policy = HookAuthorityPolicy::ImmutableAtLaunch;
        let mut immutable = HookEngine::initialize(immutable_config.clone()).unwrap();
        assert_eq!(
            immutable.reconfigure(immutable_config, ConfigAuthorization::Platform(key(3))),
            Err(HookError::ImmutableConfiguration)
        );
    }
}
