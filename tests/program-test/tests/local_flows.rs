//! Runs the driver's end-to-end flows inside ProgramTest against the **exact SBF artifacts that
//! are deployed to the integration devnet** (`target/integration-sbf`), signing the real admin
//! instructions with the deployer key from `.keys/` (the integration builds bake that key in as
//! admin). Every hook runs through both AMMs. The scaffolding is in this crate's `src/lib.rs`.
//!
//! ```text
//! cargo test -p program-test-flows --test local_flows -- --ignored --nocapture
//! ```

use program_test_flows::{arbitrary, reference, run, run_full, run_with, setup, template_id};
use raydium_hook_driver::HookSetup;
use raydium_hook_driver::{
    ArbitraryHook, CreatorCommitmentHook, FairLaunchHook, HolderRewardsHook, ReferenceHook,
};

/// One `#[tokio::test]` per (AMM, hook), each building its hook from the `Setup`.
macro_rules! flows {
    ($($name:ident: $amm:literal, |$setup:ident| $hook:expr;)+) => {$(
        #[tokio::test]
        #[ignore = "needs `cargo xtask localnet build` (or RTH_PROFILE=integration, see docs/forking.md)"]
        async fn $name() {
            let $setup = setup();
            let hook = $hook;
            run($amm, &hook, &$setup).await;
        }
    )+};
}

flows! {
    cpmm_with_the_reference_hook: "cpmm", |s| ReferenceHook {
        program_id: s.env.reference_hook_program().unwrap(),
        max_transfer: 500,
    };
    clmm_with_the_reference_hook: "clmm", |s| ReferenceHook {
        program_id: s.env.reference_hook_program().unwrap(),
        max_transfer: 500,
    };
    cpmm_with_an_unrelated_arbitrary_hook: "cpmm", |s| ArbitraryHook {
        program_id: s.env.arbitrary_hook_program().unwrap(),
        max_per_slot: 2,
    };
    clmm_with_an_unrelated_arbitrary_hook: "clmm", |s| ArbitraryHook {
        program_id: s.env.arbitrary_hook_program().unwrap(),
        max_per_slot: 2,
    };
    cpmm_with_the_creator_commitment_template: "cpmm", |_s| {
        CreatorCommitmentHook::new(template_id("creator_commitment"), 90)
    };
    clmm_with_the_creator_commitment_template: "clmm", |_s| {
        CreatorCommitmentHook::new(template_id("creator_commitment"), 90)
    };
    cpmm_with_the_fair_launch_template: "cpmm", |_s| {
        FairLaunchHook::new(template_id("fair_launch"), 150)
    };
    clmm_with_the_fair_launch_template: "clmm", |_s| {
        FairLaunchHook::new(template_id("fair_launch"), 150)
    };
    cpmm_with_the_fair_launch_per_slot_setting: "cpmm", |_s| {
        FairLaunchHook::per_slot_only(template_id("fair_launch"))
    };
    clmm_with_the_fair_launch_per_slot_setting: "clmm", |_s| {
        FairLaunchHook::per_slot_only(template_id("fair_launch"))
    };
    cpmm_with_the_holder_rewards_template: "cpmm", |_s| {
        HolderRewardsHook::new(template_id("holder_rewards"), 100)
    };
    clmm_with_the_holder_rewards_template: "clmm", |_s| {
        HolderRewardsHook::new(template_id("holder_rewards"), 100)
    };
    cpmm_with_the_holder_rewards_one_time_mode: "cpmm", |_s| {
        HolderRewardsHook::one_time(template_id("holder_rewards"), 100)
    };
    clmm_with_the_holder_rewards_one_time_mode: "clmm", |_s| {
        HolderRewardsHook::one_time(template_id("holder_rewards"), 100)
    };
}

/// Both mints hooked, each leg of every swap running its own hook.
macro_rules! combos {
    ($($name:ident: $amm:literal, |$s:ident| ($first:expr, $second:expr, $fee:expr);)+) => {$(
        #[tokio::test]
        #[ignore = "needs `cargo xtask localnet build` (or RTH_PROFILE=integration, see docs/forking.md)"]
        async fn $name() {
            let $s = setup();
            let (first, second) = ($first, $second);
            run_with($amm, &first, Some(&second), $fee, &$s).await;
        }
    )+};
}

