#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use clmm_hook_integration::resolve_swap_v2_remaining_accounts;
    use cpmm_hook_integration::{resolve_deposit, resolve_swap_base_input, resolve_withdraw};
    use hook_policy_model::{
        AccountMeta, HookAuthorityPolicy, HookPolicy, HookPreset, LaunchConfig, PlatformConfig,
        Pubkey, TransferContext,
    };
    use launchlab_hook_integration::{LaunchLabLifecycle, LaunchPhase};
    use reference_hook_program::{HookEngine, HookError, HookModule, MintHookConfig};
    use transfer_hook_sdk::{
        MintAccount, SourceError, TransferHookAccountSource, TransferHookResolver,
        ValidationListAccount,
    };

    const TOKEN_2022: Pubkey = [9; 32];
    const TOKEN_PROGRAM: Pubkey = [8; 32];

    fn key(byte: u8) -> Pubkey {
        [byte; 32]
    }

    fn validation_address(hook_program: Pubkey, mint: Pubkey) -> Pubkey {
        let mut address = [0; 32];
        for (index, byte) in address.iter_mut().enumerate() {
            *byte = hook_program[index]
                .wrapping_add(mint[index].wrapping_mul(17))
                .wrapping_add(index as u8);
        }
        address
    }

    fn transfer(source: u8, mint: u8, destination: u8, amount: u64) -> TransferContext {
        TransferContext {
            source: key(source),
            mint: key(mint),
            destination: key(destination),
            authority: key(6),
            amount,
        }
    }

    #[derive(Default)]
    struct MemorySource {
        mints: HashMap<Pubkey, MintAccount>,
        lists: HashMap<Pubkey, ValidationListAccount>,
        extras: HashMap<Pubkey, Vec<AccountMeta>>,
        fetch_mint_count: usize,
        fetch_list_count: usize,
        resolved_contexts: Vec<TransferContext>,
    }

    impl MemorySource {
        fn add_mint(
            &mut self,
            mint: Pubkey,
            hook_program: Option<Pubkey>,
            extras: Vec<AccountMeta>,
        ) {
            self.mints.insert(
                mint,
                MintAccount {
                    key: mint,
                    owner: TOKEN_2022,
                    data_len: 82,
                    transfer_hook_program: hook_program,
                },
            );
            if let Some(hook_program) = hook_program {
                let address = validation_address(hook_program, mint);
                self.lists.insert(
                    address,
                    ValidationListAccount {
                        key: address,
                        owner: hook_program,
                        mint,
                        data_len: 12,
                        has_execute_discriminator: true,
                    },
                );
                self.extras.insert(mint, extras);
            }
        }
    }

    impl TransferHookAccountSource for MemorySource {
        fn validation_list_address(&self, hook_program: Pubkey, mint: Pubkey) -> Pubkey {
            validation_address(hook_program, mint)
        }

        fn fetch_mint(&mut self, mint: Pubkey) -> Result<Option<MintAccount>, SourceError> {
            self.fetch_mint_count += 1;
            Ok(self.mints.get(&mint).cloned())
        }

        fn fetch_validation_list(
            &mut self,
            address: Pubkey,
        ) -> Result<Option<ValidationListAccount>, SourceError> {
            self.fetch_list_count += 1;
            Ok(self.lists.get(&address).cloned())
        }

        fn resolve_extra_accounts(
            &mut self,
            validation_list: &ValidationListAccount,
            transfer: TransferContext,
        ) -> Result<Vec<AccountMeta>, SourceError> {
            self.resolved_contexts.push(transfer);
            self.extras
                .get(&validation_list.mint)
                .cloned()
                .ok_or_else(|| SourceError("missing test meta list".into()))
        }
    }

    fn hook_engine(mint: u8, hook_program: u8, denied: Vec<Pubkey>) -> HookEngine {
        let enabled_modules = if denied.is_empty() {
            Vec::new()
        } else {
            vec![HookModule::AddressDenyList]
        };
        HookEngine::initialize(MintHookConfig {
            mint: key(mint),
            hook_program: key(hook_program),
            platform_authority: key(3),
            authority_policy: HookAuthorityPolicy::PlatformRetained,
            allowed_modules: vec![
                HookModule::TransferLimit,
                HookModule::AddressAllowList,
                HookModule::AddressDenyList,
            ],
            enabled_modules,
            max_transfer_amount: None,
            allowed_accounts: Vec::new(),
            denied_accounts: denied,
        })
        .unwrap()
    }

    fn optional_platform() -> PlatformConfig {
        PlatformConfig::new(
            Some(key(7)),
            HookPolicy::Optional,
            HookAuthorityPolicy::PlatformRetained,
        )
    }

    #[test]
    fn no_hook_and_policy_failures_are_explicit() {
        let no_engine = PlatformConfig::without_program(
            HookPolicy::Optional,
            HookAuthorityPolicy::PlatformRetained,
        );
        assert_eq!(
            no_engine.validate_launch(LaunchConfig::with_preset(HookPreset::FairLaunch)),
            Err(hook_policy_model::PolicyError::PresetWithoutHook)
        );
        assert_eq!(
            PlatformConfig::without_program(
                HookPolicy::Mandatory,
                HookAuthorityPolicy::PlatformRetained
            )
            .validate_launch(LaunchConfig::new()),
            Err(hook_policy_model::PolicyError::MissingPlatformHook)
        );

        let mut launch =
            LaunchLabLifecycle::create(key(1), optional_platform(), LaunchConfig::new()).unwrap();
        assert_eq!(launch.phase(), LaunchPhase::MintCreated);
        launch.begin_trading().unwrap();

        let mut source = MemorySource::default();
        source.add_mint(key(1), None, Vec::new());
        source.mints.get_mut(&key(1)).unwrap().owner = TOKEN_PROGRAM;
        let resolved = launch
            .resolve_trade(
                &TransferHookResolver::new(TOKEN_PROGRAM, TOKEN_2022),
                &mut source,
                transfer(4, 1, 5, 20),
            )
            .unwrap();
        assert!(resolved.accounts.is_empty());
        assert_eq!(source.fetch_list_count, 0);
        let graduation = launch.graduate(key(1)).unwrap();
        assert_eq!(graduation.hook_program, None);
        assert!(!graduation.validation_list_initialized);
    }

    #[test]
    fn launchlab_initializes_hook_before_trading_and_preserves_it_on_graduation() {
        let engine = hook_engine(1, 7, Vec::new());
        let mut launch = LaunchLabLifecycle::create(
            key(1),
            optional_platform(),
            LaunchConfig::with_preset(HookPreset::FairLaunch),
        )
        .unwrap();
        assert_eq!(
            launch.initialize_hook(&engine, false),
            Err(launchlab_hook_integration::LaunchLabError::MissingValidationList)
        );
        launch.initialize_hook(&engine, true).unwrap();
        launch.begin_trading().unwrap();

        let mut source = MemorySource::default();
        source.add_mint(
            key(1),
            Some(key(7)),
            vec![AccountMeta::new(key(8), false, true)],
        );
        let plan = launch
            .resolve_trade(
                &TransferHookResolver::new(TOKEN_PROGRAM, TOKEN_2022),
                &mut source,
                transfer(4, 1, 5, 20),
            )
            .unwrap();
        assert_eq!(plan.accounts.len(), 3);

        let graduation = launch.graduate(key(1)).unwrap();
        assert_eq!(graduation.hook_program, Some(key(7)));
        assert!(graduation.validation_list_initialized);
    }

    #[test]
    fn mandatory_hook_uses_platform_engine_even_without_a_launch_preset() {
        let platform = PlatformConfig::new(
            Some(key(7)),
            HookPolicy::Mandatory,
            HookAuthorityPolicy::GovernedTimelock,
        );
        let mut launch = LaunchLabLifecycle::create(key(1), platform, LaunchConfig::new()).unwrap();
        let engine = hook_engine(1, 7, Vec::new());
        launch.initialize_hook(&engine, true).unwrap();
        launch.begin_trading().unwrap();

        let mut source = MemorySource::default();
        source.add_mint(
            key(1),
            Some(key(7)),
            vec![AccountMeta::new(key(8), false, true)],
        );
        let resolved = launch
            .resolve_trade(
                &TransferHookResolver::new(TOKEN_PROGRAM, TOKEN_2022),
                &mut source,
                transfer(4, 1, 5, 1),
            )
            .unwrap();
        assert_eq!(resolved.transfers[0].hook_program, Some(key(7)));
    }

    #[test]
    fn cpmm_swap_deposit_and_withdraw_keep_transfer_specific_slices() {
        let resolver = TransferHookResolver::new(TOKEN_PROGRAM, TOKEN_2022);
        let mut source = MemorySource::default();
        source.add_mint(
            key(10),
            Some(key(20)),
            vec![AccountMeta::new(key(30), false, true)],
        );
        source.add_mint(key(11), None, Vec::new());

        let swap = resolve_swap_base_input(
            &resolver,
            &mut source,
            transfer(1, 10, 2, 15),
            transfer(3, 11, 4, 12),
        )
        .unwrap();
        assert_eq!(swap.transfers.transfers[0].accounts, 0..3);
        assert_eq!(swap.transfers.transfers[1].accounts, 3..3);
        assert_eq!(swap.transfers.accounts.len(), 3);

        let deposit = resolve_deposit(
            &resolver,
            &mut source,
            transfer(1, 10, 7, 5),
            transfer(1, 11, 8, 5),
        )
        .unwrap();
        assert_eq!(deposit.transfers.transfers.len(), 2);
        let withdraw = resolve_withdraw(
            &resolver,
            &mut source,
            transfer(9, 10, 1, 5),
            transfer(9, 11, 1, 5),
        )
        .unwrap();
        assert_eq!(withdraw.transfers.transfers.len(), 2);
        assert_eq!(source.resolved_contexts[1].source, key(1));
        assert_eq!(source.resolved_contexts[1].destination, key(7));
    }

    #[test]
    fn cpmm_swap_with_both_mints_hooked_retains_duplicate_accounts_per_leg() {
        let resolver = TransferHookResolver::new(TOKEN_PROGRAM, TOKEN_2022);
        let mut source = MemorySource::default();
        source.add_mint(
            key(10),
            Some(key(20)),
            vec![AccountMeta::new(key(30), false, true)],
        );
        source.add_mint(
            key(11),
            Some(key(21)),
            vec![AccountMeta::new(key(30), false, false)],
        );

        let swap = resolve_swap_base_input(
            &resolver,
            &mut source,
            transfer(1, 10, 2, 15),
            transfer(3, 11, 4, 12),
        )
        .unwrap();
        assert_eq!(swap.transfers.transfers[0].accounts, 0..3);
        assert_eq!(swap.transfers.transfers[1].accounts, 3..6);
        assert_eq!(swap.transfers.accounts.len(), 6);
        assert_eq!(
            swap.transfers.accounts[0].key,
            swap.transfers.accounts[3].key
        );
        assert!(swap.transfers.accounts[0].is_writable);
        assert!(!swap.transfers.accounts[3].is_writable);
    }

    #[test]
    fn clmm_tick_and_bitmap_accounts_are_kept_outside_the_hook_tail() {
        let resolver = TransferHookResolver::new(TOKEN_PROGRAM, TOKEN_2022);
        let mut source = MemorySource::default();
        source.add_mint(
            key(10),
            Some(key(20)),
            vec![AccountMeta::new(key(30), false, true)],
        );
        source.add_mint(
            key(11),
            Some(key(21)),
            vec![AccountMeta::new(key(31), false, true)],
        );
        let tick_accounts = vec![
            AccountMeta::new(key(40), false, true),
            AccountMeta::new(key(41), false, true),
        ];

        let plan = resolve_swap_v2_remaining_accounts(
            &resolver,
            &mut source,
            tick_accounts,
            transfer(1, 10, 2, 10),
            transfer(3, 11, 4, 9),
        )
        .unwrap();
        assert_eq!(plan.hook_account_range(), 2..8);
        assert_eq!(plan.transfer_account_range(0), Some(2..5));
        assert_eq!(plan.transfer_account_range(1), Some(5..8));
        assert_eq!(plan.ordered_accounts().len(), 8);
        assert_eq!(plan.ordered_accounts()[0].key, key(40));
        assert_eq!(plan.ordered_accounts()[1].key, key(41));
    }

    #[test]
    fn stale_validation_list_is_refetched_and_rejected() {
        let resolver = TransferHookResolver::new(TOKEN_PROGRAM, TOKEN_2022);
        let mut source = MemorySource::default();
        source.add_mint(
            key(10),
            Some(key(20)),
            vec![AccountMeta::new(key(30), false, true)],
        );
        let context = transfer(1, 10, 2, 10);
        resolver
            .resolve_transfer_accounts(&mut source, context)
            .unwrap();
        let address = validation_address(key(20), key(10));
        source.lists.get_mut(&address).unwrap().owner = key(99);

        assert_eq!(
            resolver.resolve_transfer_accounts(&mut source, context),
            Err(transfer_hook_sdk::ResolveError::ValidationListOwnerMismatch)
        );
        assert_eq!(source.fetch_mint_count, 2);
        assert_eq!(source.fetch_list_count, 2);
    }

    #[test]
    fn hook_rejection_does_not_commit_earlier_transfer_effects() {
        let allowed_engine = hook_engine(10, 20, Vec::new());
        let rejecting_engine = hook_engine(11, 21, vec![key(6)]);
        let first = transfer(4, 10, 5, 10);
        let second = transfer(5, 11, 6, 10);
        let mut balances = HashMap::from([(key(4), 20), (key(5), 20), (key(6), 0)]);
        let initial = balances.clone();

        let result = apply_transfers_atomically(
            &mut balances,
            &[
                (first, Some(&allowed_engine)),
                (second, Some(&rejecting_engine)),
            ],
        );
        assert_eq!(result, Err(AtomicError::Hook(HookError::AddressDenied)));
        assert_eq!(balances, initial);
    }

    fn apply_transfers_atomically(
        balances: &mut HashMap<Pubkey, u64>,
        transfers: &[(TransferContext, Option<&HookEngine>)],
    ) -> Result<(), AtomicError> {
        let mut staged = balances.clone();
        for (transfer, engine) in transfers {
            if let Some(engine) = engine {
                engine.execute(*transfer).map_err(AtomicError::Hook)?;
            }
            let source_balance = staged
                .get(&transfer.source)
                .copied()
                .ok_or(AtomicError::MissingBalance)?;
            if source_balance < transfer.amount {
                return Err(AtomicError::InsufficientFunds);
            }
            let destination_balance = staged
                .get(&transfer.destination)
                .copied()
                .ok_or(AtomicError::MissingBalance)?;
            staged.insert(transfer.source, source_balance - transfer.amount);
            staged.insert(
                transfer.destination,
                destination_balance
                    .checked_add(transfer.amount)
                    .ok_or(AtomicError::BalanceOverflow)?,
            );
        }
        *balances = staged;
        Ok(())
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum AtomicError {
        Hook(HookError),
        MissingBalance,
        InsufficientFunds,
        BalanceOverflow,
    }
}
