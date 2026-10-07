//! MODEL ONLY: end-to-end tests of the SDK, the integration planners, and the
//! policy model against an in-memory chain of real Token-2022 mint bytes and
//! real `ExtraAccountMetaList` accounts. No Raydium program is executed here;
//! runtime evidence lives in `programs/reference-hook-onchain/tests`.

#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use clmm_hook_integration::{clmm_swap_legs, plan_clmm_swap_v3};
    use cpmm_hook_integration::{cpmm_swap_legs, plan_cpmm_swap_base_input};
    use hook_policy_model::{
        HookAuthorityPolicy, HookPolicy, HookPreset, LaunchConfig, PlatformConfig, PolicyError,
        Pubkey as ModelKey, TransferContext,
    };
    use launchlab_hook_integration::{LaunchPhase, LaunchPolicySimulator, LaunchSimError};
    use reference_hook_model::{HookEngine, HookError, HookModule, MintHookConfig};
    use transfer_hook_sdk::{
        build_clmm_swap_v2, build_cpmm_swap_base_input_v1,
        solana_program::pubkey::Pubkey,
        spl_tlv_account_resolution::account::ExtraAccountMeta,
        testing::{block_on, MemoryChain},
        ClmmSwapAccounts, ClmmSwapArgs, CpmmSwapAccounts, FrameError, LegRole, ResolveOptions,
        SplResolveError, CLMM_SWAP_V2_DISCRIMINATOR, CLMM_SWAP_V3_DISCRIMINATOR,
        CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR, CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR,
    };

    fn model_key(key: Pubkey) -> ModelKey {
        key.to_bytes()
    }

    fn cpmm_accounts() -> CpmmSwapAccounts {
        CpmmSwapAccounts {
            payer: Pubkey::new_unique(),
            authority: Pubkey::new_unique(),
            amm_config: Pubkey::new_unique(),
            pool_state: Pubkey::new_unique(),
            input_token_account: Pubkey::new_unique(),
            output_token_account: Pubkey::new_unique(),
            input_vault: Pubkey::new_unique(),
            output_vault: Pubkey::new_unique(),
            input_token_program: transfer_hook_sdk::spl_token_2022::id(),
            output_token_program: transfer_hook_sdk::spl_token_2022::id(),
            input_token_mint: Pubkey::new_unique(),
            output_token_mint: Pubkey::new_unique(),
            observation_state: Pubkey::new_unique(),
        }
    }

    fn clmm_accounts() -> ClmmSwapAccounts {
        ClmmSwapAccounts {
            payer: Pubkey::new_unique(),
            amm_config: Pubkey::new_unique(),
            pool_state: Pubkey::new_unique(),
            input_token_account: Pubkey::new_unique(),
            output_token_account: Pubkey::new_unique(),
            input_vault: Pubkey::new_unique(),
            output_vault: Pubkey::new_unique(),
            observation_state: Pubkey::new_unique(),
            token_program: transfer_hook_sdk::spl_token::id(),
            token_program_2022: transfer_hook_sdk::spl_token_2022::id(),
            memo_program: Pubkey::new_unique(),
            input_vault_mint: Pubkey::new_unique(),
            output_vault_mint: Pubkey::new_unique(),
        }
    }

    fn extra(key: &Pubkey, writable: bool) -> ExtraAccountMeta {
        ExtraAccountMeta::new_with_pubkey(key, false, writable).unwrap()
    }

    fn hook_engine(mint: ModelKey, hook_program: ModelKey, denied: Vec<ModelKey>) -> HookEngine {
        let enabled_modules = if denied.is_empty() {
            Vec::new()
        } else {
            vec![HookModule::AddressDenyList]
        };
        HookEngine::initialize(MintHookConfig {
            mint,
            hook_program,
            platform_authority: [3; 32],
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

    fn optional_platform(hook: ModelKey) -> PlatformConfig {
        PlatformConfig::new(
            Some(hook),
            HookPolicy::Optional,
            HookAuthorityPolicy::PlatformRetained,
        )
    }

    #[test]
    fn policy_failures_are_explicit_and_no_hook_launches_trade_without_setup() {
        assert_eq!(
            PlatformConfig::without_program(
                HookPolicy::Optional,
                HookAuthorityPolicy::PlatformRetained
            )
            .validate_launch(LaunchConfig::with_preset(HookPreset::FairLaunch)),
            Err(PolicyError::PresetWithoutHook)
        );
        assert_eq!(
            PlatformConfig::without_program(
                HookPolicy::Mandatory,
                HookAuthorityPolicy::PlatformRetained
            )
            .validate_launch(LaunchConfig::new()),
            Err(PolicyError::MissingPlatformHook)
        );

        let mint = Pubkey::new_unique();
        let mut launch = LaunchPolicySimulator::create(
            model_key(mint),
            optional_platform([7; 32]),
            LaunchConfig::new(),
        )
        .unwrap();
        assert_eq!(launch.phase(), LaunchPhase::MintCreated);
        launch.begin_trading().unwrap();
        launch.check_trade_mint(model_key(mint)).unwrap();

        // A classic mint with no hook resolves unhooked under the launch's options.
        let mut chain = MemoryChain::new();
        chain.add_classic_mint(mint);
        let accounts = cpmm_accounts();
        let (mut input, output) = cpmm_swap_legs(&accounts, 20, 19);
        input.mint = mint;
        chain.add_classic_mint(accounts.output_token_mint);
        let plan = block_on(plan_cpmm_swap_base_input(
            input,
            output,
            &launch.resolve_options(),
            &ResolveOptions::default(),
            chain.fetcher(),
        ))
        .unwrap();
        assert_eq!(plan.hook_account_count(), 0);
        let graduation = launch.graduate(model_key(mint)).unwrap();
        assert_eq!(graduation.hook_program, None);
        assert!(!graduation.validation_list_initialized);
    }

    #[test]
    fn launch_policy_pins_the_hook_program_the_sdk_will_accept() {
        let accounts = cpmm_accounts();
        let platform_hook = Pubkey::new_unique();
        let engine = hook_engine(
            model_key(accounts.input_token_mint),
            model_key(platform_hook),
            Vec::new(),
        );
        let mut launch = LaunchPolicySimulator::create(
            model_key(accounts.input_token_mint),
            optional_platform(model_key(platform_hook)),
            LaunchConfig::with_preset(HookPreset::FairLaunch),
        )
        .unwrap();
        assert_eq!(
            launch.initialize_hook(&engine, false),
            Err(LaunchSimError::MissingValidationList)
        );
        launch.initialize_hook(&engine, true).unwrap();
        launch.begin_trading().unwrap();

        let (input, output) = cpmm_swap_legs(&accounts, 20, 19);
        let mut chain = MemoryChain::new();
        chain.add_hooked_mint(
            accounts.input_token_mint,
            platform_hook,
            None,
            &[extra(&Pubkey::new_unique(), false)],
        );
        chain.add_classic_mint(accounts.output_token_mint);
        let plan = block_on(plan_cpmm_swap_base_input(
            input,
            output,
            &launch.resolve_options(),
            &ResolveOptions::default(),
            chain.fetcher(),
        ))
        .unwrap();
        assert_eq!(plan.hook_account_count(), 3);
        let graduation = launch
            .graduate(model_key(accounts.input_token_mint))
            .unwrap();
        assert_eq!(graduation.hook_program, Some(model_key(platform_hook)));
        assert!(graduation.validation_list_initialized);

        // The same launch rejects a mint hooked to some other program.
        let rogue_hook = Pubkey::new_unique();
        chain.add_hooked_mint(accounts.input_token_mint, rogue_hook, None, &[]);
        let (input, output) = cpmm_swap_legs(&accounts, 20, 19);
        let error = block_on(plan_cpmm_swap_base_input(
            input,
            output,
            &launch.resolve_options(),
            &ResolveOptions::default(),
            chain.fetcher(),
        ))
        .unwrap_err();
        assert_eq!(error.leg, LegRole::Input);
        assert_eq!(
            error.source,
            SplResolveError::UnexpectedHookProgram {
                expected: platform_hook,
                found: rogue_hook
            }
        );
    }

    #[test]
    fn a_mandatory_launch_rejects_an_unhooked_mint() {
        let accounts = cpmm_accounts();
        let launch = LaunchPolicySimulator::create(
            model_key(accounts.input_token_mint),
            PlatformConfig::with_program(
                [7; 32],
                HookPolicy::Mandatory,
                HookAuthorityPolicy::GovernedTimelock,
            ),
            LaunchConfig::new(),
        )
        .unwrap();
        let mut chain = MemoryChain::new();
        chain.add_unhooked_token_2022_mint(accounts.input_token_mint);
        chain.add_classic_mint(accounts.output_token_mint);
        let (input, output) = cpmm_swap_legs(&accounts, 20, 19);
        let error = block_on(plan_cpmm_swap_base_input(
            input,
            output,
            &launch.resolve_options(),
            &ResolveOptions::default(),
            chain.fetcher(),
        ))
        .unwrap_err();
        assert_eq!(error.source, SplResolveError::HookRequired);
    }

    #[test]
    fn cpmm_swap_with_one_hooked_leg_keeps_transfer_specific_slices() {
        let accounts = cpmm_accounts();
        let mut chain = MemoryChain::new();
        chain.add_hooked_mint(
            accounts.input_token_mint,
            Pubkey::new_unique(),
            None,
            &[extra(&Pubkey::new_unique(), false)],
        );
        chain.add_classic_mint(accounts.output_token_mint);
        let (input, output) = cpmm_swap_legs(&accounts, 15, 12);
        let plan = block_on(plan_cpmm_swap_base_input(
            input,
            output,
            &ResolveOptions::default(),
            &ResolveOptions::default(),
            chain.fetcher(),
        ))
        .unwrap();
        let mut instruction = build_cpmm_swap_base_input_v1(Pubkey::new_unique(), &accounts, 15, 1);
        let framed = plan.frame(&mut instruction).unwrap().unwrap();
        assert_eq!(instruction.data[..8], CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR);
        assert_eq!(
            (framed.input_hook_accounts, framed.output_hook_accounts),
            (3, 0)
        );
        assert_eq!(instruction.accounts.len(), 13 + 3);
    }

    #[test]
    fn cpmm_swap_with_no_hooks_is_a_byte_identical_v1_swap() {
        let accounts = cpmm_accounts();
        let mut chain = MemoryChain::new();
        chain.add_classic_mint(accounts.input_token_mint);
        chain.add_classic_mint(accounts.output_token_mint);
        let (input, output) = cpmm_swap_legs(&accounts, 15, 12);
        let plan = block_on(plan_cpmm_swap_base_input(
            input,
            output,
            &ResolveOptions::default(),
            &ResolveOptions::default(),
            chain.fetcher(),
        ))
        .unwrap();
        let mut instruction = build_cpmm_swap_base_input_v1(Pubkey::new_unique(), &accounts, 15, 1);
        let before = instruction.clone();
        assert_eq!(plan.frame(&mut instruction), Ok(None));
        assert_eq!(instruction, before);
        assert_eq!(instruction.data[..8], CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR);
    }

    #[test]
    fn cpmm_swap_with_both_mints_hooked_retains_duplicate_accounts_per_leg() {
        let accounts = cpmm_accounts();
        let shared = Pubkey::new_unique();
        let mut chain = MemoryChain::new();
        // The same writable extra on the input leg, readonly on the output leg:
        // the resolver would accept each, but the framer must refuse the escalation.
        chain.add_hooked_mint(
            accounts.input_token_mint,
            Pubkey::new_unique(),
            None,
            &[extra(&shared, true)],
        );
        chain.add_hooked_mint(
            accounts.output_token_mint,
            Pubkey::new_unique(),
            None,
            &[extra(&shared, false)],
        );
        let options = ResolveOptions::default().with_privilege_policy(
            transfer_hook_sdk::PrivilegePolicy::allowing_writable([shared]),
        );
        let (input, output) = cpmm_swap_legs(&accounts, 15, 12);
        let plan = block_on(plan_cpmm_swap_base_input(
            input,
            output,
            &options,
            &options,
            chain.fetcher(),
        ))
        .unwrap();
        let mut instruction = build_cpmm_swap_base_input_v1(Pubkey::new_unique(), &accounts, 15, 1);
        let snapshot = instruction.clone();
        assert!(matches!(
            plan.frame(&mut instruction),
            Err(FrameError::CrossSlicePrivilegeConflict { address, .. }) if address == shared
        ));
        assert_eq!(instruction, snapshot);

        // With equal flags both copies are kept, one per leg, in leg order.
        chain.add_hooked_mint(
            accounts.output_token_mint,
            Pubkey::new_unique(),
            None,
            &[extra(&shared, true)],
        );
        let (input, output) = cpmm_swap_legs(&accounts, 15, 12);
        let plan = block_on(plan_cpmm_swap_base_input(
            input,
            output,
            &options,
            &options,
            chain.fetcher(),
        ))
        .unwrap();
        plan.frame(&mut instruction).unwrap().unwrap();
        assert_eq!(instruction.accounts[13].pubkey, shared);
        assert_eq!(instruction.accounts[16].pubkey, shared);
        assert_eq!(instruction.accounts.len(), 13 + 6);
    }

    #[test]
    fn clmm_tick_and_bitmap_accounts_stay_outside_the_hook_tail() {
        let accounts = clmm_accounts();
        let mut chain = MemoryChain::new();
        chain.add_hooked_mint(
            accounts.input_vault_mint,
            Pubkey::new_unique(),
            None,
            &[extra(&Pubkey::new_unique(), false)],
        );
        chain.add_hooked_mint(
            accounts.output_vault_mint,
            Pubkey::new_unique(),
            None,
            &[extra(&Pubkey::new_unique(), false)],
        );
        let ticks = [Pubkey::new_unique(), Pubkey::new_unique()];
        let bitmap = Pubkey::new_unique();
        let mut instruction = build_clmm_swap_v2(
            Pubkey::new_unique(),
            &accounts,
            &ticks,
            Some(bitmap),
            ClmmSwapArgs {
                amount: 10,
                other_amount_threshold: 9,
                sqrt_price_limit_x64: 0,
                is_base_input: true,
            },
        );
        assert_eq!(instruction.data[..8], CLMM_SWAP_V2_DISCRIMINATOR);
        let (input, output) = clmm_swap_legs(&accounts, 10, 9);
        let plan = block_on(plan_clmm_swap_v3(
            2,
            1,
            input,
            output,
            &ResolveOptions::default(),
            &ResolveOptions::default(),
            chain.fetcher(),
        ))
        .unwrap();
        let framed = plan.frame(&mut instruction).unwrap().unwrap();
        assert_eq!(instruction.data[..8], CLMM_SWAP_V3_DISCRIMINATOR);
        assert_eq!(instruction.accounts[13].pubkey, ticks[0]);
        assert_eq!(instruction.accounts[14].pubkey, ticks[1]);
        assert_eq!(instruction.accounts[15].pubkey, bitmap);
        assert_eq!(framed.input_range, 16..19);
        assert_eq!(framed.output_range, 19..22);
    }

    #[test]
    fn stale_validation_list_is_detected_before_signing() {
        let accounts = cpmm_accounts();
        let hook = Pubkey::new_unique();
        let mut chain = MemoryChain::new();
        chain.add_hooked_mint(
            accounts.input_token_mint,
            hook,
            None,
            &[extra(&Pubkey::new_unique(), false)],
        );
        chain.add_classic_mint(accounts.output_token_mint);
        let (input, output) = cpmm_swap_legs(&accounts, 15, 12);
        let plan = block_on(plan_cpmm_swap_base_input(
            input,
            output,
            &ResolveOptions::default(),
            &ResolveOptions::default(),
            chain.fetcher(),
        ))
        .unwrap();
        block_on(plan.verify_unchanged(chain.fetcher())).unwrap();

        chain.add_hooked_mint(
            accounts.input_token_mint,
            hook,
            None,
            &[extra(&Pubkey::new_unique(), false)],
        );
        let error = block_on(plan.verify_unchanged(chain.fetcher())).unwrap_err();
        assert_eq!(error.leg, LegRole::Input);
        assert!(matches!(
            error.source,
            SplResolveError::ValidationListChanged { .. }
        ));
    }

    #[test]
    fn hook_rejection_does_not_commit_earlier_transfer_effects() {
        let allowed_engine = hook_engine([10; 32], [20; 32], Vec::new());
        let rejecting_engine = hook_engine([11; 32], [21; 32], vec![[6; 32]]);
        let first = transfer(4, 10, 5, 10);
        let second = transfer(5, 11, 6, 10);
        let mut balances = HashMap::from([([4; 32], 20), ([5; 32], 20), ([6; 32], 0)]);
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

    fn transfer(source: u8, mint: u8, destination: u8, amount: u64) -> TransferContext {
        TransferContext {
            source: [source; 32],
            mint: [mint; 32],
            destination: [destination; 32],
            authority: [6; 32],
            amount,
        }
    }

    fn apply_transfers_atomically(
        balances: &mut HashMap<ModelKey, u64>,
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
