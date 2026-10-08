//! Shared scaffolding for the Raydium + Transfer Hook flow tests: an in-process chain loaded with
//! the real SBF programs, and a runner that executes a flow and checks it recorded what its hooks
//! declare.
//!
//! The tests themselves are in `tests/local_flows.rs` here, and in `tests/third-party-hook`.
//!
//! Two profiles, chosen with `RTH_PROFILE`:
//!
//! * `localnet` (default): needs nothing outside a clean checkout. `cargo xtask localnet build`
//!   writes the artifacts to `target/localnet-sbf`: the hook-support Raydium forks built with their
//!   `localnet` feature (upstream program ids, admin = the throwaway key in
//!   `tests/fixtures/localnet`) and this repository's hooks. Ids come from
//!   `environments/localnet.json`.
//! * `integration`: the exact artifacts deployed to the integration devnet, in
//!   `target/integration-sbf`, signed with the real admin key from `.keys/` (see docs/forking.md).
//!
//! The tests are `#[ignore]` and fail loudly, not silently, when a prerequisite is missing.

use std::path::{Path, PathBuf};

use raydium_hook_driver::{
    env::Programs, run_clmm, run_cpmm, token::empty_wsol_account, ArbitraryHook, Direction,
    Environment, FlowInputs, HookSetup, LocalChain, ReferenceHook,
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

fn read_key(path: PathBuf) -> Keypair {
    read_keypair_file(&path)
        .unwrap_or_else(|e| panic!("missing prerequisite {}: {e}", path.display()))
}

/// A key from the git-ignored `.keys/` (integration profile only).
pub fn key(name: &str) -> Keypair {
    read_key(root().join(".keys").join(format!("{name}.json")))
}

/// A throwaway key committed under `tests/fixtures/localnet` (never funded on a public cluster).
pub fn fixture_key(name: &str) -> Keypair {
    read_key(
        root()
            .join("tests/fixtures/localnet")
            .join(format!("{name}.json")),
    )
}

pub fn clone(keypair: &Keypair) -> Keypair {
    Keypair::from_bytes(&keypair.to_bytes()).expect("keypair bytes")
}

/// The template hooks: the key in `programs.templates`, the SBF artifact name, and the program id
/// the integration profile uses for them (any id will do in-process).
pub const TEMPLATES: &[(&str, &str, [u8; 32])] = &[
    ("creator_commitment", "creator_commitment_hook", [0xC0; 32]),
    ("fair_launch", "fair_launch_hook", [0xF1; 32]),
    ("holder_rewards", "holder_rewards_hook", [0xC5; 32]),
];

/// The program id of a template hook in the active profile.
pub fn template_id(env_key: &str) -> Pubkey {
    setup()
        .env
        .template_program(env_key)
        .unwrap_or_else(|e| panic!("unknown template {env_key}: {e}"))
}

pub struct Setup {
    pub env: Environment,
    pub deployer: Keypair,
    /// The fee-receiver keypair (integration), or `None` when the fee-receiver account is seeded
    /// at genesis instead (localnet: nobody holds that key).
    pub fee_receiver: Option<Keypair>,
    pub artifacts: PathBuf,
}

pub fn profile() -> String {
    std::env::var("RTH_PROFILE").unwrap_or_else(|_| "localnet".into())
}

pub fn setup() -> Setup {
    let setup = match profile().as_str() {
        "localnet" => localnet_setup(),
        "integration" => integration_setup(),
        other => panic!("unknown RTH_PROFILE `{other}`: localnet or integration"),
    };
    let required = [
        "raydium_cp_swap.so".to_string(),
        "raydium_clmm.so".to_string(),
        "transfer_hook_starter.so".to_string(),
        "arbitrary_test_hook.so".to_string(),
    ]
    .into_iter()
    .chain(TEMPLATES.iter().map(|(_, name, _)| format!("{name}.so")));
    for file in required {
        let path = setup.artifacts.join(&file);
        assert!(
            path.exists(),
            "missing artifact {} (run `cargo xtask localnet build`)",
            path.display()
        );
    }
    std::env::set_var("SBF_OUT_DIR", &setup.artifacts);
    setup
}

fn localnet_setup() -> Setup {
    let mut env = Environment::load(root().join("environments/localnet.json"))
        .expect("environments/localnet.json");
    env.rpc_url = "in-process".into();
    let deployer = fixture_key("admin");
    assert_eq!(
        env.admin.as_deref(),
        Some(deployer.pubkey().to_string().as_str()),
        "environments/localnet.json's admin must be tests/fixtures/localnet/admin.json"
    );
    Setup {
        env,
        deployer,
        fee_receiver: None,
        artifacts: root().join("target/localnet-sbf"),
    }
}

fn integration_setup() -> Setup {
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
        fee_receiver: Some(fee_receiver),
        artifacts: root().join("target/integration-sbf"),
    }
}

