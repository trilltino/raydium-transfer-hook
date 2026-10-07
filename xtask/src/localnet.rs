//! The keyless local profile: everything a clean checkout needs to run hooked Raydium swaps on a
//! real `solana-test-validator`, with no private key and no network beyond fetching the pinned
//! forks.
//!
//! * `build`: fetch the hook-support forks at their locked revisions, build them with their
//!   `localnet` feature (upstream program ids, admin baked in from `environments/localnet.json`),
//!   and build this repository's hooks, all into `target/localnet-sbf`.
//! * `validator`: start `solana-test-validator` with every program preloaded at the ids in
//!   `environments/localnet.json`, the admin as the faucet, and the CPMM fee receiver (an address
//!   nobody holds the key to) seeded as an empty wrapped-SOL account.
//! * `e2e`: `build` (unless `--skip-build`), start the validator, run `raydium-hook e2e` for every
//!   hook through both AMMs and for the starter built from source, then stop the validator.

use std::fs;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;

use crate::upstream::{fetch, load_lock, root, upstream_dir};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const ENV_FILE: &str = "environments/localnet.json";
const ADMIN_KEYPAIR: &str = "tests/fixtures/localnet/admin.json";
const RPC: &str = "http://127.0.0.1:8899";

/// This repository's hook programs: source directory, artifact name, environment key.
const HOOKS: &[(&str, &str, &str)] = &[
    (
        "programs/reference-hook-onchain",
        "reference_hook_onchain",
        "reference_hook",
    ),
    (
        "programs/arbitrary-test-hook",
        "arbitrary_test_hook",
        "arbitrary_hook",
    ),
    (
        "templates/creator-commitment",
        "creator_commitment_hook",
        "creator_commitment",
    ),
    ("templates/fair-launch", "fair_launch_hook", "fair_launch"),
    ("templates/anti-bundle", "anti_bundle_hook", "anti_bundle"),
    (
        "templates/loyalty-rewards",
        "loyalty_rewards_hook",
        "loyalty_rewards",
    ),
    (
        "templates/parent-spin-off",
        "parent_spin_off_hook",
        "parent_spin_off",
    ),
];

/// The two Raydium forks: lock name, artifact, admin env var their `localnet` feature reads.
const RAYDIUM: &[(&str, &str, &str)] = &[
    ("cpmm", "raydium_cp_swap", "CPSWAP_LOCALNET_ADMIN"),
    ("clmm", "raydium_clmm", "CLMM_LOCALNET_ADMIN"),
];

#[derive(Deserialize)]
struct Programs {
    cpmm: String,
    clmm: String,
    reference_hook: String,
    arbitrary_hook: String,
    templates: std::collections::BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Env {
    programs: Programs,
    admin: String,
    cpmm_fee_receiver: String,
}

impl Env {
    fn load() -> Result<Self> {
        let text = fs::read_to_string(root().join(ENV_FILE))?;
        Ok(serde_json::from_str(&text)?)
    }