combos! {
    cpmm_with_different_hooks_on_each_leg: "cpmm", |s| (reference(&s), arbitrary(&s), 0);
    clmm_with_different_hooks_on_each_leg: "clmm", |s| (reference(&s), arbitrary(&s), 0);
    cpmm_with_the_arbitrary_hook_on_mint_0_and_the_reference_on_mint_1: "cpmm", |s| (arbitrary(&s), reference(&s), 0);
    clmm_with_the_arbitrary_hook_on_mint_0_and_the_reference_on_mint_1: "clmm", |s| (arbitrary(&s), reference(&s), 0);
    cpmm_with_the_same_hook_program_on_both_legs: "cpmm", |s| (reference(&s), reference(&s), 0);
    clmm_with_the_same_hook_program_on_both_legs: "clmm", |s| (reference(&s), reference(&s), 0);
    cpmm_with_different_hooks_and_a_transfer_fee: "cpmm", |s| (reference(&s), arbitrary(&s), 500);
    clmm_with_different_hooks_and_a_transfer_fee: "clmm", |s| (reference(&s), arbitrary(&s), 500);
}

/// One hook on mint_0, with the Token-2022 TransferFee extension on both mints.
macro_rules! fee_flows {
    ($($name:ident: $amm:literal;)+) => {$(
        #[tokio::test]
        #[ignore = "needs `cargo xtask localnet build` (or RTH_PROFILE=integration, see docs/forking.md)"]
        async fn $name() {
            let s = setup();
            run_with($amm, &reference(&s), None, 500, &s).await;
        }
    )+};
}

fee_flows! {
    cpmm_with_a_transfer_fee_mint: "cpmm";
    clmm_with_a_transfer_fee_mint: "clmm";
}

/// CPMM exact-output swaps (`swap_base_output_v2`): hooked legs, with and without a transfer fee.
macro_rules! exact_output_flows {
    ($($name:ident: |$s:ident| ($first:expr, $second:expr, $fee:expr);)+) => {$(
        #[tokio::test]
        #[ignore = "needs `cargo xtask localnet build` (or RTH_PROFILE=integration, see docs/forking.md)"]
        async fn $name() {
            let $s = setup();
            let first = $first;
            let second: Option<Box<dyn HookSetup>> = $second;
            run_full("cpmm", &first, second.as_deref(), $fee, true, false, &$s).await;
        }
    )+};
}

// Hooks that cap swaps per slot (the arbitrary hook) are left out: the standard checks have used the
// slot's budget by the time the exact-output swaps run, and the slot does not advance in-process.
exact_output_flows! {
    cpmm_exact_output_with_the_reference_hook: |s| (reference(&s), None, 0);
    cpmm_exact_output_with_the_same_hook_program_on_both_legs: |s| (reference(&s), Some(Box::new(reference(&s))), 0);
    cpmm_exact_output_with_a_transfer_fee: |s| (reference(&s), None, 500);
}

/// CPMM pool creation, deposit, withdraw and fee collection with the hook live (the `_v2`
/// instructions). Hooks that cap swaps per slot are left out, as for the exact-output flows.
macro_rules! liquidity_flows {
    ($($name:ident: |$s:ident| ($first:expr, $second:expr, $fee:expr);)+) => {$(
        #[tokio::test]
        #[ignore = "needs `cargo xtask localnet build` (or RTH_PROFILE=integration, see docs/forking.md)"]
        async fn $name() {
            let $s = setup();
            let first = $first;
            let second: Option<Box<dyn HookSetup>> = $second;
            run_full("cpmm", &first, second.as_deref(), $fee, false, true, &$s).await;
        }
    )+};
}

liquidity_flows! {
    cpmm_liquidity_with_the_reference_hook: |s| (reference(&s), None, 0);
    cpmm_liquidity_with_the_same_hook_program_on_both_legs: |s| (reference(&s), Some(Box::new(reference(&s))), 0);
    cpmm_liquidity_with_a_transfer_fee: |s| (reference(&s), None, 500);
}