pub async fn context(setup: &Setup) -> ProgramTestContext {
    context_with(setup, |_| {}).await
}

/// [`context`], with `extra` run on the `ProgramTest` first: add a program of your own (the
/// benchmark hook, say) to the same chain.
pub async fn context_with(
    setup: &Setup,
    extra: impl FnOnce(&mut ProgramTest),
) -> ProgramTestContext {
    let mut test = ProgramTest::default();
    extra(&mut test);
    test.add_program("raydium_cp_swap", setup.env.cpmm_program().unwrap(), None);
    test.add_program("raydium_clmm", setup.env.clmm_program().unwrap(), None);
    test.add_program(
        "transfer_hook_starter",
        setup.env.reference_hook_program().unwrap(),
        None,
    );
    test.add_program(
        "arbitrary_test_hook",
        setup.env.arbitrary_hook_program().unwrap(),
        None,
    );
    for (key, artifact, _) in TEMPLATES {
        test.add_program(artifact, setup.env.template_program(key).unwrap(), None);
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
    if setup.fee_receiver.is_none() {
        test.add_account(
            setup.env.cpmm_fee_receiver_key().unwrap(),
            empty_wsol_account(&setup.deployer.pubkey()),
        );
    }
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
    run_full(amm, hook, second, transfer_fee_bps, false, false, setup).await
}

/// [`run_with`], optionally with the exact-output swap checks too (CPMM only).
pub async fn run_full(
    amm: &str,
    hook: &dyn HookSetup,
    second: Option<&dyn HookSetup>,
    transfer_fee_bps: u16,
    exact_output: bool,
    liquidity: bool,
    setup: &Setup,
) {
    let mut context = context(setup).await;
    let mut chain = LocalChain::with_payer(&mut context, clone(&setup.deployer));
    let mut inputs = FlowInputs::new(&setup.env, hook, setup.fee_receiver.as_ref())
        .with_transfer_fee(transfer_fee_bps);
    if let Some(second) = second {
        inputs = inputs.with_second_hook(second);
    }
    if exact_output {
        inputs = inputs.with_exact_output();
    }
    if liquidity {
        inputs = inputs.with_liquidity();
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
    if exact_output {
        assert!(
            evidence
                .iter()
                .filter(|e| e.step.starts_with("hooked exact-output swap"))
                .count()
                == 2,
            "the flow must record the exact-output swap in both directions"
        );
    }
    if liquidity {
        let steps: &[&str] = match amm {
            "cpmm" => &[
                "hooked pool creation (initialize_v2)",
                "hooked deposit (deposit_v2)",
                "hooked withdraw (withdraw_v2)",
                "hooked protocol-fee collection (collect_protocol_fee_v2)",
                "hooked fund-fee collection (collect_fund_fee_v2)",
                "hooked pool creation with a permission record (initialize_with_permission_v2)",
                "hooked creator-fee collection (collect_creator_fee_v2)",
                "hooked creator-fee collection by anyone (collect_creator_fee_permissionless_v2)",
            ],
            _ => &[
                "hooked position opening (open_position_with_token22_nft_v3)",
                "hooked liquidity increase (increase_liquidity_v3)",
                "hooked position-fee collection (decrease_liquidity_v3 with zero liquidity)",
                "hooked protocol-fee collection (collect_protocol_fee_v2)",
                "hooked fund-fee collection (collect_fund_fee_v2)",
                "hooked liquidity removal (decrease_liquidity_v3)",
            ],
        };
        for step in steps {
            assert!(
                evidence.iter().any(|e| e.step == *step),
                "the flow must record `{step}`"
            );
        }
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
