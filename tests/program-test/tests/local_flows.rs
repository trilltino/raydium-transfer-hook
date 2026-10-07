//! Runs the driver's end-to-end flows inside ProgramTest against the **exact SBF artifacts that
//! are deployed to the integration devnet** (`target/integration-sbf`), signing the real admin
//! instructions with the deployer key from `.keys/` (the integration builds bake that key in as
//! admin). Both hooks (reference and the unrelated arbitrary one) run through both AMMs.
//!
//! Prerequisites (see docs/forking.md): build the four artifacts into
//! `target/integration-sbf` and have `.keys/{deployer,cpmm-fee-receiver,...}.json`. The tests are
//! `#[ignore]` and fail loudly, not silently, when a prerequisite is missing.
//!
//! ```text
//! cargo test -p program-test-flows --test local_flows -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use raydium_hook_driver::{
    env::Programs, run_clmm, run_cpmm, ArbitraryHook, CreatorCommitmentHook, Environment,
    FairLaunchHook, FlowInputs, HookSetup, LocalChain, LoyaltyRewardsHook, ReferenceHook,
};
use solana_program_test::{ProgramTest, ProgramTestContext};
use solana_sdk::{
    account::Account,
    signature::{read_keypair_file, Keypair, Signer},
    system_program,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn key(name: &str) -> Keypair {
    let path = root().join(".keys").join(format!("{name}.json"));
    read_keypair_file(&path)
        .unwrap_or_else(|e| panic!("missing prerequisite {}: {e}", path.display()))
}

fn clone(keypair: &Keypair) -> Keypair {
    Keypair::from_bytes(&keypair.to_bytes()).expect("keypair bytes")
}

struct Setup {
    env: Environment,
    deployer: Keypair,
    fee_receiver: Keypair,
}

fn setup() -> Setup {
    let artifacts = root().join("target/integration-sbf");
    for file in [
        "raydium_cp_swap.so",
        "raydium_clmm.so",
        "reference_hook_onchain.so",
        "arbitrary_test_hook.so",
        "creator_commitment_hook.so",
        "fair_launch_hook.so",
        "loyalty_rewards_hook.so",
    ] {
        assert!(
            artifacts.join(file).exists(),
            "missing artifact {}",
            artifacts.join(file).display()
        );
    }
    std::env::set_var("SBF_OUT_DIR", &artifacts);
    let deployer = key("deployer");
    let fee_receiver = key("cpmm-fee-receiver");
    let env = Environment {
        name: "local-integration".into(),
        cluster: "localnet".into(),
        rpc_url: "in-process".into(),
        kind: "integration".into(),
        programs: Programs {
            cpmm: Some(key("cpmm-program").pubkey().to_string()),
            clmm: Some(key("clmm-program").pubkey().to_string()),
            reference_hook: Some(key("hook-program").pubkey().to_string()),
            arbitrary_hook: Some(key("arbitrary-hook-program").pubkey().to_string()),
            templates: [
                (
                    "creator_commitment".to_string(),
                    creator_commitment_program().to_string(),
                ),
                ("fair_launch".to_string(), fair_launch_program().to_string()),
                (
                    "loyalty_rewards".to_string(),
                    loyalty_rewards_program().to_string(),
                ),
            ]
            .into(),
        },
        admin: Some(deployer.pubkey().to_string()),
        cpmm_fee_receiver: Some(fee_receiver.pubkey().to_string()),
        ..Default::default()
    };
    Setup {
        env,
        deployer,
        fee_receiver,
    }
}

async fn context(setup: &Setup) -> ProgramTestContext {
    let mut test = ProgramTest::default();
    test.add_program("raydium_cp_swap", setup.env.cpmm_program().unwrap(), None);
    test.add_program("raydium_clmm", setup.env.clmm_program().unwrap(), None);
    test.add_program(
        "reference_hook_onchain",
        setup.env.reference_hook_program().unwrap(),
        None,
    );
    test.add_program(
        "arbitrary_test_hook",
        setup.env.arbitrary_hook_program().unwrap(),
        None,
    );
    test.add_program(
        "creator_commitment_hook",
        creator_commitment_program(),
        None,
    );
    test.add_program("fair_launch_hook", fair_launch_program(), None);
    test.add_program("loyalty_rewards_hook", loyalty_rewards_program(), None);
    // The deployer is the programs' admin, so it signs and pays.
    test.add_account(
        setup.deployer.pubkey(),
        Account {
            lamports: 1_000_000_000_000,
            data: vec![],
            owner: system_program::id(),
            executable: false,
            rent_epoch: 0,
        },
    );
    let context = test.start_with_context().await;
    // create_pool / swap require block_timestamp > open_time (0).
    let mut clock: solana_sdk::clock::Clock = context.banks_client.get_sysvar().await.unwrap();
    clock.unix_timestamp = 1_700_000_000;
    context.set_sysvar(&clock);
    context
}

/// The template hooks need no key on disk locally: any program id will do.
fn creator_commitment_program() -> solana_sdk::pubkey::Pubkey {
    solana_sdk::pubkey::Pubkey::new_from_array([0xC0; 32])
}

fn fair_launch_program() -> solana_sdk::pubkey::Pubkey {
    solana_sdk::pubkey::Pubkey::new_from_array([0xF1; 32])
}

async fn run(amm: &str, hook: &dyn HookSetup, setup: &Setup) {
    let mut context = context(setup).await;
    let mut chain = LocalChain::with_payer(&mut context, clone(&setup.deployer));
    let inputs = FlowInputs {
        env: &setup.env,
        hook,
        fee_receiver_keypair: Some(&setup.fee_receiver),
    };
    println!("== {amm} through {}", hook.name());
    let evidence = match amm {
        "cpmm" => run_cpmm(&mut chain, &inputs).await,
        "clmm" => run_clmm(&mut chain, &inputs).await,
        other => panic!("unknown amm {other}"),
    }
    .unwrap_or_else(|e| panic!("{amm} flow with {} failed: {e}", hook.name()));
    // Measurements for the docs: compute units of the hooked swaps, and the largest transaction.
    for e in evidence
        .iter()
        .filter(|e| e.step.starts_with("hooked swap"))
    {
        println!("   {}: {}", e.step, e.detail);
    }
    println!(
        "   largest transaction: {} bytes (limit 1232)",
        chain.largest_transaction
    );
    assert!(
        evidence
            .iter()
            .any(|e| e.step.starts_with("hooked swap (hooked token in)")),
        "the flow must record the hooked swap"
    );
    assert!(
        evidence
            .iter()
            .filter(|e| e.step.starts_with("hook refused swap"))
            .count()
            == hook.refusals().len(),
        "the flow must record every refusal the hook declares"
    );
}

#[tokio::test]
#[ignore = "needs target/integration-sbf artifacts and .keys (see docs/forking.md)"]
async fn cpmm_with_the_reference_hook() {
    let setup = setup();
    let hook = ReferenceHook {
        program_id: setup.env.reference_hook_program().unwrap(),
        max_transfer: 500,
    };
    run("cpmm", &hook, &setup).await;
}

#[tokio::test]
#[ignore = "needs target/integration-sbf artifacts and .keys (see docs/forking.md)"]
async fn cpmm_with_an_unrelated_arbitrary_hook() {
    let setup = setup();
    let hook = ArbitraryHook {
        program_id: setup.env.arbitrary_hook_program().unwrap(),
        max_per_slot: 2,
    };
    run("cpmm", &hook, &setup).await;
}

#[tokio::test]
#[ignore = "needs target/integration-sbf artifacts and .keys (see docs/forking.md)"]
async fn clmm_with_the_reference_hook() {
    let setup = setup();
    let hook = ReferenceHook {
        program_id: setup.env.reference_hook_program().unwrap(),
        max_transfer: 500,
    };
    run("clmm", &hook, &setup).await;
}

#[tokio::test]
#[ignore = "needs target/integration-sbf artifacts and .keys (see docs/forking.md)"]
async fn clmm_with_an_unrelated_arbitrary_hook() {
    let setup = setup();
    let hook = ArbitraryHook {
        program_id: setup.env.arbitrary_hook_program().unwrap(),
        max_per_slot: 2,
    };
    run("clmm", &hook, &setup).await;
}

#[tokio::test]
#[ignore = "needs target/integration-sbf artifacts and .keys (see docs/forking.md)"]
async fn cpmm_with_the_creator_commitment_template() {
    let setup = setup();
    let hook = CreatorCommitmentHook::new(
        setup.env.template_program("creator_commitment").unwrap(),
        90,
    );
    run("cpmm", &hook, &setup).await;
}

#[tokio::test]
#[ignore = "needs target/integration-sbf artifacts and .keys (see docs/forking.md)"]
async fn clmm_with_the_creator_commitment_template() {
    let setup = setup();
    let hook = CreatorCommitmentHook::new(
        setup.env.template_program("creator_commitment").unwrap(),
        90,
    );
    run("clmm", &hook, &setup).await;
}

#[tokio::test]
#[ignore = "needs target/integration-sbf artifacts and .keys (see docs/forking.md)"]
async fn cpmm_with_the_fair_launch_template() {
    let setup = setup();
    let hook = FairLaunchHook::new(setup.env.template_program("fair_launch").unwrap(), 150);
    run("cpmm", &hook, &setup).await;
}

#[tokio::test]
#[ignore = "needs target/integration-sbf artifacts and .keys (see docs/forking.md)"]
async fn clmm_with_the_fair_launch_template() {
    let setup = setup();
    let hook = FairLaunchHook::new(setup.env.template_program("fair_launch").unwrap(), 150);
    run("clmm", &hook, &setup).await;
}

fn loyalty_rewards_program() -> solana_sdk::pubkey::Pubkey {
    solana_sdk::pubkey::Pubkey::new_from_array([0xC5; 32])
}

#[tokio::test]
#[ignore = "needs target/integration-sbf artifacts and .keys (see docs/forking.md)"]
async fn cpmm_with_the_loyalty_rewards_template() {
    let setup = setup();
    let hook = LoyaltyRewardsHook::new(setup.env.template_program("loyalty_rewards").unwrap(), 100);
    run("cpmm", &hook, &setup).await;
}

#[tokio::test]
#[ignore = "needs target/integration-sbf artifacts and .keys (see docs/forking.md)"]
async fn clmm_with_the_loyalty_rewards_template() {
    let setup = setup();
    let hook = LoyaltyRewardsHook::new(setup.env.template_program("loyalty_rewards").unwrap(), 100);
    run("clmm", &hook, &setup).await;
}
