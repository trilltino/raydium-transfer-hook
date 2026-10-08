//! The keyless local profile: everything a clean checkout needs to run hooked Raydium swaps on a
//! real `solana-test-validator`, with no private key and no network beyond fetching the pinned
//! forks.
//!
//! * `build`: fetch the hook-support forks at their locked revisions, build them with their
//!   `localnet` feature (upstream program ids, admin baked in from `environments/localnet.json`),
//!   and build this repository's hooks, all into `target/localnet-sbf`.
//! * `validator`: start `solana-test-validator` with every program preloaded at the ids in
//!   `environments/localnet.json`, the admin as the faucet, and the CPMM fee receiver (an address
//!   nobody holds the key to) seeded as an empty wrapped-SOL account. Where the validator is not
//!   installed (it has no Windows build) and Docker is, it runs in a Linux container instead.
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
const DOCKER_IMAGE: &str = "rth-validator:agave-4.0.0";
const DOCKER_NAME: &str = "rth-validator";
const DOCKERFILE: &str = "FROM ubuntu:24.04
RUN apt-get update && apt-get install -y --no-install-recommends curl bzip2 ca-certificates libssl3 libudev1 && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /opt && curl -sSfL https://github.com/anza-xyz/agave/releases/download/v4.0.0/solana-release-x86_64-unknown-linux-gnu.tar.bz2 | tar xj -C /opt
ENV PATH=/opt/solana-release/bin:$PATH
";

