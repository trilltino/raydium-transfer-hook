//! Shared scaffolding for the Raydium + Transfer Hook flow tests: the in-process environment (the
//! exact SBF artifacts deployed to the integration devnet, the real admin key) and a runner that
//! executes a flow and checks it recorded what its hooks declare.
//!
//! The tests themselves are in `tests/local_flows.rs` here, and in `tests/third-party-hook`.
//!
//! Prerequisites (see docs/forking.md): build the artifacts into `target/integration-sbf` and have
//! `.keys/{deployer,cpmm-fee-receiver,...}.json`. The tests are `#[ignore]` and fail loudly, not
//! silently, when a prerequisite is missing.

use std::path::{Path, PathBuf};

use raydium_hook_driver::{
    env::Programs, run_clmm, run_cpmm, ArbitraryHook, Direction, Environment, FlowInputs,
    HookSetup, LocalChain, ReferenceHook,
};
use solana_program_test::{ProgramTest, ProgramTestContext};
use solana_sdk::{
    account::Account,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
    system_program,
};

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn key(name: &str) -> Keypair {
    let path = root().join(".keys").join(format!("{name}.json"));
    read_keypair_file(&path)
        .unwrap_or_else(|e| panic!("missing prerequisite {}: {e}", path.display()))
}

pub fn clone(keypair: &Keypair) -> Keypair {
    Keypair::from_bytes(&keypair.to_bytes()).expect("keypair bytes")
}

/// The template hooks: the key in `programs.templates`, the SBF artifact name, and a program id.
/// They need no key on disk locally: any program id will do.
pub const TEMPLATES: &[(&str, &str, [u8; 32])] = &[
    ("creator_commitment", "creator_commitment_hook", [0xC0; 32]),
    ("fair_launch", "fair_launch_hook", [0xF1; 32]),
    ("loyalty_rewards", "loyalty_rewards_hook", [0xC5; 32]),
    ("anti_bundle", "anti_bundle_hook", [0xAB; 32]),
    ("parent_spin_off", "parent_spin_off_hook", [0xE5; 32]),
];

pub fn template_id(env_key: &str) -> Pubkey {
    let (_, _, id) = TEMPLATES
        .iter()
        .find(|(key, _, _)| *key == env_key)
        .unwrap_or_else(|| panic!("unknown template {env_key}"));
    Pubkey::new_from_array(*id)
}

pub struct Setup {
    pub env: Environment,
    pub deployer: Keypair,
    pub fee_receiver: Keypair,
}

pub fn setup() -> Setup {
    let artifacts = root().join("target/integration-sbf");
    let required = [
        "raydium_cp_swap.so".to_string(),
        "raydium_clmm.so".to_string(),
        "reference_hook_onchain.so".to_string(),
        "arbitrary_test_hook.so".to_string(),
    ]
    .into_iter()
    .chain(TEMPLATES.iter().map(|(_, name, _)| format!("{name}.so")));
    for file in required {
        assert!(
            artifacts.join(&file).exists(),
            "missing artifact {}",
            artifacts.join(&file).display()
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
            templates: TEMPLATES
                .iter()
                .map(|(key, _, id)| (key.to_string(), Pubkey::new_from_array(*id).to_string()))
                .collect(),
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

pub async fn context(setup: &Setup) -> ProgramTestContext {
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
    for (_, artifact, id) in TEMPLATES {
        test.add_program(artifact, Pubkey::new_from_array(*id), None);
    }
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

pub async fn run(amm: &str, hook: &dyn HookSetup, setup: &Setup) {
    run_with(amm, hook, None, 0, setup).await
}

/// Run one flow. `second` is a hook on the other mint too (both legs hooked); `transfer_fee_bps`
/// puts a transfer fee on both mints.
pub async fn run_with(
    amm: &str,
    hook: &dyn HookSetup,
    second: Option<&dyn HookSetup>,
    transfer_fee_bps: u16,
    setup: &Setup,
) {
    let mut context = context(setup).await;
    let mut chain = LocalChain::with_payer(&mut context, clone(&setup.deployer));
    let mut inputs = FlowInputs::new(&setup.env, hook, Some(&setup.fee_receiver))
        .with_transfer_fee(transfer_fee_bps);
    if let Some(second) = second {
        inputs = inputs.with_second_hook(second);
    }
    println!(
        "== {amm} through {}{}{}",
        hook.name(),
        second
            .map(|s| format!(" + {}", s.name()))
            .unwrap_or_default(),
        if transfer_fee_bps > 0 {
            format!(" (transfer fee {transfer_fee_bps} bps)")
        } else {
            String::new()
        }
    );
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
            == expected_refusals(hook, second),
        "the flow must record every refusal the hooks declare"
    );
}

/// How many refusals a flow records. With a single hook, all it declares. With two hooked legs
/// only the refusals where the refusing hook's own mint is the swap's input are run (that transfer
/// comes first, so the failure can only come from that hook).
pub fn expected_refusals(hook: &dyn HookSetup, second: Option<&dyn HookSetup>) -> usize {
    match second {
        None => hook.refusals().len(),
        Some(second) => [hook, second]
            .iter()
            .flat_map(|h| h.refusals())
            .filter(|r| r.direction == Direction::HookedIn)
            .count(),
    }
}

pub fn reference(setup: &Setup) -> ReferenceHook {
    ReferenceHook {
        program_id: setup.env.reference_hook_program().unwrap(),
        max_transfer: 500,
    }
}

pub fn arbitrary(setup: &Setup) -> ArbitraryHook {
    ArbitraryHook {
        program_id: setup.env.arbitrary_hook_program().unwrap(),
        max_per_slot: 2,
    }
}