    fn program(&self, key: &str) -> Result<&str> {
        Ok(match key {
            "cpmm" => &self.programs.cpmm,
            "clmm" => &self.programs.clmm,
            "reference_hook" => &self.programs.reference_hook,
            "arbitrary_hook" => &self.programs.arbitrary_hook,
            other => self
                .programs
                .templates
                .get(other)
                .ok_or_else(|| format!("{ENV_FILE} has no program `{other}`"))?,
        })
    }
}

fn artifacts() -> PathBuf {
    root().join("target").join("localnet-sbf")
}

fn work_dir() -> PathBuf {
    root().join("target").join("localnet")
}

pub fn run(args: &[&str]) -> Result<()> {
    match args {
        ["build", ..] => build(),
        ["validator", ..] => {
            let mut child = start_validator(Stdio::inherit())?;
            println!("validator running at {RPC}; Ctrl-C to stop");
            child.wait()?;
            Ok(())
        }
        ["e2e", rest @ ..] => {
            if !rest.contains(&"--skip-build") {
                build()?;
            }
            e2e(rest)
        }
        _ => Err("usage: cargo xtask localnet <build | validator | e2e [--skip-build] [--amm cpmm|clmm|all] [--hook NAME|all]>".into()),
    }
}

pub(crate) fn run_checked(command: &mut Command, what: &str) -> Result<()> {
    let status = command.status().map_err(|e| format!("{what}: {e}"))?;
    if !status.success() {
        return Err(format!("{what} failed ({status})").into());
    }
    Ok(())
}

fn build() -> Result<()> {
    let env = Env::load()?;
    Pubkey::from_str(&env.admin).map_err(|e| format!("{ENV_FILE} admin: {e}"))?;
    build_programs(&artifacts(), "localnet", Some(&env.admin))
}

/// Fetch the hook-support forks at their locked revisions and build them with `feature`, then
/// build every hook of this repository, all into `out`. `admin` is passed to the forks'
/// `localnet` feature (which reads it at build time); the `integration` feature bakes its own.
pub(crate) fn build_programs(
    out: &std::path::Path,
    feature: &str,
    admin: Option<&str>,
) -> Result<()> {
    fs::create_dir_all(out)?;
    fetch(&["cpmm", "clmm"], true, true)?;
    let lock = load_lock()?;
    for (name, artifact, admin_var) in RAYDIUM {
        let program_dir = lock[*name]
            .program_dir
            .as_deref()
            .ok_or_else(|| format!("upstream.lock.toml [{name}] has no program_dir"))?;
        let manifest = upstream_dir()
            .join(format!("{name}-hook"))
            .join(program_dir)
            .join("Cargo.toml");
        println!("\nbuilding {artifact} (`{feature}` feature)");
        let mut command = Command::new("cargo");
        command
            .args(["build-sbf", "--manifest-path"])
            .arg(&manifest)
            .arg("--sbf-out-dir")
            .arg(out)
            .args(["--features", feature]);
        if let Some(admin) = admin {
            command.env(admin_var, admin);
        }
        run_checked(&mut command, &format!("cargo build-sbf {artifact}"))?;
    }
    for (dir, artifact, _) in HOOKS {
        println!("\nbuilding {artifact}");
        run_checked(
            Command::new("cargo")
                .args(["build-sbf", "--manifest-path"])
                .arg(root().join(dir).join("Cargo.toml"))
                .arg("--sbf-out-dir")
                .arg(out),
            &format!("cargo build-sbf {artifact}"),
        )?;
    }
    println!("\nartifacts in {}", out.display());
    Ok(())
}

/// `solana-test-validator --account` file for the fee receiver.
fn fee_receiver_file(env: &Env) -> Result<PathBuf> {
    let admin = Pubkey::from_str(&env.admin)?;
    let account = raydium_adapters::token::empty_wsol_account(&admin);
    let json = serde_json::json!({
        "pubkey": env.cpmm_fee_receiver,
        "account": {
            "lamports": account.lamports,
            "data": [base64::engine::general_purpose::STANDARD.encode(&account.data), "base64"],
            "owner": account.owner.to_string(),
            "executable": false,
            "rentEpoch": 0,
            "space": account.data.len(),
        }
    });
    let path = work_dir().join("fee-receiver.json");
    fs::create_dir_all(work_dir())?;
    fs::write(&path, serde_json::to_vec_pretty(&json)?)?;
    Ok(path)
}

/// A running validator, stopped when dropped.
struct Validator(Child);

impl Drop for Validator {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_validator(stdout: Stdio) -> Result<Child> {
    let env = Env::load()?;
    let out = artifacts();
    let mut command = Command::new("solana-test-validator");
    command
        .arg("--reset")
        .arg("--quiet")
        .arg("--ledger")
        .arg(work_dir().join("ledger"))
        .args(["--mint", &env.admin])
        .arg("--account")
        .arg(&env.cpmm_fee_receiver)
        .arg(fee_receiver_file(&env)?);
    let programs = RAYDIUM
        .iter()
        .map(|(name, artifact, _)| (*name, *artifact))
        .chain(HOOKS.iter().map(|(_, artifact, key)| (*key, *artifact)));
    for (key, artifact) in programs {
        let so = out.join(format!("{artifact}.so"));
        if !so.exists() {
            return Err(format!(
                "missing {} (run `cargo xtask localnet build`)",
                so.display()
            )
            .into());
        }
        command.arg("--bpf-program").arg(env.program(key)?).arg(so);
    }
    let child = command.stdout(stdout).spawn().map_err(|e| {
        format!("could not start solana-test-validator (is the Solana CLI on PATH?): {e}")
    })?;
    Ok(child)
}

fn wait_for_rpc(child: &mut Child) -> Result<()> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Err(format!("solana-test-validator exited early ({status})").into());
        }
        if TcpStream::connect("127.0.0.1:8899").is_ok() {
            let ok = Command::new("solana")
                .args(["--url", RPC, "cluster-version"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if ok {
                return Ok(());
            }
        }
        if started.elapsed() > Duration::from_secs(120) {
            return Err("solana-test-validator did not answer within 120 s".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// `cargo xtask ARGS` from the workspace root (as a child process, so its output streams).
pub(crate) fn cargo_xtask(args: &[&str]) -> Command {
    let mut command = Command::new("cargo");
    command.current_dir(root()).arg("xtask").args(args);
    command
}

pub(crate) fn cli(args: &[&str]) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(root())
        .args(["run", "--quiet", "-p", "raydium-hook-cli", "--"])
        .args(args);
    command
}

fn e2e(rest: &[&str]) -> Result<()> {
    let value = |name: &str| {
        rest.iter()
            .position(|a| *a == name)
            .and_then(|i| rest.get(i + 1).copied())
    };
    let amm = value("--amm").unwrap_or("all");
    let hook = value("--hook").unwrap_or("all");
    let mut child = start_validator(Stdio::null())?;
    wait_for_rpc(&mut child)?;
    let _validator = Validator(child);
    println!("solana-test-validator ready at {RPC}\n");

    // The environment is copied so a run never rewrites the committed manifest.
    let env_copy = work_dir().join("localnet.json");
    fs::copy(root().join(ENV_FILE), &env_copy)?;
    let env_copy = env_copy.to_string_lossy().to_string();
    let keys = work_dir().join("keys").to_string_lossy().to_string();
    // A live validator really waits for the timed hooks; keep their windows short.
    let common = [
        "--env",
        &env_copy,
        "--keypair",
        ADMIN_KEYPAIR,
        "--amm",
        amm,
        "--keys",
        &keys,
        "--vest-seconds",
        "30",
        "--window-seconds",
        "30",
        "--reward-seconds",
        "30",
    ];
    let mut runs: Vec<(String, Vec<&str>)> =
        vec![(format!("shipped hooks ({hook})"), vec!["--hook", hook])];
    if hook == "all" {
        runs.push((
            "the starter, built from source and set up from its setup.json".into(),
            vec!["--hook-dir", "templates/transfer-hook-starter"],
        ));
    }
    let mut failed = Vec::new();
    for (label, extra) in &runs {
        println!("==================== {label}");
        let mut args = vec!["e2e"];
        args.extend_from_slice(&common);
        args.extend_from_slice(extra);
        if let Err(e) = run_checked(&mut cli(&args), label) {
            failed.push(e.to_string());
        }
    }
    if failed.is_empty() {
        println!("\nlocalnet e2e: every flow passed");
        Ok(())
    } else {
        Err(format!("localnet e2e failed:\n  {}", failed.join("\n  ")).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_localnet_manifest_names_every_program_the_validator_loads() {
        let env = Env::load().unwrap();
        for (key, _, _) in RAYDIUM {
            Pubkey::from_str(env.program(key).unwrap()).unwrap();
        }
        for (_, _, key) in HOOKS {
            Pubkey::from_str(env.program(key).unwrap()).unwrap();
        }
        Pubkey::from_str(&env.cpmm_fee_receiver).unwrap();
    }

    #[test]
    fn the_fixture_admin_is_the_manifest_admin() {
        let text = fs::read_to_string(root().join(ADMIN_KEYPAIR)).unwrap();
        let bytes: Vec<u8> = serde_json::from_str(&text).unwrap();
        let public = Pubkey::try_from(&bytes[32..64]).unwrap();
        assert_eq!(public.to_string(), Env::load().unwrap().admin);
    }

    #[test]
    fn hook_sources_exist() {
        for (dir, _, _) in HOOKS {
            assert!(root().join(dir).join("Cargo.toml").exists(), "{dir}");
        }
    }
}