/// This repository's hook programs: source directory, artifact name, environment key.
const HOOKS: &[(&str, &str, &str)] = &[
    (
        "templates/transfer-hook-starter",
        "transfer_hook_starter",
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
    (
        "templates/holder-rewards",
        "holder_rewards_hook",
        "holder_rewards",
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
        ["ui-fixture", rest @ ..] => ui_fixture(rest),
        _ => Err("usage: cargo xtask localnet <build | validator | e2e [--skip-build] [--amm cpmm|clmm|all] [--hook NAME|all] | ui-fixture --wallet PUBKEY --out FILE [--amm cpmm|clmm] [--hook NAME]>".into()),
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
        // Killing the Docker client does not stop the container.
        let _ = Command::new("docker")
            .args(["rm", "-f", DOCKER_NAME])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Run the validator in a container when it is not installed here (it has no Windows build) and
/// Docker is. `RTH_VALIDATOR=docker` forces the container, `RTH_VALIDATOR=native` forbids it.
fn use_docker() -> bool {
    match std::env::var("RTH_VALIDATOR").as_deref() {
        Ok("docker") => return true,
        Ok("native") => return false,
        _ => {}
    }
    let native = Command::new("solana-test-validator")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    !native
        && Command::new("docker")
            .arg("version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
}

/// Build the validator image once (it downloads the pinned Agave release).
fn ensure_validator_image() -> Result<()> {
    let present = Command::new("docker")
        .args(["image", "inspect", DOCKER_IMAGE])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if present {
        return Ok(());
    }
    println!("building {DOCKER_IMAGE} (downloads Agave v4.0.0, once)");
    let mut build = Command::new("docker")
        .args(["build", "-t", DOCKER_IMAGE, "-"])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("docker build: {e}"))?;
    {
        use std::io::Write;
        build
            .stdin
            .take()
            .ok_or("docker build has no stdin")?
            .write_all(DOCKERFILE.as_bytes())?;
    }
    let status = build.wait()?;
    if !status.success() {
        return Err(format!("docker build of {DOCKER_IMAGE} failed ({status})").into());
    }
    Ok(())
}

fn start_validator(stdout: Stdio) -> Result<Child> {
    let env = Env::load()?;
    let out = artifacts();
    let docker = use_docker();
    let fee_receiver = fee_receiver_file(&env)?;
    // Where the artifacts and the fee-receiver account are, as the validator sees them.
    let (so_dir, fee_receiver_arg) = if docker {
        ensure_validator_image()?;
        ("/sbf".to_string(), "/work/fee-receiver.json".to_string())
    } else {
        (
            out.display().to_string(),
            fee_receiver.display().to_string(),
        )
    };
    let mut command = if docker {
        // A container left over from an earlier run would hold the ports.
        let _ = Command::new("docker")
            .args(["rm", "-f", DOCKER_NAME])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let mut run = Command::new("docker");
        // Agave 4 needs io_uring, which Docker's default seccomp profile blocks. The validator
        // cannot bind 0.0.0.0 (gossip refuses an unspecified address), so it binds the container's
        // own address, which the published ports reach.
        run.args(["run", "--rm", "--name", DOCKER_NAME])
            .args(["--security-opt", "seccomp=unconfined", "--ulimit", "memlock=-1:-1"])
            .args(["-p", "8899:8899", "-p", "8900:8900"])
            .arg("-v")
            .arg(format!("{}:/sbf:ro", out.display()))
            .arg("-v")
            .arg(format!("{}:/work:ro", work_dir().display()))
            .arg(DOCKER_IMAGE)
            .args([
                "sh",
                "-c",
                "exec solana-test-validator --bind-address \"$(hostname -i | cut -d' ' -f1)\" --ledger /tmp/ledger \"$@\"",
                "sh",
            ]);
        run
    } else {
        let mut native = Command::new("solana-test-validator");
        native.arg("--ledger").arg(work_dir().join("ledger"));
        native
    };
    command
        .arg("--reset")
        .arg("--quiet")
        .args(["--mint", &env.admin])
        .arg("--account")
        .arg(&env.cpmm_fee_receiver)
        .arg(&fee_receiver_arg);
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
        command
            .arg("--bpf-program")
            .arg(env.program(key)?)
            .arg(format!("{so_dir}/{artifact}.so"));
    }
    if std::env::var_os("RTH_DEBUG").is_some() {
        eprintln!("{command:?}");
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

/// Run a CLI command and return whether it succeeded and everything it printed.
fn capture(args: &[&str]) -> Result<(bool, String)> {
    let output = cli(args).output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok((output.status.success(), text))
}

fn expect(what: &str, ok: bool, text: &str, wanted: &[&str], unwanted: &[&str]) -> Result<()> {
    let missing: Vec<&&str> = wanted.iter().filter(|w| !text.contains(**w)).collect();
    let present: Vec<&&str> = unwanted.iter().filter(|w| text.contains(**w)).collect();
    if !ok || !missing.is_empty() || !present.is_empty() {
        return Err(format!(
            "{what}: success={ok}, missing {missing:?}, unexpectedly present {present:?}\n{text}"
        )
        .into());
    }
    println!("  ok: {what}");
    Ok(())
}

/// `mint approve` and `mint approval` against the validator the flows just used: a fresh hookable
/// mint is unapproved, a dry run changes nothing, the real run approves it on both AMMs, a second
/// run skips it, and a key that is not the admin is refused.
fn approval_check(env: &str) -> Result<()> {
    let (ok, text) = capture(&[
        "mint",
        "create",
        "--env",
        env,
        "--keypair",
        ADMIN_KEYPAIR,
        "--hookable",
    ])?;
    if !ok {
        return Err(format!("mint create failed\n{text}").into());
    }
    let mint = text
        .lines()
        .find_map(|line| line.strip_prefix("mint "))
        .map(|rest| rest.trim().to_string())
        .ok_or("mint create printed no mint address")?;
    println!("  a fresh hookable mint: {mint}");

    let (ok, text) = capture(&["mint", "approval", "--env", env, "--mint", &mint])?;
    expect(
        "a new mint is not approved",
        ok,
        &text,
        &["NOT approved"],
        &["approved: a pool"],
    )?;

    let (ok, text) = capture(&[
        "mint",
        "approve",
        "--env",
        env,
        "--keypair",
        ADMIN_KEYPAIR,
        "--mint",
        &mint,
        "--dry-run",
    ])?;
    expect(
        "a dry run only simulates",
        ok,
        &text,
        &["would approve"],
        &["FAILED"],
    )?;
    let (ok, text) = capture(&["mint", "approval", "--env", env, "--mint", &mint])?;
    expect(
        "a dry run approved nothing",
        ok,
        &text,
        &["NOT approved"],
        &["approved: a pool"],
    )?;

    let (ok, text) = capture(&[
        "mint",
        "approve",
        "--env",
        env,
        "--keypair",
        ADMIN_KEYPAIR,
        "--mint",
        &mint,
    ])?;
    expect(
        "the admin approves the mint",
        ok,
        &text,
        &["approved"],
        &["FAILED", "NOT approved"],
    )?;
    let (ok, text) = capture(&["mint", "approval", "--env", env, "--mint", &mint])?;
    expect(
        "the mint is approved on both AMMs",
        ok,
        &text,
        &["approved: a pool can be created"],
        &["NOT approved", "INVALID"],
    )?;
    if text.matches("approved: a pool can be created").count() != 2 {
        return Err(format!("expected approval on both AMMs\n{text}").into());
    }

    let (ok, text) = capture(&[
        "mint",
        "approve",
        "--env",
        env,
        "--keypair",
        ADMIN_KEYPAIR,
        "--mint",
        &mint,
    ])?;
    expect(
        "approving again is a no-op",
        ok,
        &text,
        &["already approved"],
        &["FAILED"],
    )?;

    // Any key that is not the admin: the command refuses before sending anything. A committed
    // program keypair serves (nothing here depends on a directory a previous run left behind).
    let other = root().join("tests/fixtures/localnet/reference-hook.json");
    let other = other.to_string_lossy().to_string();
    let (ok, text) = capture(&[
        "mint",
        "approve",
        "--env",
        env,
        "--keypair",
        &other,
        "--mint",
        &mint,
    ])?;
    if ok || !text.contains("cannot approve mints") {
        return Err(format!("a non-admin key must be refused\n{text}").into());
    }
    println!("  ok: a key that is not the admin is refused");
    Ok(())
}

/// Set up a Fair Launch pool and a funded wallet on a validator that is already running, for the
/// browser UI and its end-to-end test. The launch window is an hour; the limits (raw units, 6
/// decimals) are 100 tokens per buy, 300 per account and three buys per slot.
fn ui_fixture(rest: &[&str]) -> Result<()> {
    let value = |name: &str| {
        rest.iter()
            .position(|a| *a == name)
            .and_then(|i| rest.get(i + 1).copied())
    };
    let wallet = value("--wallet").ok_or("ui-fixture needs --wallet PUBKEY")?;
    let amm = value("--amm").unwrap_or("cpmm");
    let hook = value("--hook").unwrap_or("fair-launch");
    let out = value("--out").ok_or("ui-fixture needs --out FILE")?;
    // The environment is copied so a run never rewrites the committed manifest.
    let env_copy = work_dir().join("localnet.json");
    fs::create_dir_all(work_dir())?;
    fs::copy(root().join(ENV_FILE), &env_copy)?;
    let env_copy = env_copy.to_string_lossy().to_string();
    run_checked(
        &mut cli(&[
            "ui-fixture",
            "--env",
            &env_copy,
            "--keypair",
            ADMIN_KEYPAIR,
            "--wallet",
            wallet,
            "--out",
            out,
            "--amm",
            amm,
            "--hook",
            hook,
            "--max-transfer",
            "1000000000000",
            "--window-seconds",
            "3600",
            "--vest-seconds",
            "3600",
            "--locked-total",
            "80000000",
            "--reward-seconds",
            "3600",
            "--reward-amount",
            "3600000000",
            "--max-buy",
            "100000000",
            "--max-wallet",
            "300000000",
            "--max-buys-per-slot",
            value("--max-buys-per-slot").unwrap_or("3"),
            "--max-priority",
            "1000",
            "--seed-amount",
            "2000000000",
            "--wallet-hooked-amount",
            "100000000",
            "--wallet-quote-amount",
            "1000000000",
        ]),
        "raydium-hook ui-fixture",
    )
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
    if amm == "all" {
        println!("==================== mint approval commands");
        if let Err(e) = approval_check(&env_copy) {
            failed.push(format!("mint approval commands: {e}"));
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
