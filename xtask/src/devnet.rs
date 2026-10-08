//! `env deploy-devnet`: build and deploy the integration stack to devnet, run the end-to-end flows
//! there and record the evidence. Every step is one of the commands a person would run by hand
//! (docs/forking.md); this only puts them in order and refuses to start without the keys.
//!
//! The integration builds of the forks bake in this environment's program ids, admin and fee
//! receiver (their `integration` feature), so the keys in `.keys/` must be the ones behind
//! the environment file (`--env`, default `environments/devnet.json`). Programs that already exist are skipped, never replaced, so the
//! recorded program ids are preserved.

use std::path::PathBuf;

use crate::localnet::{build_programs, cli, run_checked};
use crate::upstream::root;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const DEFAULT_ENV: &str = "environments/devnet.json";
const REQUIRED_KEYS: &[&str] = &["deployer.json", "cpmm-fee-receiver.json"];

pub fn run(args: &[&str]) -> Result<()> {
    match args {
        ["deploy-devnet", rest @ ..] => deploy_devnet(rest),
        // Build only, for upgrading programs that are already deployed (`solana program deploy
        // --program-id ...`): the integration builds bake in the environment's ids and admin.
        ["build-integration", ..] => build_programs(
            &root().join("target").join("integration-sbf"),
            "integration",
            None,
        ),
        _ => Err("usage: cargo xtask env build-integration | deploy-devnet [--env FILE] [--skip-build] [--amm cpmm|clmm|all] [--hook NAME|all] [--no-record]".into()),
    }
}

fn deploy_devnet(rest: &[&str]) -> Result<()> {
    let value = |name: &str| {
        rest.iter()
            .position(|a| *a == name)
            .and_then(|i| rest.get(i + 1).copied())
    };
    let env_file = value("--env").unwrap_or(DEFAULT_ENV);
    let keys = root().join(".keys");
    let missing: Vec<_> = REQUIRED_KEYS
        .iter()
        .filter(|k| !keys.join(k).exists())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "deploy-devnet needs the integration keys in .keys/ (missing: {missing:?}). They must be \
             the keys behind {env_file}; to deploy under your own, see docs/forking.md."
        )
        .into());
    }
    let keypair = keys.join("deployer.json").to_string_lossy().to_string();
    let fee_receiver = keys
        .join("cpmm-fee-receiver.json")
        .to_string_lossy()
        .to_string();
    let keys_dir = keys.to_string_lossy().to_string();
    let artifacts: PathBuf = root().join("target").join("integration-sbf");
    let artifacts_text = artifacts.to_string_lossy().to_string();

    run_checked(
        &mut crate::localnet::cargo_xtask(&["upstream", "verify"]),
        "upstream verify",
    )?;
    if !rest.contains(&"--skip-build") {
        build_programs(&artifacts, "integration", None)?;
    }
    run_checked(
        &mut cli(&[
            "deploy",
            "--env",
            env_file,
            "--keypair",
            &keypair,
            "--artifacts",
            &artifacts_text,
            "--keys",
            &keys_dir,
        ]),
        "raydium-hook deploy",
    )?;
    let mut e2e = vec![
        "e2e",
        "--env",
        env_file,
        "--keypair",
        &keypair,
        "--fee-receiver-keypair",
        &fee_receiver,
        "--amm",
        value("--amm").unwrap_or("all"),
        "--hook",
        value("--hook").unwrap_or("all"),
    ];
    if !rest.contains(&"--no-record") {
        e2e.push("--record");
    }
    run_checked(&mut cli(&e2e), "raydium-hook e2e")?;
    run_checked(
        &mut crate::localnet::cargo_xtask(&["devnet-doc", "--env", env_file]),
        "devnet-doc",
    )?;
    println!("\n{env_file} and docs/devnet.md updated; review and commit them");
    Ok(())
}
